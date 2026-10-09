//! CUPS backend: `lp` argument construction (pure, all platforms) and, on Unix,
//! spooling and cancellation.
//!
//! Flag mapping (`docs/03-requirements/print-settings.md` §CUPS table):
//!
//! | Setting              | `lp` flag                                             |
//! |----------------------|-------------------------------------------------------|
//! | printer              | `-d <name>`                                           |
//! | title                | `-t <title>`                                          |
//! | duplex none          | `-o sides=one-sided`                                  |
//! | duplex long          | `-o sides=two-sided-long-edge`                        |
//! | duplex short         | `-o sides=two-sided-short-edge`                       |
//! | colour               | *(omitted — driver default)*                          |
//! | monochrome           | `-o ColorModel=Gray`                                  |
//! | copies > 1           | `-n <copies>`                                         |
//! | paper                | `-o media=A4\|Letter\|Legal\|A3`                      |
//! | tray                 | `-o InputSlot=<bin> -o media-source=<bin>`            |
//! | quality              | `-o print-quality=3\|4\|5 -o cupsPrintQuality=…`      |
//! | orientation landscape| `-o landscape` *(portrait/auto omitted)*              |
//! | scaling fit/shrink   | `-o fit-to-page` *(none omitted)*                     |
//! | page ranges          | `-o page-ranges=<r>`                                  |
//!
//! `sides` and `media` are always emitted so an explicit "none"/paper choice
//! wins over any driver default; `quality` is emitted for the same reason (a
//! user who picks "normal" must not inherit a driver default of "draft"). CUPS
//! has a single `fit-to-page` toggle and cannot distinguish "shrink only" from
//! "fit", so both map to it.
//!
//! **Why two spellings for tray and quality.** The same choice has a PPD name
//! and an IPP name, and which one a queue understands depends on how it was
//! created: PPD-backed queues take `InputSlot` / `cupsPrintQuality`, while
//! driverless (IPP Everywhere) queues take the standard job attributes
//! `media-source` / `print-quality`. Both carry the identical value, and CUPS
//! ignores a job option a queue does not support, so emitting the pair is what
//! makes the setting actually apply on both kinds of queue instead of applying
//! on one and silently vanishing on the other.

use std::path::Path;

use crate::queue::job::{ColorMode, Duplex, JobSettings, Orientation, PaperSize, Quality, Scaling};

#[cfg(unix)]
use anyhow::Context;
#[cfg(unix)]
use std::process::Command;

/// The option flags (everything after `-d <printer> -t <title>` and before the
/// PDF path). Shared by [`lp_args`] and the UI preview so they never drift.
pub fn lp_opts(settings: &JobSettings) -> Vec<String> {
    let mut opts = Vec::new();

    // Sides — always explicit: a user's "none" must force single-sided rather
    // than inherit a duplex driver default.
    opts.push("-o".to_string());
    opts.push(format!(
        "sides={}",
        match settings.duplex {
            Duplex::None => "one-sided",
            Duplex::Long => "two-sided-long-edge",
            Duplex::Short => "two-sided-short-edge",
        }
    ));

    // Colour: only force grey; colour is the driver default so we omit it.
    if matches!(settings.color_mode, ColorMode::Monochrome) {
        opts.push("-o".to_string());
        opts.push("ColorModel=Gray".to_string());
    }

    // Copies use `lp`'s own `-n` flag.
    if settings.copies > 1 {
        opts.push("-n".to_string());
        opts.push(settings.copies.to_string());
    }

    // Paper — always explicit.
    opts.push("-o".to_string());
    opts.push(format!("media={}", media(settings.paper_size)));

    // Tray — only when the job names one; no name means the printer's own
    // default bin, which is not ours to override.
    if let Some(tray) = settings.selected_tray() {
        opts.push("-o".to_string());
        opts.push(format!("InputSlot={tray}"));
        opts.push("-o".to_string());
        opts.push(format!("media-source={tray}"));
    }

    // Quality — always explicit, in both the PPD and the IPP spelling.
    opts.push("-o".to_string());
    opts.push(format!("print-quality={}", ipp_quality(settings.quality)));
    opts.push("-o".to_string());
    opts.push(format!(
        "cupsPrintQuality={}",
        ppd_quality(settings.quality)
    ));

    // Orientation — only landscape needs a flag.
    if matches!(settings.orientation, Orientation::Landscape) {
        opts.push("-o".to_string());
        opts.push("landscape".to_string());
    }

    // Scaling — fit and shrink both collapse to CUPS's single fit-to-page.
    if matches!(settings.scaling, Scaling::Fit | Scaling::Shrink) {
        opts.push("-o".to_string());
        opts.push("fit-to-page".to_string());
    }

    // Page ranges — whitespace-stripped so the value is a single clean token.
    if let Some(ranges) = &settings.page_ranges {
        let compact = strip_ws(ranges);
        if !compact.is_empty() {
            opts.push("-o".to_string());
            opts.push(format!("page-ranges={compact}"));
        }
    }

    opts
}

