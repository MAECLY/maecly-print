//! Windows printer enumeration via the print spooler (`EnumPrinters`) and the
//! GDI `DeviceCapabilities` query.
//!
//! `EnumPrinters` (level 2) gives the core record (name, port, driver,
//! location, shared, status, queued jobs); `DeviceCapabilities` adds
//! color/duplex/paper/tray/resolution support. Toner levels are not exposed by
//! the Win32 API (they need SNMP/WMI/IPP) and stay empty here; network
//! discovery (WSD/mDNS) is a documented follow-up and returns empty.
//!
//! All field reads that touch the spooler buffer are `unsafe`; capability
//! queries are defensive — any failure yields empty rather than aborting the
//! whole enumeration.

use std::os::windows::process::CommandExt;
use std::process::Command;

use anyhow::Context;
use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Graphics::Printing::{
    EnumPrintersW, GetDefaultPrinterW, SetDefaultPrinterW, PRINTER_ATTRIBUTE_SHARED,
    PRINTER_ENUM_CONNECTIONS, PRINTER_ENUM_LOCAL, PRINTER_INFO_2W, PRINTER_STATUS_BUSY,
    PRINTER_STATUS_DOOR_OPEN, PRINTER_STATUS_ERROR, PRINTER_STATUS_NO_TONER,
    PRINTER_STATUS_OFFLINE, PRINTER_STATUS_PAPER_JAM, PRINTER_STATUS_PAPER_OUT,
    PRINTER_STATUS_PRINTING, PRINTER_STATUS_PROCESSING,
};
// DeviceCapabilities and the DC_* constants live under Storage::Xps in windows 0.58.
use windows::Win32::Storage::Xps::{
    DeviceCapabilitiesW, DC_BINNAMES, DC_COLORDEVICE, DC_DUPLEX, DC_ENUMRESOLUTIONS, DC_PAPERNAMES,
    PRINTER_DEVICE_CAPABILITIES,
};

use crate::ipc::printers::{
    DiscoveredPrinter, Printer, PrinterKind, PrinterStatus, Supplies, Tray,
};

/// Read a (possibly null) wide string pointer into an owned `String`.
fn pwstr(p: PWSTR) -> String {
    if p.is_null() {
        String::new()
    } else {
        unsafe { p.to_string().unwrap_or_default() }
    }
}

pub fn list_printers() -> anyhow::Result<Vec<Printer>> {
    let flags = PRINTER_ENUM_LOCAL | PRINTER_ENUM_CONNECTIONS;
    let mut needed = 0u32;
    let mut returned = 0u32;

    // First call sizes the buffer (returns an insufficient-buffer error).
    let _ = unsafe { EnumPrintersW(flags, PCWSTR::null(), 2, None, &mut needed, &mut returned) };
    if needed == 0 {
        return Ok(Vec::new());
    }

    let mut buf = vec![0u8; needed as usize];
    unsafe {
        EnumPrintersW(
            flags,
            PCWSTR::null(),
            2,
            Some(buf.as_mut_slice()),
            &mut needed,
            &mut returned,
        )?;
    }

    let default = default_printer().unwrap_or(None);
    let infos = buf.as_ptr() as *const PRINTER_INFO_2W;
    let mut out = Vec::with_capacity(returned as usize);
    for i in 0..returned as usize {
        let info = unsafe { &*infos.add(i) };
        out.push(printer_from(info, default.as_deref()));
    }
    Ok(out)
}

pub fn default_printer() -> anyhow::Result<Option<String>> {
    let mut len = 0u32;
    // First call sizes the buffer.
    let _ = unsafe { GetDefaultPrinterW(PWSTR::null(), &mut len) };
    if len == 0 {
        return Ok(None);
    }
    let mut buf = vec![0u16; len as usize];
    let ok = unsafe { GetDefaultPrinterW(PWSTR(buf.as_mut_ptr()), &mut len) };
    if !ok.as_bool() {
        return Ok(None);
    }
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    let name = String::from_utf16_lossy(&buf[..end]);
    Ok(if name.is_empty() { None } else { Some(name) })
}

