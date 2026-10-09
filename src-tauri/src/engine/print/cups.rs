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
//! | paper                | `-o media=<the printer's keyword>` *(auto omitted)*   |
//! | tray                 | `-o InputSlot=<bin> -o media-source=<bin>`            |
//! | quality              | `-o print-quality=3\|4\|5 -o cupsPrintQuality=…`      |
//! | orientation          | `-o orientation-requested=3\|4` *(auto omitted)*      |
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

use crate::queue::job::{ColorMode, Duplex, JobSettings, Orientation, Quality, Scaling};

use super::PrinterCaps;

#[cfg(unix)]
use anyhow::Context;
#[cfg(unix)]
use std::process::Command;

/// The option flags for a printer whose capabilities are not known — see
/// [`lp_opts_for`].
pub fn lp_opts(settings: &JobSettings) -> Vec<String> {
    lp_opts_for(settings, &PrinterCaps::unknown())
}

/// The option flags (everything after `-d <printer> -t <title>` and before the
/// PDF path) for a printer with capabilities `caps`. Shared by [`lp_args_for`]
/// and the UI preview so they never drift.
pub fn lp_opts_for(settings: &JobSettings, caps: &PrinterCaps) -> Vec<String> {
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

    // Paper — explicit unless the job defers to the printer ("auto"), and in
    // the printer's own spelling when it lists the size.
    if !settings.paper_size.is_auto() {
        let keyword = caps
            .paper_keyword(&settings.paper_size)
            .unwrap_or(settings.paper_size.as_str());
        opts.push("-o".to_string());
        opts.push(format!("media={keyword}"));
    }

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

    // Orientation — an explicit choice is sent explicitly, in the standard IPP
    // attribute (3 portrait, 4 landscape): a "portrait" that emitted nothing
    // would silently inherit a queue whose default is landscape. Only "auto"
    // says nothing, so each page keeps its own orientation.
    match settings.orientation {
        Orientation::Portrait => {
            opts.push("-o".to_string());
            opts.push("orientation-requested=3".to_string());
        }
        Orientation::Landscape => {
            opts.push("-o".to_string());
            opts.push("orientation-requested=4".to_string());
        }
        Orientation::Auto => {}
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

/// Full `lp` argument vector for a printer whose capabilities are not known.
pub fn lp_args(printer: &str, pdf: &Path, settings: &JobSettings, title: &str) -> Vec<String> {
    lp_args_for(printer, pdf, settings, &PrinterCaps::unknown(), title)
}

/// Full `lp` argument vector: `-d <printer> -t <title>` + [`lp_opts_for`] + the
/// PDF path as the final argument.
pub fn lp_args_for(
    printer: &str,
    pdf: &Path,
    settings: &JobSettings,
    caps: &PrinterCaps,
    title: &str,
) -> Vec<String> {
    let mut args = vec![
        "-d".to_string(),
        printer.to_string(),
        "-t".to_string(),
        title.to_string(),
    ];
    args.extend(lp_opts_for(settings, caps));
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
    caps: &PrinterCaps,
    title: &str,
    timeout: Option<std::time::Duration>,
) -> Result<super::SpoolOutcome, super::PrintError> {
    use crate::engine::EngineError;
    use std::io::Read;

    let args = lp_args_for(printer, pdf, settings, caps, title);
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

/// The numeric CUPS job id inside an `lp` request id (`Office-42` → `42`).
/// The destination may itself contain `-`, so the number is the last segment.
pub fn job_number(os_job_id: &str) -> Option<u32> {
    os_job_id.rsplit('-').next()?.parse().ok()
}

/// Map an IPP `job-state` keyword (as `ipptool` prints it) onto
/// [`super::OsJobState`]. Unknown keywords are `None`, never guessed.
pub fn parse_ipp_job_state(keyword: &str) -> Option<super::OsJobState> {
    use super::OsJobState as S;
    Some(match keyword.trim() {
        "pending" => S::Pending,
        "pending-held" => S::Held,
        "processing" => S::Processing,
        "processing-stopped" => S::Stopped,
        "canceled" => S::Canceled,
        "aborted" => S::Aborted,
        "completed" => S::Completed,
        _ => return None,
    })
}

/// Read the job state out of `ipptool -tv … get-job-attributes.test` output:
/// a `job-state (enum) = <keyword>` line, or `client-error-not-found` when the
/// scheduler no longer knows the job.
pub fn parse_ipptool_output(out: &str) -> Option<super::OsJobState> {
    for line in out.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("job-state ") {
            if let Some((_, value)) = rest.split_once('=') {
                return parse_ipp_job_state(value);
            }
        }
    }
    if out.contains("client-error-not-found") {
        return Some(super::OsJobState::Gone);
    }
    None
}

/// Whether `lpstat -W … -o <dest>` output lists `os_job_id` (first column).
pub fn lpstat_lists(out: &str, os_job_id: &str) -> bool {
    out.lines()
        .filter_map(|l| l.split_whitespace().next())
        .any(|id| id == os_job_id)
}

/// Where the CUPS job `os_job_id` stands.
///
/// `ipptool get-job-attributes` is asked first because it is the only source
/// that tells a printed job from a cancelled or aborted one. Where `ipptool`
/// is missing (a minimal Linux install), `lpstat -W` still tells a waiting job
/// from a finished one, which is what keeps the print slot honest.
#[cfg(unix)]
pub fn job_state(os_job_id: &str) -> Result<super::OsJobState, super::TrackError> {
    use super::{OsJobState, TrackError};

    let Some(number) = job_number(os_job_id) else {
        return Err(TrackError::Unavailable(format!(
            "'{os_job_id}' is not a CUPS request id"
        )));
    };

    let uri = format!("ipp://localhost/jobs/{number}");
    match Command::new("ipptool")
        .args(["-tv", &uri, "get-job-attributes.test"])
        .env("LC_ALL", "C")
        .env("LANG", "C")
        .output()
    {
        Ok(out) => {
            let text = String::from_utf8_lossy(&out.stdout);
            if let Some(state) = parse_ipptool_output(&text) {
                return Ok(state);
            }
            // Ran but said nothing usable (no test file, scheduler busy): fall
            // through to lpstat rather than give up.
        }
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
            return Err(TrackError::Transient(format!("failed to run ipptool: {e}")));
        }
        Err(_) => {}
    }

    let dest = os_job_id
        .rsplit_once('-')
        .map(|(d, _)| d)
        .unwrap_or(os_job_id);
    let lpstat = |which: &str| -> Result<String, TrackError> {
        let out = Command::new("lpstat")
            .args(["-W", which, "-o", dest])
            .env("LC_ALL", "C")
            .env("LANG", "C")
            .output()
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    TrackError::Unavailable("neither ipptool nor lpstat is installed".to_string())
                } else {
                    TrackError::Transient(format!("failed to run lpstat: {e}"))
                }
            })?;
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    };
    if lpstat_lists(&lpstat("not-completed")?, os_job_id) {
        return Ok(OsJobState::Processing);
    }
    if lpstat_lists(&lpstat("completed")?, os_job_id) {
        return Ok(OsJobState::Completed);
    }
    Ok(OsJobState::Gone)
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
    use crate::queue::job::PaperSize;
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
            paper_size: PaperSize::LEGAL,
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
                "orientation-requested=4",
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

    /// Portrait is a choice, not the absence of one: it must reach the queue
    /// (a queue can default to landscape). Only auto stays silent.
    #[test]
    fn orientation_portrait_and_landscape_are_explicit_and_auto_is_silent() {
        let with = |orientation| {
            lp_opts(&JobSettings {
                orientation,
                ..JobSettings::default()
            })
        };
        assert!(with(Orientation::Portrait)
            .iter()
            .any(|o| o == "orientation-requested=3"));
        assert!(with(Orientation::Landscape)
            .iter()
            .any(|o| o == "orientation-requested=4"));
        let auto = with(Orientation::Auto);
        assert!(
            !auto.iter().any(|o| o.starts_with("orientation-requested")),
            "{auto:?}"
        );
        assert!(!auto.iter().any(|o| o == "landscape"), "{auto:?}");
    }

    /// Any size the printer lists reaches `lp` in the printer's own spelling;
    /// `auto` sends no media at all, so the printer's own setting applies.
    #[test]
    fn media_uses_the_printers_keyword_and_auto_sends_none() {
        let caps = PrinterCaps {
            papers: vec!["A4".into(), "Letter".into(), "4x6".into(), "Tabloid".into()],
            ..PrinterCaps::unknown()
        };
        let with = |name: &str| {
            lp_opts_for(
                &JobSettings {
                    paper_size: PaperSize::parse(name).unwrap(),
                    ..JobSettings::default()
                },
                &caps,
            )
        };
        assert!(with("tabloid").iter().any(|o| o == "media=Tabloid"));
        assert!(with("4x6").iter().any(|o| o == "media=4x6"));
        assert!(with("letter").iter().any(|o| o == "media=Letter"));
        let auto = with("auto");
        assert!(!auto.iter().any(|o| o.starts_with("media=")), "{auto:?}");
    }

    #[test]
    fn job_number_is_the_last_segment_of_the_request_id() {
        assert_eq!(job_number("Office-42"), Some(42));
        assert_eq!(job_number("EPSON_L15160_Series-7"), Some(7));
        assert_eq!(job_number("Front-Desk-Laser-1522"), Some(1522));
        assert_eq!(job_number("Office"), None);
        assert_eq!(job_number(""), None);
    }

    #[test]
    fn ipp_job_states_map_one_to_one() {
        use crate::engine::print::OsJobState as S;
        for (kw, state) in [
            ("pending", S::Pending),
            ("pending-held", S::Held),
            ("processing", S::Processing),
            ("processing-stopped", S::Stopped),
            ("canceled", S::Canceled),
            ("aborted", S::Aborted),
            ("completed", S::Completed),
        ] {
            assert_eq!(parse_ipp_job_state(kw), Some(state), "{kw}");
        }
        assert_eq!(parse_ipp_job_state("something-new"), None);
    }

    /// Captured from `ipptool -tv ipp://localhost/jobs/1522
    /// get-job-attributes.test` against the `Maecly_Hold` queue.
    #[test]
    fn ipptool_output_yields_the_job_state() {
        use crate::engine::print::OsJobState as S;
        let held = r#""/usr/share/cups/ipptool/get-job-attributes.test":
    Get-Job-Attributes:
        attributes-charset (charset) = utf-8
    Get job info with get-job-attributes                                 [PASS]
        RECEIVED: 431 bytes in response
        status-code = successful-ok (successful-ok)
        job-state (enum) = pending-held
        job-state-reasons (keyword) = job-hold-until-specified
"#;
        assert_eq!(parse_ipptool_output(held), Some(S::Held));
        let done = "        job-state (enum) = completed\n";
        assert_eq!(parse_ipptool_output(done), Some(S::Completed));
        let gone = "        status-code = client-error-not-found (client-error-not-found)\n";
        assert_eq!(parse_ipptool_output(gone), Some(S::Gone));
        assert_eq!(parse_ipptool_output("garbage"), None);
    }

    /// Real-spooler check against the `Maecly_Hold` queue, which holds every
    /// job (nothing prints). Opt-in: `cargo test -- --ignored maecly_hold`.
    #[cfg(unix)]
    #[test]
    #[ignore = "needs the local CUPS queue Maecly_Hold"]
    fn maecly_hold_job_is_tracked_as_held_then_canceled() {
        use crate::engine::print::OsJobState;
        let dir = tempfile::tempdir().unwrap();
        let pdf = dir.path().join("track.pdf");
        std::fs::write(
            &pdf,
            crate::platform::testpage::render(&crate::platform::testpage::TestPageInfo {
                printer: "Maecly_Hold".into(),
                paper: None,
                tray: None,
            }),
        )
        .unwrap();
        let out = spool(
            &pdf,
            "Maecly_Hold",
            &JobSettings::default(),
            &PrinterCaps::unknown(),
            "maecly-track-test",
            Some(std::time::Duration::from_secs(30)),
        )
        .expect("spooled");
        let id = out.os_job_id.expect("lp reports a request id");
        assert_eq!(job_state(&id), Ok(OsJobState::Held));
        cancel(&id).unwrap();
        assert_eq!(job_state(&id), Ok(OsJobState::Canceled));
    }

    #[test]
    fn lpstat_lists_matches_the_first_column_exactly() {
        let out = "Maecly_Hold-1523        ana     1024   Fri Oct  9 15:32:48 2026\n\
                   Maecly_Hold-15230       ana     1024   Fri Oct  9 15:32:48 2026\n";
        assert!(lpstat_lists(out, "Maecly_Hold-1523"));
        assert!(!lpstat_lists(out, "Maecly_Hold-152"));
        assert!(!lpstat_lists("", "Maecly_Hold-1523"));
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