/// Full `lp` argument vector: `-d <printer> -t <title>` + [`lp_opts`] + the PDF
/// path as the final argument.
pub fn lp_args(printer: &str, pdf: &Path, settings: &JobSettings, title: &str) -> Vec<String> {
    let mut args = vec![
        "-d".to_string(),
        printer.to_string(),
        "-t".to_string(),
        title.to_string(),
    ];
    args.extend(lp_opts(settings));
    args.push(pdf.to_string_lossy().into_owned());
    args
}

/// Parse `lp`'s success line into the `<dest>-<num>` request id.
///
/// In English the line is `request id is <dest>-<num> (1 file(s))`, but
/// macOS localizes it from the user's language list whatever `LC_ALL` says
/// (`el id de la petición es Maecly_Hold-1537 (1 archivos)` on a Spanish
/// Mac), so the marker cannot be relied on. The id itself is not localized:
/// it is `<dest>-<digits>`, and `dest` is the queue we just spooled to. So the
/// English marker is tried first, then a token naming `printer`, then any
/// `<name>-<digits>` token. `<dest>` may itself contain `-`, which is why the
/// number is only ever the last segment.
pub fn parse_request_id(stdout: &str, printer: &str) -> Option<String> {
    const MARKER: &str = "request id is ";
    if let Some(start) = stdout.find(MARKER) {
        if let Some(tok) = stdout[start + MARKER.len()..].split_whitespace().next() {
            if is_request_id(tok) {
                return Some(tok.to_string());
            }
        }
    }
    let tokens: Vec<&str> = stdout
        .split_whitespace()
        .map(|t| t.trim_matches(|c: char| matches!(c, '(' | ')' | ',' | '.' | ':' | ';' | '"')))
        .collect();
    let named = format!("{printer}-");
    tokens
        .iter()
        .find(|t| t.starts_with(&named) && is_request_id(t))
        .or_else(|| tokens.iter().find(|t| is_request_id(t)))
        .map(|t| t.to_string())
}

/// `<non-empty name>-<digits>`, the shape of every CUPS request id.
fn is_request_id(token: &str) -> bool {
    match token.rsplit_once('-') {
        Some((dest, num)) => {
            !dest.is_empty() && !num.is_empty() && num.chars().all(|c| c.is_ascii_digit())
        }
        None => false,
    }
}

/// IPP `print-quality` enum value (RFC 8011: 3 draft, 4 normal, 5 high), as
/// understood by driverless queues.
fn ipp_quality(quality: Quality) -> u8 {
    match quality {
        Quality::Draft => 3,
        Quality::Normal => 4,
        Quality::High => 5,
    }
}

/// PPD `cupsPrintQuality` keyword, as understood by PPD-backed queues.
fn ppd_quality(quality: Quality) -> &'static str {
    match quality {
        Quality::Draft => "Draft",
        Quality::Normal => "Normal",
        Quality::High => "High",
    }
}

fn media(paper: PaperSize) -> &'static str {
    match paper {
        PaperSize::A4 => "A4",
        PaperSize::Letter => "Letter",
        PaperSize::Legal => "Legal",
        PaperSize::A3 => "A3",
    }
}

/// Remove *all* whitespace from a page-range string (`" 1-5, 8 "` → `"1-5,8"`),
/// keeping it a single shell token.
fn strip_ws(s: &str) -> String {
    s.split_whitespace().collect()
}