pub fn discover_printers() -> anyhow::Result<Vec<DiscoveredPrinter>> {
    // WSD/mDNS discovery on Windows is a documented follow-up.
    Ok(Vec::new())
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Run a CLI and succeed only on a zero exit status.
fn run_ok(program: &str, args: &[&str]) -> anyhow::Result<()> {
    let status = Command::new(program).args(args).status()?;
    if status.success() {
        Ok(())
    } else {
        anyhow::bail!("`{program}` exited with {status}");
    }
}

/// Set the current user's default printer (no elevation required).
pub fn set_os_default(name: &str) -> anyhow::Result<()> {
    let w = wide(name);
    if unsafe { SetDefaultPrinterW(PCWSTR(w.as_ptr())) }.as_bool() {
        Ok(())
    } else {
        anyhow::bail!("SetDefaultPrinter failed");
    }
}

/// Print the Windows test page via the print UI helper.
pub fn print_test_page(name: &str) -> anyhow::Result<()> {
    run_ok("rundll32", &["printui.dll,PrintUIEntry", "/k", "/n", name])
}

/// Open Settings → Printers & scanners.
pub fn open_printer_settings() -> anyhow::Result<()> {
    run_ok("cmd", &["/C", "start", "", "ms-settings:printers"])
}

// ----------------------------------------------------------------------------
// Adding a printer (`Add-Printer` via PowerShell)
// ----------------------------------------------------------------------------

/// `CreateProcess` flag that suppresses the transient console window a
/// `powershell.exe` child would otherwise flash on screen.
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Install a printer for `uri` using the built-in Microsoft IPP Class Driver.
///
/// Only `ipp`/`ipps` URIs are supported here — raw sockets/LPD/WSD need the
/// system dialog, so those are rejected with a message pointing at "Open
/// printer settings". The IPP port is created on demand if `Add-Printer`
/// reports it is missing. Adding a printer can require elevation; when the
/// spooler returns access-denied we surface guidance rather than a raw error.
pub fn add_printer(name: &str, uri: &str) -> anyhow::Result<()> {
    let scheme = uri
        .split("://")
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    if scheme != "ipp" && scheme != "ipps" {
        anyhow::bail!(
            "On Windows, printers using {scheme}:// must be added via system settings \
             (Open printer settings)"
        );
    }

    let port = ipp_port_name(uri)?;
    let script = add_printer_script(name, &port);
    match run_powershell(&script) {
        Ok(()) => Ok(()),
        Err(e) => {
            let msg = e.to_string();
            let lower = msg.to_ascii_lowercase();
            // Elevation problems can't be retried away — creating the port
            // needs the same rights — so surface actionable guidance first.
            // The HRESULT is checked too because the message text is localized
            // on non-English Windows.
            if msg.contains("0x80070005") || (lower.contains("access") && lower.contains("denied"))
            {
                anyhow::bail!(
                    "Adding a printer may require administrator rights. Use \"Open printer \
                     settings\" to add it from Windows."
                );
            }
            Err(e)
        }
    }
}

/// Run a Windows PowerShell 5.1 one-liner, returning its captured stderr as the
/// error text on failure so the spooler's own message reaches the UI.
fn run_powershell(script: &str) -> anyhow::Result<()> {
    let out = Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .with_context(|| "failed to spawn powershell.exe")?;
    if out.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&out.stderr);
    let msg = stderr.trim();
    if msg.is_empty() {
        anyhow::bail!("powershell exited with {}", out.status);
    }
    anyhow::bail!("{msg}");
}

/// Map an IPP/IPPS device URI to the port name Windows expects.
///
/// `ipp://host[:port]/path` becomes `IPP://host/path` (and `ipps://` →
/// `IPPS://…`). The default IPP port 631 is dropped; any explicit non-default
/// port is kept as `host:port`.
fn ipp_port_name(uri: &str) -> anyhow::Result<String> {
    let (scheme, rest) = uri
        .split_once("://")
        .with_context(|| format!("not a URI: {uri}"))?;
    let prefix = match scheme.to_ascii_lowercase().as_str() {
        "ipp" => "IPP",
        "ipps" => "IPPS",
        other => anyhow::bail!("unsupported scheme for an IPP port: {other}"),
    };
    let (authority, path) = rest.split_once('/').unwrap_or((rest, ""));
    if authority.is_empty() {
        anyhow::bail!("URI has no host: {uri}");
    }
    // Split host from port. Bracketed IPv6 authorities ("[::1]:631") must not
    // be split at their first ':', which is inside the brackets.
    let (bare_host, port) = if let Some(v6) = authority.strip_prefix('[') {
        match v6.split_once(']') {
            Some((h, tail)) => (
                format!("[{h}]"),
                tail.strip_prefix(':').unwrap_or(tail).to_string(),
            ),
            None => anyhow::bail!("malformed IPv6 authority: {authority}"),
        }
    } else {
        match authority.split_once(':') {
            Some((h, p)) => (h.to_string(), p.to_string()),
            None => (authority.to_string(), String::new()),
        }
    };
    let host = if port.is_empty() || port == "631" {
        bare_host
    } else {
        format!("{bare_host}:{port}")
    };
    if path.is_empty() {
        Ok(format!("{prefix}://{host}"))
    } else {
        Ok(format!("{prefix}://{host}/{path}"))
    }
}

/// Quote a value for a single-quoted PowerShell string literal (embedded single
/// quotes are doubled, which is PowerShell's own escaping rule).
fn ps_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

