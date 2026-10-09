//! CUPS printer enumeration for Unix (macOS, Linux).
//!
//! macOS localizes `lpstat`'s human-readable prose even under `LC_ALL=C`, so we
//! avoid parsing it and read locale-independent IPP keyword output instead:
//!
//! - `lpstat -e` — bare queue names (not localized).
//! - `lpstat -d` — default; we keep the trailing token only if it is a known queue.
//! - `lpoptions -p N` — IPP metadata (`device-uri`, `printer-state`, `printer-make-and-model`, `marker-colors`/`marker-levels`).
//! - `lpoptions -p N -l` — capability lists (`ColorModel`, `Duplex`, `PageSize`, `InputSlot`, `Resolution`).
//! - `ippfind` — mDNS/Bonjour discovery.
//!
//! Per-queue enumeration is best-effort: missing tools or fields degrade to
//! empty/`None`, because a half-readable printer is still worth showing.
//! **Discovery is the exception**: a scan that could not run returns `Err` so
//! the UI can say so, instead of claiming the network holds no printers.

use std::collections::HashMap;
use std::process::Command;

use anyhow::Context;

use crate::ipc::printers::{
    DiscoveredPrinter, Printer, PrinterKind, PrinterStatus, Supplies, Tray,
};
use crate::platform::infer_kind;

/// Run a CLI in the C locale and return its stdout (lossy UTF-8).
fn lc_run(program: &str, args: &[&str]) -> anyhow::Result<String> {
    let out = Command::new(program)
        .args(args)
        .env("LC_ALL", "C")
        .env("LANG", "C")
        .output()
        .with_context(|| format!("failed to spawn `{program}`"))?;
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Run a CLI and succeed only on a zero exit status.
fn run_ok(program: &str, args: &[&str]) -> anyhow::Result<()> {
    let status = Command::new(program)
        .args(args)
        .status()
        .with_context(|| format!("failed to spawn `{program}`"))?;
    if status.success() {
        Ok(())
    } else {
        anyhow::bail!("`{program}` exited with {status}");
    }
}

/// Run a CLI and, on failure, surface its captured stderr verbatim as the error.
///
/// Unlike [`run_ok`], this lets the tool's own diagnostic (e.g. lpadmin's
/// "Unable to connect to ...") reach the caller — and thus the UI toast —
/// unaltered. Keeps the `LC_ALL=C` locale so messages are not localized.
fn run_ok_capture(program: &str, args: &[&str]) -> anyhow::Result<()> {
    let out = Command::new(program)
        .args(args)
        .env("LC_ALL", "C")
        .env("LANG", "C")
        .output()
        .with_context(|| format!("failed to spawn `{program}`"))?;
    if out.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&out.stderr);
    let msg = stderr.trim();
    if msg.is_empty() {
        anyhow::bail!("`{program}` exited with {}", out.status);
    }
    anyhow::bail!("{msg}");
}

/// Set the CUPS *user* default — writes `~/.cups/lpoptions`, no privileges.
pub fn set_os_default(name: &str) -> anyhow::Result<()> {
    run_ok("lpoptions", &["-d", name])
}