/// Spool `pdf` via `lp`. On success returns the CUPS job id (when the line
/// parses); a non-zero exit becomes [`EngineError::PrintFailed`] carrying the
/// trimmed stderr. Run in the C locale so CUPS's prose stays parseable.
///
/// The child is spawned rather than run through `Command::output()` so the wait
/// can be bounded by `timeout` and the process killed when it expires (see
/// [`super::wait_with_timeout`]) — a hung `lp` would otherwise hold the queue's
/// single print slot forever. `lp` writes a single short line to each pipe, so
/// draining them after the wait cannot deadlock on a full pipe buffer.
#[cfg(unix)]
pub fn spool(
    pdf: &Path,
    printer: &str,
    settings: &JobSettings,
    title: &str,
    timeout: Option<std::time::Duration>,
) -> Result<super::SpoolOutcome, super::PrintError> {
    use crate::engine::EngineError;
    use std::io::Read;

    let args = lp_args(printer, pdf, settings, title);
    let mut child = Command::new("lp")
        .args(&args)
        .env("LC_ALL", "C")
        .env("LANG", "C")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| {
            EngineError::DependencyMissing(format!("failed to run `lp` (is CUPS installed?): {e}"))
        })?;

    let status = super::wait_with_timeout(&mut child, timeout)?;

    let read = |pipe: Option<&mut dyn Read>| {
        let mut buf = String::new();
        if let Some(p) = pipe {
            let _ = p.read_to_string(&mut buf);
        }
        buf
    };
    let stdout = read(child.stdout.as_mut().map(|p| p as &mut dyn Read));
    let stderr = read(child.stderr.as_mut().map(|p| p as &mut dyn Read));

    if status.success() {
        Ok(super::SpoolOutcome {
            os_job_id: parse_request_id(&stdout, printer),
        })
    } else {
        let msg = stderr.trim();
        Err(EngineError::PrintFailed(if msg.is_empty() {
            format!("lp exited with {status}")
        } else {
            msg.to_string()
        })
        .into())
    }
}

