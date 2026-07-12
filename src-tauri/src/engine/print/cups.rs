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
//! | orientation landscape| `-o landscape` *(portrait/auto omitted)*              |
//! | scaling fit/shrink   | `-o fit-to-page` *(none omitted)*                     |
//! | page ranges          | `-o page-ranges=<r>`                                  |
//!
//! `sides` and `media` are always emitted so an explicit "none"/paper choice
//! wins over any driver default. CUPS has a single `fit-to-page` toggle and
//! cannot distinguish "shrink only" from "fit", so both map to it.

use std::path::Path;

use crate::queue::job::{ColorMode, Duplex, JobSettings, Orientation, PaperSize, Scaling};

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

/// Parse `lp`'s success line — `request id is <dest>-<num> (1 file(s))` — into
/// the `<dest>-<num>` job id. `<dest>` may itself contain `-`, so we take the
/// whole token up to the first space after the marker.
pub fn parse_request_id(stdout: &str) -> Option<String> {
    const MARKER: &str = "request id is ";
    let start = stdout.find(MARKER)? + MARKER.len();
    stdout[start..]
        .split_whitespace()
        .next()
        .map(str::to_string)
        .filter(|s| !s.is_empty())
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
#[cfg(unix)]
pub fn spool(
    pdf: &Path,
    printer: &str,
    settings: &JobSettings,
    title: &str,
) -> Result<super::SpoolOutcome, crate::engine::EngineError> {
    use crate::engine::EngineError;

    let args = lp_args(printer, pdf, settings, title);
    let output = Command::new("lp")
        .args(&args)
        .env("LC_ALL", "C")
        .env("LANG", "C")
        .output()
        .map_err(|e| {
            EngineError::DependencyMissing(format!("failed to run `lp` (is CUPS installed?): {e}"))
        })?;

    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(super::SpoolOutcome {
            os_job_id: parse_request_id(&stdout),
        })
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let msg = stderr.trim();
        Err(EngineError::PrintFailed(if msg.is_empty() {
            format!("lp exited with {}", output.status)
        } else {
            msg.to_string()
        }))
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
            vec!["-o", "sides=two-sided-long-edge", "-o", "media=A3"]
        );
    }

    #[test]
    fn parse_request_id_handles_hyphenated_dests() {
        assert_eq!(
            parse_request_id("request id is Office-42 (1 file(s))"),
            Some("Office-42".to_string())
        );
        assert_eq!(
            parse_request_id("request id is EPSON_L15160_Series-7 (1 file(s))"),
            Some("EPSON_L15160_Series-7".to_string())
        );
        assert_eq!(parse_request_id("no request id present"), None);
        assert_eq!(parse_request_id(""), None);
    }
}