/// The PowerShell script binding `name` to the built-in IPP class driver on
/// `port`.
///
/// `$ErrorActionPreference = 'Stop'` turns non-terminating cmdlet errors into
/// terminating ones, so PowerShell exits non-zero instead of reporting success
/// with an error on stderr. The port is pre-created inside a `try/catch` that
/// swallows the already-exists case — locale-independent, unlike matching
/// "port does not exist" against localized stderr.
fn add_printer_script(name: &str, port: &str) -> String {
    let port = ps_quote(port);
    format!(
        "$ErrorActionPreference = 'Stop'; try {{ Add-PrinterPort -Name {port} }} catch {{ }}; \
         Add-Printer -Name {} -DriverName 'Microsoft IPP Class Driver' -PortName {port}",
        ps_quote(name),
    )
}

fn printer_from(info: &PRINTER_INFO_2W, default: Option<&str>) -> Printer {
    let name = pwstr(info.pPrinterName);
    let port = pwstr(info.pPortName);
    let driver = pwstr(info.pDriverName);
    let device = PCWSTR(info.pPrinterName.0);
    let port_w = PCWSTR(info.pPortName.0);

    let shared = (info.Attributes & PRINTER_ATTRIBUTE_SHARED) != 0;

    Printer {
        id: name.clone(),
        name: if name.is_empty() {
            port.clone()
        } else {
            name.clone()
        },
        kind: infer_kind(&driver),
        color: dc_flag(device, port_w, DC_COLORDEVICE) == 1,
        duplex: dc_flag(device, port_w, DC_DUPLEX) == 1,
        status: status_from(info.Status, info.cJobs),
        is_default: default == Some(name.as_str()),
        location: pwstr(info.pLocation),
        connection: if port.is_empty() {
            "Local".into()
        } else {
            port
        },
        driver,
        shared,
        queued_jobs: info.cJobs,
        max_dpi: dc_resolution(device, port_w),
        papers: dc_names(device, port_w, DC_PAPERNAMES, 64),
        trays: dc_names(device, port_w, DC_BINNAMES, 24)
            .into_iter()
            .map(|name| Tray {
                name,
                paper: None,
                level: None,
            })
            .collect(),
        // Win32 does not expose supply levels.
        supplies: Supplies::default(),
        default_tray: None,
        default_paper: None,
    }
}

fn status_from(status: u32, jobs: u32) -> PrinterStatus {
    let err = PRINTER_STATUS_ERROR
        | PRINTER_STATUS_PAPER_JAM
        | PRINTER_STATUS_PAPER_OUT
        | PRINTER_STATUS_NO_TONER
        | PRINTER_STATUS_DOOR_OPEN;
    let busy = PRINTER_STATUS_PRINTING | PRINTER_STATUS_PROCESSING | PRINTER_STATUS_BUSY;
    if status & PRINTER_STATUS_OFFLINE != 0 {
        PrinterStatus::Offline
    } else if status & err != 0 {
        PrinterStatus::Error
    } else if status & busy != 0 || jobs > 0 {
        PrinterStatus::Busy
    } else {
        PrinterStatus::Ready
    }
}

/// A single-value `DeviceCapabilities` query (e.g. `DC_DUPLEX`, `DC_COLORDEVICE`).
fn dc_flag(device: PCWSTR, port: PCWSTR, cap: PRINTER_DEVICE_CAPABILITIES) -> i32 {
    unsafe { DeviceCapabilitiesW(device, port, cap, PWSTR::null(), None) }
}

/// A name-array `DeviceCapabilities` query (`DC_PAPERNAMES` @64, `DC_BINNAMES` @24).
fn dc_names(
    device: PCWSTR,
    port: PCWSTR,
    cap: PRINTER_DEVICE_CAPABILITIES,
    width: usize,
) -> Vec<String> {
    let count = unsafe { DeviceCapabilitiesW(device, port, cap, PWSTR::null(), None) };
    if count <= 0 {
        return Vec::new();
    }
    let mut buf = vec![0u16; count as usize * width];
    let got = unsafe { DeviceCapabilitiesW(device, port, cap, PWSTR(buf.as_mut_ptr()), None) };
    if got <= 0 {
        return Vec::new();
    }
    buf.chunks(width)
        .take(got as usize)
        .map(|chunk| {
            let end = chunk.iter().position(|&c| c == 0).unwrap_or(chunk.len());
            String::from_utf16_lossy(&chunk[..end])
        })
        .filter(|s| !s.is_empty())
        .collect()
}

