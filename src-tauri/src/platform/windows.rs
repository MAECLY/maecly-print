//! Windows printer backend: enumeration via the print spooler (`EnumPrinters`)
//! and the GDI `DeviceCapabilities` query, setting the default printer,
//! adding via PowerShell's `Add-Printer -IppURL`, and network discovery via
//! the OS DNS-SD client (`DnsServiceBrowse`).
//!
//! The spooler queue itself is never paused, resumed or deleted from here:
//! *Remove* on the Printers screen is a MaeclyPrint-only soft-disable
//! (`docs/05-features/printers-management.md`), and OS-level queue management
//! is left to Windows (*Open printer settings*).
//!
//! `EnumPrinters` (level 2) gives the core record (name, port, driver,
//! location, shared, status, attributes, queued jobs, default DEVMODE);
//! `DeviceCapabilities` adds color/duplex/paper/tray/resolution support and the
//! paper/bin *ids* that let the default DEVMODE be named. Toner levels are not
//! exposed by the Win32 API (they need SNMP/WMI/IPP) and stay empty here.
//!
//! All field reads that touch the spooler buffer are `unsafe`; capability
//! queries are defensive — any failure yields empty rather than aborting the
//! whole enumeration. A spooler that is not running is the exception: it is an
//! error, never an empty list, so the UI can say why there are no printers.

use std::os::windows::process::CommandExt;
use std::process::Command;
use std::time::Duration;

use anyhow::Context;
use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::{GetLastError, ERROR_FILE_NOT_FOUND, ERROR_INSUFFICIENT_BUFFER};
use windows::Win32::Graphics::Gdi::{DEVMODEW, DM_DEFAULTSOURCE, DM_PAPERSIZE};
use windows::Win32::Graphics::Printing::{
    EnumPrintersW, GetDefaultPrinterW, SetDefaultPrinterW, PRINTER_ATTRIBUTE_SHARED,
    PRINTER_ATTRIBUTE_WORK_OFFLINE, PRINTER_ENUM_CONNECTIONS, PRINTER_ENUM_LOCAL, PRINTER_INFO_2W,
    PRINTER_STATUS_BUSY, PRINTER_STATUS_DOOR_OPEN, PRINTER_STATUS_ERROR,
    PRINTER_STATUS_NOT_AVAILABLE, PRINTER_STATUS_NO_TONER, PRINTER_STATUS_OFFLINE,
    PRINTER_STATUS_PAPER_JAM, PRINTER_STATUS_PAPER_OUT, PRINTER_STATUS_PAPER_PROBLEM,
    PRINTER_STATUS_PAUSED, PRINTER_STATUS_PRINTING, PRINTER_STATUS_PROCESSING,
    PRINTER_STATUS_SERVER_UNKNOWN, PRINTER_STATUS_USER_INTERVENTION,
};
// DeviceCapabilities and the DC_* constants live under Storage::Xps in windows 0.58.
use windows::Win32::Storage::Xps::{
    DeviceCapabilitiesW, DC_BINNAMES, DC_BINS, DC_COLORDEVICE, DC_DUPLEX, DC_ENUMRESOLUTIONS,
    DC_PAPERNAMES, DC_PAPERS, PRINTER_DEVICE_CAPABILITIES,
};

use crate::engine::print::windows::{id_for_name, paper_size_for_dmpaper, with_canonical_papers};
use crate::ipc::printers::{DiscoveredPrinter, Printer, PrinterStatus, Supplies, Tray};
use crate::platform::{dnssd, infer_kind};
use crate::queue::job::PaperSize;
use crate::util::error::{IpcErrorCode, LocalizedError};

/// `RPC_S_SERVER_UNAVAILABLE`: what every spooler call returns while the Print
/// Spooler service is stopped.
const RPC_S_SERVER_UNAVAILABLE: u32 = 1722;

/// Shown when the spooler is not running. The single most common Windows
/// support case for "MaeclyPrint shows no printers".
const SPOOLER_DOWN: &str = "The Windows Print Spooler service is not running. Start it \
     (Services → Print Spooler) and try again.";

/// Read a (possibly null) wide string pointer into an owned `String`.
fn pwstr(p: PWSTR) -> String {
    if p.is_null() {
        String::new()
    } else {
        unsafe { p.to_string().unwrap_or_default() }
    }
}

/// Turn a spooler failure into an error the UI can show. A stopped spooler
/// gets a sentence of its own; anything else keeps the OS message.
fn spooler_error(e: windows::core::Error, what: &str) -> anyhow::Error {
    if e.code() == windows::core::HRESULT::from_win32(RPC_S_SERVER_UNAVAILABLE) {
        anyhow::anyhow!(SPOOLER_DOWN)
    } else {
        anyhow::anyhow!("{what} failed: {e}")
    }
}

/// `EnumPrinters` level 2 into a buffer that is correctly aligned for
/// `PRINTER_INFO_2W` (a `Vec<u8>` is only guaranteed byte alignment, and the
/// records are read in place through a pointer cast).
///
/// The sizing call is expected to fail with `ERROR_INSUFFICIENT_BUFFER`; any
/// *other* failure is real and returned — previously it was discarded and a
/// stopped spooler looked exactly like a machine with no printers. A printer
/// added between the two calls makes the second fail with the same
/// insufficient-buffer code, so the pair is retried a few times.
fn enum_printers_level2() -> anyhow::Result<(Vec<u64>, u32)> {
    let flags = PRINTER_ENUM_LOCAL | PRINTER_ENUM_CONNECTIONS;
    let insufficient = ERROR_INSUFFICIENT_BUFFER.to_hresult();
    for _ in 0..4 {
        let mut needed = 0u32;
        let mut returned = 0u32;
        match unsafe { EnumPrintersW(flags, PCWSTR::null(), 2, None, &mut needed, &mut returned) } {
            // Success with no buffer means there is nothing to enumerate.
            Ok(()) => return Ok((Vec::new(), 0)),
            Err(e) if e.code() == insufficient => {}
            Err(e) => return Err(spooler_error(e, "EnumPrinters")),
        }
        if needed == 0 {
            return Ok((Vec::new(), 0));
        }
        let words = (needed as usize).div_ceil(std::mem::size_of::<u64>());
        let mut buf = vec![0u64; words];
        let bytes = unsafe {
            std::slice::from_raw_parts_mut(buf.as_mut_ptr().cast::<u8>(), needed as usize)
        };
        match unsafe {
            EnumPrintersW(
                flags,
                PCWSTR::null(),
                2,
                Some(bytes),
                &mut needed,
                &mut returned,
            )
        } {
            Ok(()) => return Ok((buf, returned)),
            Err(e) if e.code() == insufficient => continue,
            Err(e) => return Err(spooler_error(e, "EnumPrinters")),
        }
    }
    anyhow::bail!("the printer list kept changing while it was being read; try again")
}