/// Best-effort cancellation of a CUPS job by its id (`cancel <id>`).
#[cfg(unix)]
pub fn cancel(os_job_id: &str) -> anyhow::Result<()> {
    let status = Command::new("cancel")
        .arg(os_job_id)
        .status()
        .with_context(|| format!("failed to run `cancel {os_job_id}`"))?;
    if status.success() {
        Ok(())
    } else {
        anyhow::bail!("`cancel {os_job_id}` exited with {status}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn strs(v: &[String]) -> Vec<&str> {
        v.iter().map(String::as_str).collect()
    }

    #[test]
    fn defaults_produce_minimal_args() {
        let args = lp_args(
            "Office",
            Path::new("/tmp/x.pdf"),
            &JobSettings::default(),
            "Doc",
        );
        assert_eq!(
            strs(&args),
            vec![
                "-d",
                "Office",
                "-t",
                "Doc",
                "-o",
                "sides=one-sided",
                "-o",
                "media=A4",
                "-o",
                "print-quality=4",
                "-o",
                "cupsPrintQuality=Normal",
                "-o",
                "fit-to-page",
                "/tmp/x.pdf",
            ]
        );
    }

    #[test]
    fn every_setting_maps_in_stable_order() {
        let settings = JobSettings {
            printer: None,
            duplex: Duplex::Short,
            color_mode: ColorMode::Monochrome,
            copies: 3,
            paper_size: PaperSize::Legal,
            orientation: Orientation::Landscape,
            scaling: Scaling::Shrink,
            page_ranges: Some(" 1-5, 8 ".to_string()),
            quality: Quality::High,
            tray: Some("Tray 2".to_string()),
        };
        assert_eq!(
            strs(&lp_opts(&settings)),
            vec![
                "-o",
                "sides=two-sided-short-edge",
                "-o",
                "ColorModel=Gray",
                "-n",
                "3",
                "-o",
                "media=Legal",
                "-o",
                "InputSlot=Tray 2",
                "-o",
                "media-source=Tray 2",
                "-o",
                "print-quality=5",
                "-o",
                "cupsPrintQuality=High",
                "-o",
                "landscape",
                "-o",
                "fit-to-page",
                "-o",
                "page-ranges=1-5,8",
            ]
        );
    }

    #[test]
    fn long_edge_and_a3_and_no_scale() {
        let settings = JobSettings {
            duplex: Duplex::Long,
            paper_size: PaperSize::A3,
            scaling: Scaling::None,
            ..JobSettings::default()
        };
        assert_eq!(
            strs(&lp_opts(&settings)),
            vec![
                "-o",
                "sides=two-sided-long-edge",
                "-o",
                "media=A3",
                "-o",
                "print-quality=4",
                "-o",
                "cupsPrintQuality=Normal",
            ]
        );
    }

    /// FR-041: Quality and Tray are editable in the profile editor, so they have
    /// to reach `lp` — in both the IPP and the PPD spelling, since which one a
    /// queue understands depends on how it was created.
    #[test]
    fn fr_041_quality_and_tray_reach_lp() {
        let with = |quality, tray: Option<&str>| {
            lp_opts(&JobSettings {
                quality,
                tray: tray.map(String::from),
                ..JobSettings::default()
            })
        };

        for (quality, ipp, ppd) in [
            (Quality::Draft, "print-quality=3", "cupsPrintQuality=Draft"),
            (
                Quality::Normal,
                "print-quality=4",
                "cupsPrintQuality=Normal",
            ),
            (Quality::High, "print-quality=5", "cupsPrintQuality=High"),
        ] {
            let opts = with(quality, None);
            assert!(opts.iter().any(|o| o == ipp), "{quality:?}: {opts:?}");
            assert!(opts.iter().any(|o| o == ppd), "{quality:?}: {opts:?}");
        }

        let opts = with(Quality::Normal, Some("Drawer 1"));
        assert!(opts.iter().any(|o| o == "InputSlot=Drawer 1"), "{opts:?}");
        assert!(
            opts.iter().any(|o| o == "media-source=Drawer 1"),
            "{opts:?}"
        );

        // No tray chosen → nothing is said, so the printer's default bin wins.
        let opts = with(Quality::Normal, None);
        assert!(
            !opts.iter().any(|o| o.starts_with("InputSlot=")),
            "{opts:?}"
        );
        assert!(
            !opts.iter().any(|o| o.starts_with("media-source=")),
            "{opts:?}"
        );
        // A blank name is not a bin name (`validate` rejects it too).
        let opts = with(Quality::Normal, Some("   "));
        assert!(
            !opts.iter().any(|o| o.starts_with("InputSlot=")),
            "{opts:?}"
        );
    }

    #[test]
    fn parse_request_id_handles_hyphenated_dests() {
        assert_eq!(
            parse_request_id("request id is Office-42 (1 file(s))", "Office"),
            Some("Office-42".to_string())
        );
        assert_eq!(
            parse_request_id(
                "request id is EPSON_L15160_Series-7 (1 file(s))",
                "EPSON_L15160_Series"
            ),
            Some("EPSON_L15160_Series-7".to_string())
        );
        assert_eq!(parse_request_id("no request id present", "Office"), None);
        assert_eq!(parse_request_id("", "Office"), None);
    }

    /// macOS localizes `lp`'s line from the user's language list even under
    /// `LC_ALL=C` — captured verbatim on a Spanish Mac. Without this the job id
    /// was lost, so the job could neither be tracked nor cancelled.
    #[test]
    fn parse_request_id_survives_a_localized_lp() {
        assert_eq!(
            parse_request_id(
                "el id de la petición es Maecly_Hold-1537 (1 archivos)",
                "Maecly_Hold"
            ),
            Some("Maecly_Hold-1537".to_string())
        );
        assert_eq!(
            parse_request_id(
                "l'identifiant de requête est Front-Desk-9 (1 fichier(s))",
                "Front-Desk"
            ),
            Some("Front-Desk-9".to_string())
        );
        // A number elsewhere on the line is not mistaken for the id.
        assert_eq!(
            parse_request_id("ID 2024 de solicitud: Office-3 (1 archivos)", "Office"),
            Some("Office-3".to_string())
        );
    }
}