/// Print the branded MaeclyPrint test page, applying the user's configured
/// default paper/tray for this printer (from `PrinterPrefs`).
pub fn print_test_page(name: &str) -> anyhow::Result<()> {
    use std::io::Write as _;

    // The app's saved defaults for this printer.
    let prefs = crate::settings::Config::load_or_default().printers;
    let over = prefs.overrides.get(name).cloned().unwrap_or_default();

    // A friendlier display name for the page.
    let opts = parse_kv(&lc_run("lpoptions", &["-p", name]).unwrap_or_default());
    let display = opts
        .get("printer-info")
        .filter(|s| !s.is_empty())
        .cloned()
        .unwrap_or_else(|| name.to_string());

    let pdf = super::testpage::render(&super::testpage::TestPageInfo {
        printer: display,
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
    let path = tmp.path().to_string_lossy().into_owned();

    // `lp` spools (copies) the file before returning, so the temp file can be
    // dropped afterwards.
    let mut args = vec![
        "-d".to_string(),
        name.to_string(),
        "-t".to_string(),
        "MaeclyPrint Test Page".to_string(),
    ];
    if let Some(paper) = &over.paper {
        args.push("-o".into());
        args.push(format!("media={paper}"));
    }
    if let Some(tray) = &over.tray {
        args.push("-o".into());
        args.push(format!("InputSlot={tray}"));
    }
    args.push(path);
    let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
    run_ok("lp", &arg_refs)
}

/// Open the OS "Printers" settings so the user can add one manually.
/// macOS 13+ deep link, with a legacy-pane fallback. (`open` is macOS-only;
/// on Linux this returns an error the UI surfaces as a toast.)
pub fn open_printer_settings() -> anyhow::Result<()> {
    const URL: &str = "x-apple.systempreferences:com.apple.Print-Scan-Settings.extension";
    if run_ok("open", &[URL]).is_ok() {
        return Ok(());
    }
    run_ok(
        "open",
        &["/System/Library/PreferencePanes/PrintAndFax.prefPane"],
    )
}

// ----------------------------------------------------------------------------
// Adding a printer (`lpadmin`)
// ----------------------------------------------------------------------------

/// Install a CUPS print queue for `uri`, labelled with the human `name`.
///
/// `lpadmin -p EXISTING` silently *modifies* an existing queue, so we derive a
/// legal queue id from `name` and then de-duplicate it against the installed
/// queues before creating anything. Driverless IPP URIs use the `everywhere`
/// model; raw sockets/LPD use the `raw` model; anything else is rejected with a
/// message the UI can show.
pub fn add_printer(name: &str, uri: &str) -> anyhow::Result<()> {
    let base = sanitize_queue_name(name);
    let queue = unique_queue_name(&base, &queue_names());
    // The display/description keeps the user's original text; fall back to the
    // queue id if the name was blank.
    let trimmed = name.trim();
    let display = if trimmed.is_empty() {
        queue.clone()
    } else {
        trimmed.to_string()
    };
    let args = lpadmin_args(&queue, uri, &display)?;
    let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
    run_ok_capture("lpadmin", &arg_refs)
}

/// Turn a free-form display name into a legal CUPS queue id.
///
/// CUPS queue ids may not contain spaces, `/`, or `#`; those become `_`. The
/// result is capped at 127 characters and falls back to `"printer"` when the
/// input sanitizes to nothing (e.g. it was empty or all whitespace).
fn sanitize_queue_name(name: &str) -> String {
    let cleaned: String = name
        .trim()
        .chars()
        .map(|c| {
            if matches!(c, ' ' | '\t' | '/' | '#') {
                '_'
            } else {
                c
            }
        })
        .take(127)
        .collect();
    if cleaned.is_empty() {
        "printer".to_string()
    } else {
        cleaned
    }
}

/// Return `base` if no installed queue already uses it, otherwise the first
/// free `base_2`, `base_3`, … variant. This is what keeps `add` from silently
/// overwriting an existing queue.
fn unique_queue_name(base: &str, existing: &[String]) -> String {
    if !existing.iter().any(|e| e == base) {
        return base.to_string();
    }
    let mut n = 2u32;
    loop {
        let candidate = format!("{base}_{n}");
        if !existing.iter().any(|e| e == &candidate) {
            return candidate;
        }
        n += 1;
    }
}

/// Build the `lpadmin` argument vector, selecting the PPD-less model by scheme:
/// `ipp`/`ipps`/`dnssd` are driverless (`-m everywhere`); `socket`/`lpd` are
/// raw byte streams (`-m raw`). Any other scheme is rejected.
fn lpadmin_args(queue: &str, uri: &str, display: &str) -> anyhow::Result<Vec<String>> {
    let model = match scheme(uri) {
        "ipp" | "ipps" | "dnssd" => "everywhere",
        "socket" | "lpd" => "raw",
        other => anyhow::bail!(
            "MaeclyPrint can't add a printer over '{other}'. Add it from your \
             system's printer settings instead."
        ),
    };
    Ok(vec![
        "-p".into(),
        queue.into(),
        "-E".into(),
        "-v".into(),
        uri.into(),
        "-m".into(),
        model.into(),
        "-D".into(),
        display.into(),
    ])
}

pub fn list_printers() -> anyhow::Result<Vec<Printer>> {
    let names = queue_names();
    if names.is_empty() {
        return Ok(Vec::new());
    }
    let default = default_printer().unwrap_or(None);
    let jobs = parse_jobs(&lc_run("lpstat", &["-o"]).unwrap_or_default());

    Ok(names
        .iter()
        .map(|name| detail(name, default.as_deref(), &jobs))
        .collect())
}

pub fn default_printer() -> anyhow::Result<Option<String>> {
    let names = queue_names();
    let out = lc_run("lpstat", &["-d"]).unwrap_or_default();
    // The line is "<localized label>: NAME"; queue names have no spaces, so the
    // trailing token is the name — but only trust it if it is a real queue.
    let token = out.split_whitespace().last().unwrap_or("").to_string();
    Ok(if names.iter().any(|n| n == &token) {
        Some(token)
    } else {
        None
    })
}

/// Run `ippfind`, telling "the scan found nothing" apart from "the scan could
/// not run".
///
/// This deliberately does NOT go through [`lc_run`]/[`run_ok`]: `ippfind` exits
/// **1** when it matched no service, which is an empty network — the normal,
/// successful outcome of scanning a room with no printers in it. Only a spawn
/// failure (the CUPS client tools are not installed), a signal, or a non-0/1
/// status is a real error, and that error must reach the UI instead of being
/// painted as "no printers found" (which is what `unwrap_or_default()` used to
/// do).
fn run_ippfind(args: &[&str]) -> anyhow::Result<String> {
    let out = match Command::new("ippfind")
        .args(args)
        .env("LC_ALL", "C")
        .env("LANG", "C")
        .output()
    {
        Ok(out) => out,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => anyhow::bail!(
            "`ippfind` is not installed — network discovery needs the CUPS client tools"
        ),
        Err(e) => anyhow::bail!("failed to run `ippfind`: {e}"),
    };
    classify_ippfind(
        out.status.code(),
        &out.status.to_string(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        &String::from_utf8_lossy(&out.stderr),
    )
}

/// Pure exit-status policy for [`run_ippfind`], split out so it is unit-testable
/// without spawning anything: exit **0** (matches) and **1** (no matches) are
/// both successful scans; anything else surfaces the tool's own stderr.
fn classify_ippfind(
    code: Option<i32>,
    status_text: &str,
    stdout: String,
    stderr: &str,
) -> anyhow::Result<String> {
    if matches!(code, Some(0) | Some(1)) {
        return Ok(stdout);
    }
    let msg = stderr.trim();
    if msg.is_empty() {
        anyhow::bail!("`ippfind` exited with {status_text}");
    }
    anyhow::bail!("{msg}");
}

pub fn discover_printers() -> anyhow::Result<Vec<DiscoveredPrinter>> {
    // `-T 4` bounds the scan; ippfind prints one URI per match by default.
    // A failure propagates: the UI renders a scan error, not an empty network.
    let out = run_ippfind(&["_ipp._tcp", "_ipps._tcp", "-T", "4"])?;
    // Skip anything already installed (matched loosely by host/service).
    let installed: Vec<String> = queue_names()
        .iter()
        .filter_map(|n| {
            parse_kv(&lc_run("lpoptions", &["-p", n]).unwrap_or_default()).remove("device-uri")
        })
        .collect();

    let mut seen = std::collections::HashSet::new();
    let mut out_vec = Vec::new();
    for uri in out.lines().map(str::trim).filter(|l| l.contains("://")) {
        if !seen.insert(uri.to_string()) {
            continue;
        }
        if installed.iter().any(|d| same_device(d, uri)) {
            continue;
        }
        out_vec.push(DiscoveredPrinter {
            id: uri.to_string(),
            name: uri_service_name(uri),
            connection: pretty_conn(Some(uri)),
            kind: PrinterKind::Mfp,
            via: format!("mDNS · {}", scheme(uri).to_uppercase()),
        });
    }
    Ok(out_vec)
}

// ----------------------------------------------------------------------------
// Per-printer assembly
// ----------------------------------------------------------------------------

fn queue_names() -> Vec<String> {
    lc_run("lpstat", &["-e"])
        .unwrap_or_default()
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(String::from)
        .collect()
}

fn detail(name: &str, default: Option<&str>, jobs: &HashMap<String, u32>) -> Printer {
    let opts = parse_kv(&lc_run("lpoptions", &["-p", name]).unwrap_or_default());
    let caps = parse_caps(&lc_run("lpoptions", &["-p", name, "-l"]).unwrap_or_default());

    let driver = opts
        .get("printer-make-and-model")
        .cloned()
        .unwrap_or_default();
    let uri = opts.get("device-uri").map(String::as_str);
    let display = opts
        .get("printer-info")
        .filter(|s| !s.is_empty())
        .cloned()
        .unwrap_or_else(|| name.to_string());

    Printer {
        id: name.to_string(),
        name: display,
        kind: infer_kind(&driver, name, uri),
        color: color_capable(&caps),
        duplex: duplex_capable(&caps),
        status: status_from(
            opts.get("printer-state").map(String::as_str),
            opts.get("printer-state-reasons").map(String::as_str),
        ),
        is_default: default == Some(name),
        location: opts.get("printer-location").cloned().unwrap_or_default(),
        connection: pretty_conn(uri),
        driver,
        shared: opts
            .get("printer-is-shared")
            .map(|v| v == "true")
            .unwrap_or(false),
        queued_jobs: jobs.get(name).copied().unwrap_or(0),
        max_dpi: max_dpi(&caps),
        papers: papers(&caps),
        trays: trays(&caps),
        supplies: parse_supplies(
            opts.get("marker-colors").map(String::as_str),
            opts.get("marker-levels").map(String::as_str),
        ),
        default_tray: caps.get("InputSlot").and_then(|c| c.default.clone()),
        default_paper: caps.get("PageSize").and_then(|c| c.default.clone()),
    }
}

// ----------------------------------------------------------------------------
// Parsers (pure — unit-tested against captured real output)
// ----------------------------------------------------------------------------

/// Parse a single `lpoptions -p NAME` line of space-separated `key=value`
/// pairs, honoring single-quoted values (which may contain spaces) and bare
/// keys (no `=`, meaning empty). Backslash-escaped spaces are unescaped.
fn parse_kv(line: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for tok in tokenize(line) {
        let (k, v) = match tok.split_once('=') {
            Some((k, v)) => (k, unquote(v)),
            None => (tok.as_str(), String::new()),
        };
        if !k.is_empty() {
            map.insert(k.to_string(), v);
        }
    }
    map
}

/// Split on whitespace, but keep single-quoted runs together.
fn tokenize(line: &str) -> Vec<String> {
    let mut toks = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    for ch in line.chars() {
        match ch {
            '\'' => quoted = !quoted,
            c if c.is_whitespace() && !quoted => {
                if !cur.is_empty() {
                    toks.push(std::mem::take(&mut cur));
                }
            }
            c => cur.push(c),
        }
    }
    if !cur.is_empty() {
        toks.push(cur);
    }
    toks
}

fn unquote(v: &str) -> String {
    v.trim_matches('\'').replace("\\ ", " ")
}

#[derive(Default)]
struct Cap {
    values: Vec<String>,
    default: Option<String>,
}

/// Parse `lpoptions -p NAME -l` lines:
/// `Keyword/Human label: val1 *defaultVal val3`.
fn parse_caps(text: &str) -> HashMap<String, Cap> {
    let mut map = HashMap::new();
    for line in text.lines() {
        let Some((left, right)) = line.split_once(':') else {
            continue;
        };
        let keyword = left.split('/').next().unwrap_or("").trim();
        if keyword.is_empty() {
            continue;
        }
        let mut cap = Cap::default();
        for raw in right.split_whitespace() {
            if let Some(def) = raw.strip_prefix('*') {
                cap.default = Some(def.to_string());
                cap.values.push(def.to_string());
            } else {
                cap.values.push(raw.to_string());
            }
        }
        map.insert(keyword.to_string(), cap);
    }
    map
}

/// The `ColorModel` choices `lpoptions -p <id> -l` lists for queue `id`.
pub fn color_models(id: &str) -> Vec<String> {
    parse_caps(&lc_run("lpoptions", &["-p", id, "-l"]).unwrap_or_default())
        .remove("ColorModel")
        .map(|c| c.values)
        .unwrap_or_default()
}

fn is_grayscale(v: &str) -> bool {
    matches!(
        v.to_ascii_lowercase().as_str(),
        "gray" | "grayscale" | "fastgray" | "mono" | "monochrome" | "black" | "kgray" | "bw"
    )
}

fn color_capable(caps: &HashMap<String, Cap>) -> bool {
    caps.get("ColorModel")
        .map(|c| c.values.iter().any(|v| !is_grayscale(v)))
        .unwrap_or(false)
}

fn duplex_capable(caps: &HashMap<String, Cap>) -> bool {
    caps.get("Duplex")
        .map(|c| c.values.iter().any(|v| !v.eq_ignore_ascii_case("None")))
        .unwrap_or(false)
}

fn papers(caps: &HashMap<String, Cap>) -> Vec<String> {
    caps.get("PageSize")
        .map(|c| {
            c.values
                .iter()
                .filter(|v| !v.contains("Custom"))
                .cloned()
                .collect()
        })
        .unwrap_or_default()
}

fn trays(caps: &HashMap<String, Cap>) -> Vec<Tray> {
    caps.get("InputSlot")
        .map(|c| {
            c.values
                .iter()
                .map(|name| Tray {
                    name: name.clone(),
                    paper: None,
                    level: None,
                })
                .collect()
        })
        .unwrap_or_default()
}

fn max_dpi(caps: &HashMap<String, Cap>) -> Option<u32> {
    let cap = caps.get("Resolution")?;
    cap.values
        .iter()
        .filter_map(|v| {
            // values like "600dpi" or "1200x1200dpi" — take the leading number.
            let digits: String = v.chars().take_while(|c| c.is_ascii_digit()).collect();
            digits.parse::<u32>().ok()
        })
        .max()
}

fn status_from(state: Option<&str>, reasons: Option<&str>) -> PrinterStatus {
    let reasons = reasons.unwrap_or("").to_ascii_lowercase();
    if reasons.contains("offline") || reasons.contains("shutdown") {
        return PrinterStatus::Offline;
    }
    match state {
        Some("4") => PrinterStatus::Busy,
        Some("5") => PrinterStatus::Error,
        _ => PrinterStatus::Ready,
    }
}

fn parse_supplies(colors: Option<&str>, levels: Option<&str>) -> Supplies {
    let mut s = Supplies::default();
    let (Some(colors), Some(levels)) = (colors, levels) else {
        return s;
    };
    for (hex, lvl) in colors.split(',').zip(levels.split(',')) {
        let Ok(pct) = lvl.trim().parse::<f64>() else {
            continue;
        };
        let frac = (pct / 100.0).clamp(0.0, 1.0);
        match hex.trim().to_ascii_uppercase().as_str() {
            "#000000" => s.black = Some(frac),
            "#00FFFF" => s.cyan = Some(frac),
            "#FF00FF" => s.magenta = Some(frac),
            "#FFFF00" => s.yellow = Some(frac),
            _ => {}
        }
    }
    s
}

/// Count queued jobs per destination from `lpstat -o`. Job ids are
/// `<dest>-<num>`; the destination may itself contain `-`.
fn parse_jobs(text: &str) -> HashMap<String, u32> {
    let mut map = HashMap::new();
    for line in text.lines() {
        let Some(job_id) = line.split_whitespace().next() else {
            continue;
        };
        if let Some((dest, num)) = job_id.rsplit_once('-') {
            if !num.is_empty() && num.chars().all(|c| c.is_ascii_digit()) {
                *map.entry(dest.to_string()).or_insert(0) += 1;
            }
        }
    }
    map
}

// ----------------------------------------------------------------------------
// URI / kind helpers
// ----------------------------------------------------------------------------

fn scheme(uri: &str) -> &str {
    uri.split("://").next().unwrap_or("")
}

fn percent_decode(s: &str) -> String {
    s.replace("%20", " ").replace("%2F", "/")
}

/// Service/host name extracted from a printer URI, for display.
fn uri_service_name(uri: &str) -> String {
    let rest = uri.split("://").nth(1).unwrap_or(uri);
    let head = rest
        .split(['/', '?'])
        .next()
        .unwrap_or(rest)
        .split("._ipp")
        .next()
        .unwrap_or(rest);
    percent_decode(head.trim_end_matches('.'))
}

/// Loose match of two device URIs (same scheme-less host/service).
fn same_device(a: &str, b: &str) -> bool {
    !uri_service_name(a).is_empty() && uri_service_name(a) == uri_service_name(b)
}

fn pretty_conn(uri: Option<&str>) -> String {
    let Some(uri) = uri.filter(|u| !u.is_empty()) else {
        return "Local".to_string();
    };
    match scheme(uri) {
        "usb" => "USB".to_string(),
        "dnssd" | "ipp" | "ipps" => format!("{} (IPP)", uri_service_name(uri)),
        "socket" => format!("{} (Raw 9100)", uri_service_name(uri)),
        "lpd" => format!("{} (LPD)", uri_service_name(uri)),
        other if !other.is_empty() => {
            format!("{} ({})", uri_service_name(uri), other.to_uppercase())
        }
        _ => uri.to_string(),
    }
}

// ----------------------------------------------------------------------------
// Tests — fixtures are verbatim from real macOS CUPS output.
// ----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const LPOPTIONS_P: &str = "copies=1 device-uri=dnssd://EPSON%20L15160%20Series._ipps._tcp.local./?uuid=cfe92100 finishings=3 marker-colors=#000000,#00FFFF,#FF00FF,#FFFF00 marker-levels=38,52,51,55 marker-names='Black\\ ink,Cyan\\ ink,Magenta\\ ink,Yellow\\ ink' printer-info='EPSON L15160 Series' printer-is-shared=false printer-location printer-make-and-model='EPSON L15160 Series-AirPrint' printer-state=3 printer-state-reasons=media-empty-report";

    const LPOPTIONS_L: &str = "ColorModel/Color Mode: Gray *RGB\nDuplex/2-Sided Printing: None *DuplexNoTumble DuplexTumble\nPageSize/Media Size: A3 A4 *Letter Legal Custom.WIDTHxHEIGHT\nInputSlot/Media Source: auto main alternate rear\ncupsPrintQuality/Quality: *Normal High";

    #[test]
    fn parse_kv_handles_quotes_and_bare_keys() {
        let kv = parse_kv(LPOPTIONS_P);
        assert_eq!(kv["printer-info"], "EPSON L15160 Series");
        assert_eq!(kv["printer-make-and-model"], "EPSON L15160 Series-AirPrint");
        assert_eq!(kv["printer-is-shared"], "false");
        assert_eq!(kv["printer-state"], "3");
        // bare key → empty value
        assert_eq!(kv["printer-location"], "");
        // value with embedded '=' keeps the tail
        assert!(kv["device-uri"].contains("uuid=cfe92100"));
        // escaped spaces unescaped inside quotes
        assert_eq!(
            kv["marker-names"],
            "Black ink,Cyan ink,Magenta ink,Yellow ink"
        );
    }

    #[test]
    fn supplies_map_cmyk_by_hex() {
        let kv = parse_kv(LPOPTIONS_P);
        let s = parse_supplies(
            kv.get("marker-colors").map(String::as_str),
            kv.get("marker-levels").map(String::as_str),
        );
        assert_eq!(s.black, Some(0.38));
        assert_eq!(s.cyan, Some(0.52));
        assert_eq!(s.magenta, Some(0.51));
        assert_eq!(s.yellow, Some(0.55));
    }

    #[test]
    fn caps_color_duplex_papers_trays_dpi() {
        let caps = parse_caps(LPOPTIONS_L);
        assert!(color_capable(&caps)); // RGB present
        assert!(duplex_capable(&caps)); // DuplexNoTumble present
        let p = papers(&caps);
        assert!(p.contains(&"A4".to_string()));
        assert!(!p.iter().any(|v| v.contains("Custom"))); // custom filtered
        assert_eq!(caps["PageSize"].default.as_deref(), Some("Letter"));
        assert_eq!(trays(&caps).len(), 4);
        // this fixture has no Resolution line — degrade to None
        assert_eq!(max_dpi(&caps), None);
    }

    #[test]
    fn status_mapping() {
        assert!(matches!(
            status_from(Some("3"), Some("none")),
            PrinterStatus::Ready
        ));
        assert!(matches!(status_from(Some("4"), None), PrinterStatus::Busy));
        assert!(matches!(status_from(Some("5"), None), PrinterStatus::Error));
        assert!(matches!(
            status_from(Some("3"), Some("offline-report")),
            PrinterStatus::Offline
        ));
    }

    #[test]
    fn jobs_counted_per_dest() {
        let out = "EPSON_L15160_Series-7 user 1024 Mon\nEPSON_L15160_Series-8 user 2048 Mon";
        let jobs = parse_jobs(out);
        assert_eq!(jobs["EPSON_L15160_Series"], 2);
    }

    #[test]
    fn pretty_conn_variants() {
        assert_eq!(pretty_conn(Some("usb://EPSON/L15160")), "USB");
        assert_eq!(
            pretty_conn(Some("dnssd://EPSON%20L15160%20Series._ipps._tcp.local./")),
            "EPSON L15160 Series (IPP)"
        );
        assert_eq!(pretty_conn(None), "Local");
    }

    #[test]
    fn sanitize_queue_name_replaces_illegal_chars() {
        assert_eq!(sanitize_queue_name("Front Desk"), "Front_Desk");
        assert_eq!(sanitize_queue_name("HP/LaserJet"), "HP_LaserJet");
        assert_eq!(sanitize_queue_name("Lab#3"), "Lab_3");
    }

    #[test]
    fn sanitize_queue_name_falls_back_when_empty() {
        assert_eq!(sanitize_queue_name(""), "printer");
        assert_eq!(sanitize_queue_name("   "), "printer");
    }

    #[test]
    fn sanitize_queue_name_caps_length() {
        let out = sanitize_queue_name(&"a".repeat(300));
        assert_eq!(out.chars().count(), 127);
    }

    #[test]
    fn unique_queue_name_passes_through_and_suffixes() {
        let existing = vec!["Office".to_string(), "Office_2".to_string()];
        // no collision → unchanged
        assert_eq!(unique_queue_name("Lab", &existing), "Lab");
        // collides with Office and Office_2 → first free suffix is _3
        assert_eq!(unique_queue_name("Office", &existing), "Office_3");
    }

    #[test]
    fn lpadmin_args_selects_model_by_scheme() {
        // Value that follows the `-m` flag in an argument vector.
        fn model_of(args: &[String]) -> Option<&str> {
            let i = args.iter().position(|a| a == "-m")?;
            args.get(i + 1).map(String::as_str)
        }
        let ipp = lpadmin_args("Q", "ipp://host/ipp/print", "Disp").unwrap();
        assert_eq!(model_of(&ipp), Some("everywhere"));
        let dnssd = lpadmin_args("Q", "dnssd://Printer._ipp._tcp.local./", "Disp").unwrap();
        assert_eq!(model_of(&dnssd), Some("everywhere"));
        let sock = lpadmin_args("Q", "socket://10.0.0.5:9100", "Disp").unwrap();
        assert_eq!(model_of(&sock), Some("raw"));
        let lpd = lpadmin_args("Q", "lpd://10.0.0.5/queue", "Disp").unwrap();
        assert_eq!(model_of(&lpd), Some("raw"));
        // unknown scheme is rejected
        assert!(lpadmin_args("Q", "usb://HP/Printer", "Disp").is_err());
    }

    /// Real `lpadmin` round-trip. Ignored by default so it never runs in CI;
    /// exercise it manually with `cargo test -- --ignored add_and_remove`.
    #[test]
    #[ignore]
    fn add_and_remove_real_queue() {
        // A raw socket queue can be created without a reachable device.
        add_printer("MaeclyPrint Test Queue", "socket://127.0.0.1:9100").expect("add should work");
        // Best-effort cleanup of the queue id sanitize_queue_name would produce.
        let _ = run_ok("lpadmin", &["-x", "MaeclyPrint_Test_Queue"]);
    }

    #[test]
    fn kind_inference() {
        // The keyword table itself lives in `platform::mod` and is tested
        // there; this only pins the CUPS call shape (model + queue + URI).
        assert!(matches!(
            infer_kind("Zebra ZD420 ZPL", "labels", None),
            PrinterKind::Label
        ));
        assert!(matches!(
            infer_kind("Generic", "Save_as_PDF", Some("cups-pdf:/")),
            PrinterKind::Virtual
        ));
        assert!(matches!(
            infer_kind("HP LaserJet M404", "office", Some("usb://HP/M404")),
            PrinterKind::Laser
        ));
    }

    /// `ippfind` exits 1 on an empty network; that must stay a successful,
    /// empty scan rather than becoming an error the UI paints red.
    #[test]
    fn fr_classify_ippfind_no_matches_is_an_empty_scan_not_a_failure() {
        assert_eq!(
            classify_ippfind(Some(1), "exit status: 1", String::new(), "").unwrap(),
            ""
        );
        assert_eq!(
            classify_ippfind(Some(0), "exit status: 0", "ipp://a/\n".into(), "").unwrap(),
            "ipp://a/\n"
        );
    }

    /// A broken scan must NOT be reported as "no printers on the network".
    #[test]
    fn fr_classify_ippfind_surfaces_real_failures() {
        let e = classify_ippfind(Some(2), "exit status: 2", String::new(), "bad argument")
            .expect_err("a non-0/1 exit is a failed scan");
        assert_eq!(e.to_string(), "bad argument");

        // Killed by a signal (no exit code) with nothing on stderr.
        let e = classify_ippfind(None, "signal: 9", String::new(), "  ")
            .expect_err("a signalled scan is a failed scan");
        assert!(e.to_string().contains("signal: 9"), "{e}");
    }

    /// Real-environment smoke test: enumeration must not error and must yield
    /// well-formed records (tolerates zero printers on a headless CI runner).
    ///
    /// Ignored by default — it shells out to the host's CUPS install, so its
    /// result depends on the developer's real queues. `docs/08-development/
    /// testing-strategy.md` § "When to skip subprocess tests" requires exactly
    /// this gate.
    ///
    /// Nothing in the blocking CI lanes runs it: the only job that does is the
    /// advisory `ignored-tests` lane in `.github/workflows/ci.yml`, which runs
    /// `cargo test -- --ignored` on a schedule (Mondays) or on demand, on
    /// macOS, with `continue-on-error: true`. So a failure here never blocks a
    /// merge — treat it as a weekly signal, and run it locally with
    /// `cargo test --manifest-path src-tauri/Cargo.toml -- --ignored
    /// list_printers_smoke`.
    #[test]
    #[ignore = "requires a real CUPS install; run with -- --ignored"]
    fn list_printers_smoke() {
        let printers = list_printers().expect("list_printers should not error");
        for p in &printers {
            assert!(!p.id.is_empty(), "every printer needs an id");
            for (ch, lvl) in [
                p.supplies.black,
                p.supplies.cyan,
                p.supplies.magenta,
                p.supplies.yellow,
            ]
            .iter()
            .filter_map(|v| v.map(|x| (x, x)))
            {
                let _ = ch;
                assert!((0.0..=1.0).contains(&lvl), "supply levels are normalized");
            }
        }
        // default, if any, must be one of the listed printers
        if let Some(def) = default_printer().unwrap() {
            assert!(printers.iter().any(|p| p.id == def));
        }
    }
}
