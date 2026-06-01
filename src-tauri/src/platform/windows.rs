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

use std::process::Command;

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