/// Visit every `PRINTER_INFO_2W` record of an enumeration.
fn for_each_printer(mut f: impl FnMut(&PRINTER_INFO_2W)) -> anyhow::Result<()> {
    let (buf, returned) = enum_printers_level2()?;
    let infos = buf.as_ptr().cast::<PRINTER_INFO_2W>();
    for i in 0..returned as usize {
        // SAFETY: the spooler wrote `returned` contiguous records at the start
        // of `buf`, which is 8-byte aligned and outlives the borrow.
        let info = unsafe { &*infos.add(i) };
        f(info);
    }
    Ok(())
}

pub fn list_printers() -> anyhow::Result<Vec<Printer>> {
    let default = default_printer().unwrap_or(None);
    let mut out = Vec::new();
    for_each_printer(|info| out.push(printer_from(info, default.as_deref())))?;
    Ok(out)
}

/// Installed queue names and ports, cheaply (no capability queries). Used to
/// de-duplicate a new queue name and to hide installed devices from discovery.
fn installed_names_and_ports() -> anyhow::Result<Vec<(String, String)>> {
    let mut out = Vec::new();
    for_each_printer(|info| out.push((pwstr(info.pPrinterName), pwstr(info.pPortName))))?;
    Ok(out)
}

pub fn default_printer() -> anyhow::Result<Option<String>> {
    let mut len = 0u32;
    // First call sizes the buffer; ERROR_FILE_NOT_FOUND means "no default".
    let ok = unsafe { GetDefaultPrinterW(PWSTR::null(), &mut len) };
    if !ok.as_bool() {
        let err = unsafe { GetLastError() };
        if err == ERROR_FILE_NOT_FOUND {
            return Ok(None);
        }
        if err.0 == RPC_S_SERVER_UNAVAILABLE {
            anyhow::bail!(SPOOLER_DOWN);
        }
        if err != ERROR_INSUFFICIENT_BUFFER {
            anyhow::bail!("GetDefaultPrinter failed: {}", err.to_hresult().message());
        }
    }
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

// ----------------------------------------------------------------------------
// Network discovery (mDNS / DNS-SD)
// ----------------------------------------------------------------------------

/// How long the browse listens. Matches the CUPS backend's `ippfind -T 4`, so
/// the "Scan network" spinner feels the same on both platforms.
const DISCOVERY_WINDOW: Duration = Duration::from_secs(4);

/// Browse `_ipp._tcp` and `_ipps._tcp` for [`DISCOVERY_WINDOW`] and return the
/// printers that are not installed yet.
///
/// Windows has no `ippfind`, so this asks the OS DNS-SD client
/// (`DnsServiceBrowse`, see [`super::windows_dnssd`]). The query runs inside
/// the DNS Client service, whose firewall rule admits the multicast answers;
/// an mDNS socket of our own is blocked by Windows Defender Firewall for a
/// per-user app and used to hear nothing. WSD-only devices are not found by
/// it; they are the ones Windows' own "Add device" dialog lists, which the UI
/// already offers via "Open printer settings". A browse that cannot be started
/// is an error (the UI shows a scan error), not an empty network.
pub fn discover_printers() -> anyhow::Result<Vec<DiscoveredPrinter>> {
    let records =
        super::windows_dnssd::browse(&[dnssd::IPP_SERVICE, dnssd::IPPS_SERVICE], DISCOVERY_WINDOW)?;

    // Hide what is already installed. If the spooler cannot be read the scan
    // still returns what it found rather than failing outright.
    let installed = installed_names_and_ports().unwrap_or_default();
    Ok(dnssd::to_discovered(&records, &installed))
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// `CreateProcess` flag that suppresses the transient console window a
/// console-subsystem child (`cmd.exe`, `powershell.exe`) would otherwise flash
/// on screen: the release binary has no console of its own to lend it.
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Run a CLI and succeed only on a zero exit status.
fn run_ok(program: &str, args: &[&str]) -> anyhow::Result<()> {
    // stdout/stderr never reach our own handles: a CLI run's `--json` output
    // must stay parseable.
    let status = Command::new(program)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .status()?;
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
        Err(LocalizedError::new(
            IpcErrorCode::PrintFailed,
            "set_default_failed",
            "SetDefaultPrinter failed",
        )
        .into())
    }
}

// ----------------------------------------------------------------------------
// Test page
// ----------------------------------------------------------------------------

/// Print the branded MaeclyPrint test page, applying the user's configured
/// default paper and tray for this printer — the same contract the CUPS path
/// honours: an override that is set is sent, and one that is not set sends
/// nothing, so the driver's own default applies.
///
/// The PDF goes through the ordinary print backend
/// ([`crate::engine::print::sumatra`]), which is the only Windows path whose
/// **failure is observable**: `rundll32 printui.dll,PrintUIEntry` exits 0 even
/// when `PrintUIEntry` itself fails (it shows its own dialog), so an `Ok(())`
/// from it never meant the page was accepted. Spooling a real job gives a real
/// exit status. `rundll32` survives only as the fallback for hosts without the
/// SumatraPDF sidecar.
///
/// The paper override is a *driver* paper name (from `DC_PAPERNAMES`), which is
/// localized — Spanish Windows says "Carta" for Letter — so it is mapped
/// through the driver's paper **id** (`DC_PAPERS`, a `DMPAPER_*` value) rather
/// than by matching the string. A paper SumatraPDF cannot name is left out and
/// logged instead of being silently replaced by A4. The tray goes out as
/// SumatraPDF's `bin=<name>`.
pub fn print_test_page(name: &str) -> anyhow::Result<()> {
    use crate::engine::print::sumatra::{self, PaperToken};
    use std::io::Write as _;

    let prefs = crate::settings::Config::load_or_default().printers;
    let over = prefs.overrides.get(name).cloned().unwrap_or_default();

    let pdf = super::testpage::render(&super::testpage::TestPageInfo {
        printer: name.to_string(),
        paper: over.paper.clone(),
        tray: over.tray.clone(),
    });

    let mut tmp = tempfile::Builder::new()
        .prefix("maeclyprint-testpage-")
        .suffix(".pdf")
        .tempfile()
        .with_context(|| "failed to create temp PDF")?;
    tmp.write_all(&pdf)?;
    tmp.flush()?;

    let paper = over
        .paper
        .as_deref()
        .map(str::trim)
        .filter(|p| !p.is_empty());
    let paper_size = paper.and_then(|p| paper_size_for_driver_name(name, p));
    if let (Some(p), None) = (paper, &paper_size) {
        tracing::warn!(
            event = "print_setting_not_applied",
            setting = "paper",
            backend = "sumatra",
            printer = %name,
            paper = %p,
            message = "SumatraPDF cannot name this paper size; the driver default is used",
        );
    }

    let paper_token = if paper_size.is_some() {
        PaperToken::FromSettings
    } else {
        PaperToken::Omit
    };
    let defaults = crate::queue::job::JobSettings::default();
    let settings = crate::queue::job::JobSettings {
        // Only read when `paper_token` says so, i.e. when `paper_size` is Some.
        paper_size: paper_size.unwrap_or(defaults.paper_size),
        tray: over.tray.clone(),
        ..defaults
    };
    // SumatraPDF blocks until the spooler accepts the job, so the temp file is
    // still alive for the whole call and the status is meaningful.
    match sumatra::spool_with(
        tmp.path(),
        name,
        &settings,
        "MaeclyPrint Test Page",
        Some(Duration::from_secs(120)),
        paper_token,
    ) {
        Ok(_) => Ok(()),
        Err(crate::engine::print::PrintError::Engine(
            crate::engine::EngineError::DependencyMissing(_),
        )) => run_ok("rundll32", &["printui.dll,PrintUIEntry", "/k", "/n", name]),
        Err(e) => Err(e.into()),
    }
}

/// Map a driver paper name of `printer` to a [`PaperSize`] through its
/// `DMPAPER_*` id, or `None` when the driver does not list it or SumatraPDF
/// has no name for it. Driver names are localized ("Carta" on Spanish Windows),
/// so the id — not the string — is what identifies the sheet.
pub fn paper_size_for_driver_name(printer: &str, paper: &str) -> Option<PaperSize> {
    let (names, ids) = driver_papers(printer);
    let id = id_for_name(&names, &ids, paper)?;
    paper_size_for_dmpaper(id)
}

/// The driver's parallel paper name and `DMPAPER_*` id arrays for `printer`.
fn driver_papers(printer: &str) -> (Vec<String>, Vec<u16>) {
    let device = wide(printer);
    let device = PCWSTR(device.as_ptr());
    (
        dc_names(device, PCWSTR::null(), DC_PAPERNAMES, 64),
        dc_ids(device, PCWSTR::null(), DC_PAPERS),
    )
}

/// The paper list the capability gate checks a job against: the names the
/// driver shows (localized) **plus** the canonical name of every sheet it
/// lists by id, so a profile saying `Letter` passes on a Spanish driver that
/// calls it "Carta" (see [`with_canonical_papers`]). Falls back to `listed`
/// (what enumeration reported) when the driver cannot be asked.
pub fn caps_papers(printer: &str, listed: &[String]) -> Vec<String> {
    let (names, ids) = driver_papers(printer);
    if names.is_empty() || ids.is_empty() {
        return listed.to_vec();
    }
    with_canonical_papers(&names, &ids)
}

/// The name paired with `id` in two parallel `DeviceCapabilities` arrays.
fn name_for_id(names: &[String], ids: &[u16], id: u16) -> Option<String> {
    let pos = ids.iter().position(|&i| i == id)?;
    names.get(pos).filter(|n| !n.is_empty()).cloned()
}

/// Open Settings → Printers & scanners.
pub fn open_printer_settings() -> anyhow::Result<()> {
    run_ok("cmd", &["/C", "start", "", "ms-settings:printers"])
}

// ----------------------------------------------------------------------------
// Adding a printer (`Add-Printer -IppURL` via PowerShell)
// ----------------------------------------------------------------------------

/// Exit code the add-printer script uses for an access-denied failure. Matched
/// as a number, so it works on every Windows display language (the error text
/// itself is localized).
const PS_EXIT_ACCESS_DENIED: i32 = 5;
/// Exit code for "this Windows has no `Add-Printer -IppURL`".
const PS_EXIT_NO_IPPURL: i32 = 3;

/// Install a printer for `uri` using the built-in Microsoft IPP Class Driver.
///
/// Uses `Add-Printer -IppURL`, which creates a real IPP connection bound to the
/// inbox class driver. (An earlier version pre-created a port with
/// `Add-PrinterPort -Name 'IPP://…'`; with only `-Name` that cmdlet creates a
/// *Local Port* whose name happens to be a URL, so jobs never reached the
/// device, and its `catch {}` hid every failure.)
///
/// Only `ipp`/`ipps` URIs are supported — raw sockets/LPD/WSD need the system
/// dialog, so those are rejected with a message pointing at "Open printer
/// settings". The queue name is de-duplicated against the installed queues
/// first (`Name`, `Name_2`, …), as on CUPS, because `Add-Printer` refuses an
/// existing name.
pub fn add_printer(name: &str, uri: &str) -> anyhow::Result<()> {
    let scheme = uri
        .split("://")
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    if scheme != "ipp" && scheme != "ipps" {
        return Err(LocalizedError::new(
            IpcErrorCode::InvalidRequest,
            "add_printer_unsupported_scheme",
            format!(
                "On Windows, printers using {scheme}:// must be added via system settings \
                 (Open printer settings)"
            ),
        )
        .param("scheme", &scheme)
        .into());
    }

    let url = ipp_class_driver_url(uri)?;
    let base = queue_base_name(name, &url);
    let existing: Vec<String> = installed_names_and_ports()?
        .into_iter()
        .map(|(n, _)| n)
        .collect();
    let queue = unique_queue_name(&base, &existing);

    // The name and URL come from the network (a DNS-SD instance name, a TXT
    // `rp` path) or from the user. They never become PowerShell source: the
    // script is a constant and reads them from its environment.
    match run_powershell(
        ADD_PRINTER_SCRIPT,
        &[(ENV_PRINTER_NAME, &queue), (ENV_PRINTER_URL, &url)],
    )? {
        PsOutcome::Ok => Ok(()),
        PsOutcome::Failed { code, message } => match code {
            // Elevation problems can't be retried away, so the guidance is
            // localized (FR-092) rather than left as the spooler's text.
            PS_EXIT_ACCESS_DENIED => Err(LocalizedError::new(
                IpcErrorCode::PrintFailed,
                "add_printer_needs_admin",
                "Adding a printer may require administrator rights. Use \"Open printer \
                 settings\" to add it from Windows.",
            )
            .into()),
            PS_EXIT_NO_IPPURL => anyhow::bail!(
                "This version of Windows cannot add an IPP printer from MaeclyPrint \
                 (Add-Printer has no -IppURL). Use \"Open printer settings\" to add it from \
                 Windows."
            ),
            _ if message.is_empty() => anyhow::bail!("Add-Printer failed (exit code {code})"),
            _ => anyhow::bail!("{message}"),
        },
    }
}

/// Result of a PowerShell run that did start.
enum PsOutcome {
    Ok,
    Failed { code: i32, message: String },
}

/// Run a constant Windows PowerShell 5.1 script with `env` set in its
/// environment, returning the exit code and the script's own error line on
/// failure.
///
/// Values travel **only** as environment variables: PowerShell reads
/// `$env:NAME` as data, so no quote, Unicode quote (`'` `‘` `’` `‚` `‛`),
/// `;` or `$(…)` in them can end a literal and run code. The script itself
/// must never be built from a value.
///
/// The script writes its error text through `[Console]::Error` after switching
/// the console encoding to UTF-8 (see [`ADD_PRINTER_SCRIPT`]), so it is decoded
/// as UTF-8 here. Without that switch `powershell.exe` writes in the OEM code
/// page (CP850 on Spanish Windows) and "No se encontró" arrived as "encontr�".
fn run_powershell(script: &'static str, env: &[(&str, &str)]) -> anyhow::Result<PsOutcome> {
    let mut command = Command::new("powershell.exe");
    command.args([
        "-NoProfile",
        "-NonInteractive",
        "-ExecutionPolicy",
        "Bypass",
        "-Command",
        script,
    ]);
    for (key, value) in env {
        command.env(key, value);
    }
    let out = command
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .with_context(|| "failed to spawn powershell.exe")?;
    if out.status.success() {
        return Ok(PsOutcome::Ok);
    }
    let message = String::from_utf8_lossy(&out.stderr).trim().to_string();
    Ok(PsOutcome::Failed {
        code: out.status.code().unwrap_or(1),
        message,
    })
}

/// Map an `ipp://` / `ipps://` device URI to the `http(s)://` URL the IPP
/// Class Driver's `-IppURL` takes. IPP runs over HTTP, and Windows' IPP
/// connections are addressed by their HTTP URL; the port is kept, and filled
/// in as 631 (the IPP and IPPS default, RFC 8010 / RFC 7472) when absent.
fn ipp_class_driver_url(uri: &str) -> anyhow::Result<String> {
    let (scheme, rest) = uri
        .split_once("://")
        .with_context(|| format!("not a URI: {uri}"))?;
    let http = match scheme.to_ascii_lowercase().as_str() {
        "ipp" => "http",
        "ipps" => "https",
        other => anyhow::bail!("unsupported scheme for an IPP printer: {other}"),
    };
    let (authority, path) = rest.split_once('/').unwrap_or((rest, ""));
    if authority.is_empty() {
        anyhow::bail!("URI has no host: {uri}");
    }
    // Bracketed IPv6 authorities ("[::1]:631") must not be split at their
    // first ':', which is inside the brackets.
    let (host, port) = if let Some(v6) = authority.strip_prefix('[') {
        match v6.split_once(']') {
            Some((h, tail)) => (format!("[{h}]"), tail.strip_prefix(':').unwrap_or("")),
            None => anyhow::bail!("malformed IPv6 authority: {authority}"),
        }
    } else {
        match authority.split_once(':') {
            Some((h, p)) => (h.to_string(), p),
            None => (authority.to_string(), ""),
        }
    };
    if host.is_empty() || host == "[]" {
        anyhow::bail!("URI has no host: {uri}");
    }
    if !valid_host(&host) {
        anyhow::bail!("URI has an invalid host: {uri}");
    }
    let port = if port.is_empty() { "631" } else { port };
    if !port.chars().all(|c| c.is_ascii_digit()) {
        anyhow::bail!("URI has an invalid port: {uri}");
    }
    let path = if path.is_empty() { "ipp/print" } else { path };
    Ok(format!("{http}://{host}:{port}/{}", encode_path(path)))
}

/// A DNS name or IPv4 address (letters, digits, `-`, `.`, `_`), or a bracketed
/// IPv6 address (hex digits, `:`, `.` and a `%zone`). Anything else — a quote,
/// a space, a non-ASCII character — cannot be a host and is refused.
fn valid_host(host: &str) -> bool {
    match host.strip_prefix('[').and_then(|h| h.strip_suffix(']')) {
        Some(v6) => {
            let (addr, zone) = v6.split_once('%').unwrap_or((v6, ""));
            !addr.is_empty()
                && addr
                    .chars()
                    .all(|c| c.is_ascii_hexdigit() || c == ':' || c == '.')
                && zone
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        }
        None => host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_')),
    }
}

/// Percent-encode every byte of `path` outside RFC 3986's path characters
/// (unreserved, sub-delims except `'`, `:`, `@`, `/`, and an existing `%`).
/// A TXT `rp` is whatever the device advertises; this keeps quotes, spaces
/// and non-ASCII text from reaching the spooler as anything but a URL.
fn encode_path(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for b in path.bytes() {
        let keep = b.is_ascii_alphanumeric()
            || matches!(
                b,
                b'-' | b'.'
                    | b'_'
                    | b'~'
                    | b'/'
                    | b'!'
                    | b'$'
                    | b'&'
                    | b'('
                    | b')'
                    | b'*'
                    | b'+'
                    | b','
                    | b';'
                    | b'='
                    | b':'
                    | b'@'
                    | b'%'
            );
        if keep {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// The queue name to start from: the user's display name, else the host.
/// Characters the spooler refuses in a printer name (`\` and `,`) and control
/// characters become `_`; the spooler caps names at 220 characters.
fn queue_base_name(name: &str, url: &str) -> String {
    let trimmed = name.trim();
    let raw = if trimmed.is_empty() {
        url.split_once("://")
            .map(|(_, rest)| rest.split(['/', ':']).next().unwrap_or(rest))
            .unwrap_or(url)
    } else {
        trimmed
    };
    let cleaned: String = raw
        .chars()
        .map(|c| {
            if matches!(c, '\\' | ',') || c.is_control() {
                '_'
            } else {
                c
            }
        })
        .take(220)
        .collect();
    if cleaned.is_empty() {
        "Printer".to_string()
    } else {
        cleaned
    }
}

/// `base` if no installed queue uses it (Windows printer names compare
/// case-insensitively), else the first free `base_2`, `base_3`, ….
fn unique_queue_name(base: &str, existing: &[String]) -> String {
    let taken = |candidate: &str| existing.iter().any(|e| e.eq_ignore_ascii_case(candidate));
    if !taken(base) {
        return base.to_string();
    }
    let mut n = 2u32;
    loop {
        let candidate = format!("{base}_{n}");
        if !taken(&candidate) {
            return candidate;
        }
        n += 1;
    }
}

/// Environment variable carrying the queue name to [`ADD_PRINTER_SCRIPT`].
const ENV_PRINTER_NAME: &str = "MAECLYPRINT_PRINTER_NAME";
/// Environment variable carrying the IPP URL to [`ADD_PRINTER_SCRIPT`].
const ENV_PRINTER_URL: &str = "MAECLYPRINT_PRINTER_URL";

/// The PowerShell script adding a queue for an IPP printer. A constant: the
/// queue name and URL are read from [`ENV_PRINTER_NAME`] and
/// [`ENV_PRINTER_URL`], never spliced into the source (see [`run_powershell`]).
///
/// - Output encoding is switched to UTF-8 first, so localized error text
///   survives the trip.
/// - Feature detection: `-IppURL` is not on every Windows build; its absence
///   exits with [`PS_EXIT_NO_IPPURL`] (3).
/// - Access denied is recognised by HRESULT `0x80070005` or the
///   `PermissionDenied` category — never by English words — and exits with
///   [`PS_EXIT_ACCESS_DENIED`] (5).
/// - Any other failure writes just the exception message to stderr and exits 1.
const ADD_PRINTER_SCRIPT: &str = "$ErrorActionPreference = 'Stop'; \
     [Console]::OutputEncoding = New-Object System.Text.UTF8Encoding $false; \
     try { \
       if (-not (Get-Command Add-Printer).Parameters.ContainsKey('IppURL')) { exit 3 }; \
       Add-Printer -Name $env:MAECLYPRINT_PRINTER_NAME -IppURL $env:MAECLYPRINT_PRINTER_URL; \
       exit 0 \
     } catch { \
       $id = \"$($_.FullyQualifiedErrorId) $($_.Exception.HResult) $($_.Exception.MessageId)\"; \
       if ($_.CategoryInfo.Category -eq 'PermissionDenied' -or $id -match '80070005|-2147024891') { exit 5 }; \
       [Console]::Error.WriteLine($_.Exception.Message); exit 1 \
     }";

// ----------------------------------------------------------------------------
// Per-printer assembly
// ----------------------------------------------------------------------------

fn printer_from(info: &PRINTER_INFO_2W, default: Option<&str>) -> Printer {
    let name = pwstr(info.pPrinterName);
    let port = pwstr(info.pPortName);
    let driver = pwstr(info.pDriverName);
    let device = PCWSTR(info.pPrinterName.0);
    let port_w = PCWSTR(info.pPortName.0);

    let shared = (info.Attributes & PRINTER_ATTRIBUTE_SHARED) != 0;

    let paper_names = dc_names(device, port_w, DC_PAPERNAMES, 64);
    let bin_names = dc_names(device, port_w, DC_BINNAMES, 24);
    let (default_paper, default_tray) = devmode_defaults(
        // SAFETY: pDevMode is either null or points into the enumeration
        // buffer, which outlives this call.
        unsafe { info.pDevMode.as_ref() },
        &paper_names,
        &dc_ids(device, port_w, DC_PAPERS),
        &bin_names,
        &dc_ids(device, port_w, DC_BINS),
    );

    Printer {
        id: name.clone(),
        name: if name.is_empty() {
            port.clone()
        } else {
            name.clone()
        },
        kind: infer_kind(&driver, &name, Some(&port)),
        color: dc_flag(device, port_w, DC_COLORDEVICE) == 1,
        duplex: dc_flag(device, port_w, DC_DUPLEX) == 1,
        status: status_from(info.Status, info.Attributes, info.cJobs),
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
        papers: paper_names.into_iter().filter(|n| !n.is_empty()).collect(),
        trays: bin_names
            .into_iter()
            .filter(|n| !n.is_empty())
            .map(|name| Tray {
                name,
                paper: None,
                level: None,
            })
            .collect(),
        // Win32 does not expose supply levels.
        supplies: Supplies::default(),
        default_tray,
        default_paper,
    }
}

/// The spooler's default paper and tray *names*, read from the printer's
/// default DEVMODE and named through the driver's parallel id/name arrays.
/// A field the DEVMODE does not set (`dmFields`) or an id the driver does not
/// list yields `None`, which the UI shows as a plain "Printer default".
fn devmode_defaults(
    dm: Option<&DEVMODEW>,
    paper_names: &[String],
    paper_ids: &[u16],
    bin_names: &[String],
    bin_ids: &[u16],
) -> (Option<String>, Option<String>) {
    let Some(dm) = dm else {
        return (None, None);
    };
    // SAFETY: the printer arm of the DEVMODE union is the one a printer's
    // DEVMODE uses; reading plain integers from it cannot be invalid.
    let printer_fields = unsafe { dm.Anonymous1.Anonymous1 };
    let paper = (dm.dmFields.0 & DM_PAPERSIZE.0 != 0)
        .then(|| name_for_id(paper_names, paper_ids, printer_fields.dmPaperSize as u16))
        .flatten();
    let tray = (dm.dmFields.0 & DM_DEFAULTSOURCE.0 != 0)
        .then(|| name_for_id(bin_names, bin_ids, printer_fields.dmDefaultSource as u16))
        .flatten();
    (paper, tray)
}

/// Normalize the spooler's `Status` bits *and* `Attributes` into one state.
///
/// - **Offline**: the device is offline, unreachable or unknown to its server,
///   the user chose *Use Printer Offline* (`PRINTER_ATTRIBUTE_WORK_OFFLINE` —
///   an attribute, with `Status` often 0), or the queue is **paused**. A paused
///   queue accepts jobs but prints none, which to the user is the same promise
///   broken as offline, so it must not read "Ready".
/// - **Error**: anything that needs a person (jam, out of paper or toner, door
///   open, "user intervention").
/// - **Busy**: printing/processing, or jobs waiting.
fn status_from(status: u32, attributes: u32, jobs: u32) -> PrinterStatus {
    let offline = PRINTER_STATUS_OFFLINE
        | PRINTER_STATUS_NOT_AVAILABLE
        | PRINTER_STATUS_SERVER_UNKNOWN
        | PRINTER_STATUS_PAUSED;
    let err = PRINTER_STATUS_ERROR
        | PRINTER_STATUS_PAPER_JAM
        | PRINTER_STATUS_PAPER_OUT
        | PRINTER_STATUS_PAPER_PROBLEM
        | PRINTER_STATUS_NO_TONER
        | PRINTER_STATUS_DOOR_OPEN
        | PRINTER_STATUS_USER_INTERVENTION;
    let busy = PRINTER_STATUS_PRINTING | PRINTER_STATUS_PROCESSING | PRINTER_STATUS_BUSY;
    if status & offline != 0 || attributes & PRINTER_ATTRIBUTE_WORK_OFFLINE != 0 {
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
///
/// Empty names are kept (as empty strings) so the array stays parallel to the
/// matching id array (`DC_PAPERS`/`DC_BINS`); callers that show the names
/// filter blanks themselves. Names are trimmed: the Microsoft IPP class driver
/// pads some (`" 8 5x13 4in"`), and the padding then showed in the UI and in a
/// saved paper override.
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
            String::from_utf16_lossy(&chunk[..end]).trim().to_string()
        })
        .collect()
}

/// A `WORD`-array `DeviceCapabilities` query (`DC_PAPERS`, `DC_BINS`).
fn dc_ids(device: PCWSTR, port: PCWSTR, cap: PRINTER_DEVICE_CAPABILITIES) -> Vec<u16> {
    let count = unsafe { DeviceCapabilitiesW(device, port, cap, PWSTR::null(), None) };
    if count <= 0 {
        return Vec::new();
    }
    let mut buf = vec![0u16; count as usize];
    let got = unsafe { DeviceCapabilitiesW(device, port, cap, PWSTR(buf.as_mut_ptr()), None) };
    if got <= 0 {
        return Vec::new();
    }
    buf.truncate(got as usize);
    buf
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_priority() {
        assert!(matches!(
            status_from(PRINTER_STATUS_OFFLINE, 0, 0),
            PrinterStatus::Offline
        ));
        assert!(matches!(
            status_from(PRINTER_STATUS_PAPER_JAM, 0, 0),
            PrinterStatus::Error
        ));
        assert!(matches!(
            status_from(PRINTER_STATUS_PRINTING, 0, 0),
            PrinterStatus::Busy
        ));
        assert!(matches!(status_from(0, 0, 3), PrinterStatus::Busy));
        assert!(matches!(status_from(0, 0, 0), PrinterStatus::Ready));
    }

    /// A paused queue, "Use Printer Offline" (an *attribute*, with Status 0),
    /// an unreachable device and a server that no longer knows it are not
    /// Ready; "user intervention" is an error.
    #[test]
    fn fr_printers_status_honours_paused_offline_attribute_and_intervention() {
        let offline = |s, a| matches!(status_from(s, a, 0), PrinterStatus::Offline);
        assert!(offline(PRINTER_STATUS_PAUSED, 0));
        assert!(offline(0, PRINTER_ATTRIBUTE_WORK_OFFLINE));
        assert!(offline(PRINTER_STATUS_NOT_AVAILABLE, 0));
        assert!(offline(PRINTER_STATUS_SERVER_UNKNOWN, 0));
        // Offline wins over jobs waiting: they are stuck, not printing.
        assert!(matches!(
            status_from(0, PRINTER_ATTRIBUTE_WORK_OFFLINE, 4),
            PrinterStatus::Offline
        ));
        assert!(matches!(
            status_from(PRINTER_STATUS_USER_INTERVENTION, 0, 0),
            PrinterStatus::Error
        ));
        // A shared printer is still Ready.
        assert!(matches!(
            status_from(0, PRINTER_ATTRIBUTE_SHARED, 0),
            PrinterStatus::Ready
        ));
    }

    #[test]
    fn ipp_class_driver_url_maps_schemes_and_ports() {
        assert_eq!(
            ipp_class_driver_url("ipp://10.0.0.41:631/ipp/print").unwrap(),
            "http://10.0.0.41:631/ipp/print"
        );
        assert_eq!(
            ipp_class_driver_url("ipps://printer.local/ipp/print").unwrap(),
            "https://printer.local:631/ipp/print"
        );
        assert_eq!(
            ipp_class_driver_url("ipp://host:9631/printers/lab").unwrap(),
            "http://host:9631/printers/lab"
        );
        // No path → the IPP Everywhere default.
        assert_eq!(
            ipp_class_driver_url("ipp://host").unwrap(),
            "http://host:631/ipp/print"
        );
    }

    #[test]
    fn ipp_class_driver_url_handles_ipv6_and_rejects_invalid() {
        assert_eq!(
            ipp_class_driver_url("ipp://[fe80::1]:631/ipp/print").unwrap(),
            "http://[fe80::1]:631/ipp/print"
        );
        assert_eq!(
            ipp_class_driver_url("ipp://[fe80::1]/ipp/print").unwrap(),
            "http://[fe80::1]:631/ipp/print"
        );
        assert!(ipp_class_driver_url("ipp://[fe80::1/ipp/print").is_err());
        assert!(ipp_class_driver_url("not-a-uri").is_err());
        assert!(ipp_class_driver_url("socket://host/queue").is_err());
        assert!(ipp_class_driver_url("ipp:///ipp/print").is_err());
        assert!(ipp_class_driver_url("ipp://host:abc/ipp/print").is_err());
    }

    /// The script binds the class driver through `-IppURL` — never a Local
    /// Port named after the URL — reports access-denied by exit code, and has
    /// no catch-all that swallows errors.
    #[test]
    fn fr_printers_add_printer_script_uses_ippurl() {
        let s = ADD_PRINTER_SCRIPT;
        assert!(s.starts_with("$ErrorActionPreference = 'Stop';"));
        assert!(s.contains("[Console]::OutputEncoding"));
        assert!(s.contains(&format!(
            "Add-Printer -Name $env:{ENV_PRINTER_NAME} -IppURL $env:{ENV_PRINTER_URL}"
        )));
        assert!(!s.contains("Add-PrinterPort"));
        assert!(!s.contains("catch { }"));
        assert!(s.contains(&format!("exit {PS_EXIT_ACCESS_DENIED}")));
        assert!(s.contains(&format!("exit {PS_EXIT_NO_IPPURL}")));
        assert!(s.contains("80070005"));
    }

    /// A rogue mDNS advert controls the instance name and the TXT `rp`. With
    /// Unicode single quotes (which PowerShell also treats as delimiters) they
    /// used to close the literal and run code. Values now never reach the
    /// script source, the host is validated and the path percent-encoded.
    #[test]
    fn fr_printers_hostile_names_and_paths_cannot_become_powershell() {
        let url = ipp_class_driver_url(
            "ipp://lab.local:631/ipp/print\u{2019};Start-Process calc;\u{2019}",
        )
        .unwrap();
        assert_eq!(
            url,
            "http://lab.local:631/ipp/print%E2%80%99;Start-Process%20calc;%E2%80%99"
        );
        assert!(ipp_class_driver_url("ipp://evil'host/ipp/print").is_err());
        assert!(ipp_class_driver_url("ipp://ev\u{2019}il/ipp/print").is_err());
        assert!(ipp_class_driver_url("ipp://a b/ipp/print").is_err());
        assert_eq!(
            ipp_class_driver_url("ipp://[fe80::1%25en0]:631/ipp/print").unwrap(),
            "http://[fe80::1%25en0]:631/ipp/print"
        );
        // Control characters cannot reach the queue name (an env var value).
        assert_eq!(queue_base_name("Lab\u{0}\n1", "http://h:631/p"), "Lab__1");
    }

    /// The real thing, end to end: a name full of PowerShell metacharacters
    /// travels to a constant script as data and comes back unchanged.
    #[test]
    fn fr_printers_values_reach_powershell_as_data() {
        const ECHO: &str =
            "[Console]::OutputEncoding = New-Object System.Text.UTF8Encoding $false; \
             [Console]::Error.WriteLine($env:MAECLYPRINT_PRINTER_NAME); exit 7";
        let marker = std::env::temp_dir().join("maeclyprint-pwned");
        let _ = std::fs::remove_file(&marker);
        let hostile = format!(
            "Lab\u{2019}; New-Item '{}' \u{2018}$(calc)' `\"x",
            marker.display()
        );
        match run_powershell(ECHO, &[(ENV_PRINTER_NAME, &hostile)]).unwrap() {
            PsOutcome::Failed { code, message } => {
                assert_eq!(code, 7);
                assert_eq!(message, hostile);
            }
            PsOutcome::Ok => panic!("the probe script exits 7"),
        }
        assert!(!marker.exists());
    }

    #[test]
    fn fr_printers_add_printer_deduplicates_queue_names() {
        let existing = vec![
            "10.0.0.5".to_string(),
            "office".to_string(),
            "Office_2".into(),
        ];
        assert_eq!(unique_queue_name("Lab", &existing), "Lab");
        assert_eq!(unique_queue_name("10.0.0.5", &existing), "10.0.0.5_2");
        // Windows printer names compare case-insensitively.
        assert_eq!(unique_queue_name("Office", &existing), "Office_3");
    }

    #[test]
    fn queue_base_name_falls_back_to_host_and_cleans_reserved_chars() {
        assert_eq!(
            queue_base_name("  Front Desk ", "http://h:631/p"),
            "Front Desk"
        );
        assert_eq!(
            queue_base_name("", "http://10.0.0.5:631/ipp/print"),
            "10.0.0.5"
        );
        assert_eq!(queue_base_name("a\\b,c", "http://h:631/p"), "a_b_c");
    }

    /// Localized driver names resolve through the id, not the string:
    /// Spanish Windows calls Letter "Carta".
    #[test]
    fn localized_paper_names_resolve_by_id() {
        let names = vec!["Carta".to_string(), "Oficio".into(), "A4".into()];
        let ids = vec![1u16, 5, 9];
        assert_eq!(
            id_for_name(&names, &ids, "Carta").and_then(paper_size_for_dmpaper),
            Some(PaperSize::LETTER)
        );
        assert_eq!(name_for_id(&names, &ids, 9).as_deref(), Some("A4"));
        assert_eq!(name_for_id(&names, &ids, 70), None);
    }

    /// The gate's list for a real driver (Microsoft Print to PDF, always on
    /// Windows) carries the canonical names next to the driver's own.
    #[test]
    fn fr_041_caps_papers_add_canonical_names_for_a_real_driver() {
        let papers = caps_papers("Microsoft Print to PDF", &[]);
        if papers.is_empty() {
            return; // no such queue on this host
        }
        for canonical in ["A4", "Letter"] {
            assert!(
                papers.iter().any(|p| p.eq_ignore_ascii_case(canonical)),
                "{canonical} missing from {papers:?}"
            );
        }
        assert_eq!(
            paper_size_for_driver_name("Microsoft Print to PDF", "A4"),
            Some(PaperSize::A4)
        );
    }

    #[test]
    fn devmode_defaults_name_the_spooler_default() {
        let mut dm = DEVMODEW {
            dmFields: DM_PAPERSIZE | DM_DEFAULTSOURCE,
            ..Default::default()
        };
        dm.Anonymous1.Anonymous1.dmPaperSize = 1;
        dm.Anonymous1.Anonymous1.dmDefaultSource = 15;
        let papers = vec!["Letter".to_string(), "A4".into()];
        let paper_ids = vec![1u16, 9];
        let bins = vec!["Auto".to_string(), "Tray 1".into()];
        let bin_ids = vec![7u16, 15];
        assert_eq!(
            devmode_defaults(Some(&dm), &papers, &paper_ids, &bins, &bin_ids),
            (Some("Letter".into()), Some("Tray 1".into()))
        );
        // Unset fields and unknown ids are "no hint", not a guess.
        dm.dmFields = DM_PAPERSIZE;
        dm.Anonymous1.Anonymous1.dmPaperSize = 70;
        assert_eq!(
            devmode_defaults(Some(&dm), &papers, &paper_ids, &bins, &bin_ids),
            (None, None)
        );
        assert_eq!(
            devmode_defaults(None, &papers, &paper_ids, &bins, &bin_ids),
            (None, None)
        );
    }

    #[test]
    fn kind_inference() {
        use crate::ipc::printers::PrinterKind;
        // The keyword table lives in `platform::mod` and is tested there; this
        // pins the Windows call shape (driver + queue name + port), which is
        // what makes a generic universal driver classify correctly.
        assert!(matches!(
            infer_kind(
                "HP Smart Universal Printing Driver",
                "HP OfficeJet Pro 9015",
                Some("IPP://10.0.0.9")
            ),
            PrinterKind::Mfp
        ));
        assert!(matches!(
            infer_kind("Microsoft Print to PDF", "Microsoft Print to PDF", None),
            PrinterKind::Virtual
        ));
    }

    /// Enumeration must not panic/error on a real Windows host (CI runner has
    /// Microsoft Print to PDF). Tolerates any count.
    ///
    /// Unlike the CUPS smoke test this one spawns no subprocess — it is a
    /// direct `EnumPrinters` call — so it stays in the default lane, where it
    /// is the only guard that the `unsafe` buffer walk is sound.
    #[test]
    fn list_printers_smoke() {
        let printers = list_printers().expect("list_printers should not error");
        for p in &printers {
            assert!(!p.id.is_empty());
            // A spooler default is always one of the printer's own names.
            if let Some(paper) = &p.default_paper {
                assert!(p.papers.contains(paper), "{paper} not in {:?}", p.papers);
            }
        }
        if let Some(def) = default_printer().unwrap() {
            assert!(printers.iter().any(|p| p.id == def));
        }
        let names = installed_names_and_ports().unwrap();
        assert_eq!(names.len(), printers.len());
    }

    /// The browse runs for its window and returns without error on a host with
    /// a network stack (it may find nothing).
    #[test]
    #[ignore = "waits for the 4 s browse window; run on the Windows VM"]
    fn discover_printers_smoke() {
        let found = discover_printers().expect("mDNS browse should start");
        for p in &found {
            assert!(p.id.starts_with("ipp://") || p.id.starts_with("ipps://"));
        }
    }
}