/// Max supported resolution from `DC_ENUMRESOLUTIONS` (pairs of `LONG` x,y).
fn dc_resolution(device: PCWSTR, port: PCWSTR) -> Option<u32> {
    let count =
        unsafe { DeviceCapabilitiesW(device, port, DC_ENUMRESOLUTIONS, PWSTR::null(), None) };
    if count <= 0 {
        return None;
    }
    let mut buf = vec![0i32; count as usize * 2];
    let got = unsafe {
        DeviceCapabilitiesW(
            device,
            port,
            DC_ENUMRESOLUTIONS,
            PWSTR(buf.as_mut_ptr() as *mut u16),
            None,
        )
    };
    if got <= 0 {
        return None;
    }
    buf.chunks(2)
        .take(got as usize)
        .map(|p| p[0].max(p[1]))
        .max()
        .map(|x| x as u32)
}

fn infer_kind(driver: &str) -> PrinterKind {
    let d = driver.to_ascii_lowercase();
    if d.contains("pdf") || d.contains("xps") || d.contains("onenote") || d.contains("fax") {
        PrinterKind::Virtual
    } else if d.contains("zebra") || d.contains("label") || d.contains("zpl") || d.contains("dymo")
    {
        PrinterKind::Label
    } else if d.contains("designjet") || d.contains("wide") || d.contains("plotter") {
        PrinterKind::Wide
    } else if d.contains("mfp") || d.contains("workforce") || d.contains("pixma") {
        PrinterKind::Mfp
    } else {
        PrinterKind::Laser
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_priority() {
        assert!(matches!(
            status_from(PRINTER_STATUS_OFFLINE, 0),
            PrinterStatus::Offline
        ));
        assert!(matches!(
            status_from(PRINTER_STATUS_PAPER_JAM, 0),
            PrinterStatus::Error
        ));
        assert!(matches!(
            status_from(PRINTER_STATUS_PRINTING, 0),
            PrinterStatus::Busy
        ));
        assert!(matches!(status_from(0, 3), PrinterStatus::Busy));
        assert!(matches!(status_from(0, 0), PrinterStatus::Ready));
    }

    #[test]
    fn ipp_port_name_strips_default_port() {
        assert_eq!(
            ipp_port_name("ipp://printer.local:631/ipp/print").unwrap(),
            "IPP://printer.local/ipp/print"
        );
    }

    #[test]
    fn ipp_port_name_handles_ipps_and_paths() {
        assert_eq!(
            ipp_port_name("ipps://host/ipp/print").unwrap(),
            "IPPS://host/ipp/print"
        );
    }

    #[test]
    fn ipp_port_name_keeps_explicit_port() {
        assert_eq!(
            ipp_port_name("ipp://host:9631/ipp/print").unwrap(),
            "IPP://host:9631/ipp/print"
        );
    }

    #[test]
    fn ipp_port_name_rejects_invalid() {
        assert!(ipp_port_name("not-a-uri").is_err());
        assert!(ipp_port_name("socket://host/queue").is_err());
    }

    #[test]
    fn ipp_port_name_handles_bracketed_ipv6() {
        assert_eq!(
            ipp_port_name("ipp://[fe80::1]:631/ipp/print").unwrap(),
            "IPP://[fe80::1]/ipp/print"
        );
        assert_eq!(
            ipp_port_name("ipp://[fe80::1]:9631/ipp/print").unwrap(),
            "IPP://[fe80::1]:9631/ipp/print"
        );
        assert!(ipp_port_name("ipp://[fe80::1/ipp/print").is_err());
    }

    #[test]
    fn ps_quote_escapes_single_quotes() {
        assert_eq!(ps_quote("plain"), "'plain'");
        assert_eq!(ps_quote("O'Brien"), "'O''Brien'");
    }

    #[test]
    fn add_printer_script_shape() {
        let s = add_printer_script("Front Desk", "IPP://host/ipp/print");
        assert!(s.starts_with("$ErrorActionPreference = 'Stop';"));
        assert!(s.contains("try { Add-PrinterPort -Name 'IPP://host/ipp/print' } catch { }"));
        assert!(s.contains("Add-Printer -Name 'Front Desk'"));
        assert!(s.contains("-DriverName 'Microsoft IPP Class Driver'"));
        assert!(s.contains("-PortName 'IPP://host/ipp/print'"));
    }

    #[test]
    fn kind_inference() {
        assert!(matches!(
            infer_kind("Microsoft Print to PDF"),
            PrinterKind::Virtual
        ));
        assert!(matches!(
            infer_kind("ZDesigner ZD420 ZPL"),
            PrinterKind::Label
        ));
        assert!(matches!(
            infer_kind("HP LaserJet Pro M404"),
            PrinterKind::Laser
        ));
    }

    /// Enumeration must not panic/error on a real Windows host (CI runner has
    /// Microsoft Print to PDF). Tolerates any count.
    #[test]
    fn list_printers_smoke() {
        let printers = list_printers().expect("list_printers should not error");
        for p in &printers {
            assert!(!p.id.is_empty());
        }
        if let Some(def) = default_printer().unwrap() {
            assert!(printers.iter().any(|p| p.id == def));
        }
    }
}
