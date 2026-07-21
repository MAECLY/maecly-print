//! Printer dispatch facade.
//!
//! One printable PDF plus a [`JobSettings`] becomes a real spooler invocation
//! here. The per-backend flag translation lives in [`cups`] (CUPS `lp`, Unix)
//! and [`sumatra`] (SumatraPDF, Windows); this module picks the backend for the
//! current platform and also exposes:
//!
//! - [`preview_command`] — the exact command string shown in the UI's "this is
//!   what we'll run" panel (honesty policy: no hidden flags),
//! - [`check_supported`] — the pure capability gate the dispatcher calls before
//!   spooling,
//! - [`effective_settings`] — the per-input narrowing of the batch settings
//!   (page ranges are a PDF-only concept), and
//! - [`wait_with_timeout`] — the bounded child wait every backend spools
//!   through, so a wedged spooler can never hold the queue forever.

use std::borrow::Cow;
use std::path::Path;
use std::process::{Child, ExitStatus};
use std::time::Duration;

use crate::queue::job::{Duplex, JobSettings};

use super::EngineError;

pub mod cups;
pub mod sumatra;

/// Result of spooling a job to the OS. `os_job_id` is the spooler's own id when
/// the backend reports one (CUPS does; SumatraPDF does not), used later to
/// query status or cancel.
pub struct SpoolOutcome {
    pub os_job_id: Option<String>,
}

/// How the print stage can fail.
///
/// The conversion stage speaks plain [`EngineError`], but printing has two
/// outcomes the dispatcher must treat differently from any other engine
/// failure, so they get their own variants rather than being flattened into
/// [`EngineError::PrintFailed`]:
///
/// - [`PrintError::TimedOut`] is **terminal** (no automatic retry), exactly like
///   the conversion stage's `conversion_timeout` — re-spooling a job that
///   already wedged the spooler once would just wedge it again.
/// - [`PrintError::PrinterMissing`] / [`PrintError::NoPrinter`] are
///   **user-fixable**, so the job is paused for the user to pick a replacement
///   printer (`docs/03-requirements/print-settings.md` §printer,
///   `docs/05-features/printers-management.md` §Missing-printer reconciliation).
///
/// Everything else is an [`EngineError`] carried through unchanged and handled
/// by the ordinary retry policy.
#[derive(Debug, thiserror::Error)]
pub enum PrintError {
    /// The spooler child outlived its wall-clock budget and was killed.
    #[error("printing timed out after {seconds}s")]
    TimedOut { seconds: u64 },

    /// The requested printer is not among the queues the OS enumerates.
    #[error("printer '{printer}' is not installed — choose another printer")]
    PrinterMissing { printer: String },

    /// No printer was requested and neither MaeclyPrint nor the OS has a default.
    #[error("no printer is configured — choose a printer first")]
    NoPrinter,

    /// Any other engine failure, handled by the ordinary retry policy.
    #[error(transparent)]
    Engine(#[from] EngineError),
}

/// Spool `pdf` to `printer` with `settings`, tagging the job with `title`.
///
/// Dispatches to CUPS on Unix and SumatraPDF on Windows. On any other platform
/// there is no print backend, so it reports [`EngineError::DependencyMissing`].
/// `timeout` bounds the wait on the spooler child; `None` waits forever.
pub fn spool(
    pdf: &Path,
    printer: &str,
    settings: &JobSettings,
    title: &str,
    timeout: Option<Duration>,
) -> Result<SpoolOutcome, PrintError> {
    #[cfg(unix)]
    {
        cups::spool(pdf, printer, settings, title, timeout)
    }
    #[cfg(windows)]
    {
        sumatra::spool(pdf, printer, settings, title, timeout)
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (pdf, printer, settings, title, timeout);
        Err(PrintError::Engine(EngineError::DependencyMissing(
            "no print backend is available on this platform".to_string(),
        )))
    }
}

/// Wait for `child` for at most `timeout`, killing (and reaping) it when the
/// deadline passes.
///
/// This is the print stage's half of NFR-012 — "a timeout MUST NOT leak
/// processes; orphan children MUST be killed within 5 s" — and the reason the
/// spool path spawns instead of using `Command::output()`: a hung `lp` or
/// SumatraPDF is only one job, but the dispatcher prints one job at a time, so
/// an unbounded wait wedges the whole queue with no way out from the UI.
///
/// `None` disables the bound and waits like a plain `Child::wait`. On timeout
/// the child is killed and waited on (so it cannot linger as a zombie) before
/// [`PrintError::TimedOut`] is returned; a kill that fails is not reported over
/// the timeout itself, which is the fact the caller must act on.
pub fn wait_with_timeout(
    child: &mut Child,
    timeout: Option<Duration>,
) -> Result<ExitStatus, PrintError> {
    use wait_timeout::ChildExt;

    let spawn_failed = |e: std::io::Error| {
        PrintError::Engine(EngineError::PrintFailed(format!(
            "failed to wait for the print process: {e}"
        )))
    };

    let Some(timeout) = timeout else {
        return child.wait().map_err(spawn_failed);
    };

    match child.wait_timeout(timeout).map_err(spawn_failed)? {
        Some(status) => Ok(status),
        None => {
            let _ = child.kill();
            let _ = child.wait();
            Err(PrintError::TimedOut {
                seconds: timeout.as_secs(),
            })
        }
    }
}

/// The settings that actually apply to an input with extension `input_ext`.
///
/// Page ranges are a PDF-only concept: an image is rasterised into a
/// single-page PDF, so forwarding a range like `2-3` to the spooler prints
/// nothing at all — a silent empty job. The UI and the docs already promise
/// this ("Applies to PDFs only — images ignore the range",
/// `docs/05-features/print-job.md` §Settings panel), so the engine drops the
/// range rather than the doc being wrong. The job keeps the range the user
/// typed; only what reaches the spooler is narrowed.
pub fn effective_settings<'a>(settings: &'a JobSettings, input_ext: &str) -> Cow<'a, JobSettings> {
    if input_ext.eq_ignore_ascii_case("pdf") || settings.page_ranges.is_none() {
        return Cow::Borrowed(settings);
    }
    Cow::Owned(JobSettings {
        page_ranges: None,
        ..settings.clone()
    })
}

/// The exact command MaeclyPrint will run, as a copy-pasteable string for the
/// UI. It uses the same argument builders as [`spool`], so what the user sees is
/// what actually runs — on Windows the SumatraPDF command, elsewhere the CUPS
/// `lp` command.
pub fn preview_command(pdf_display: &str, printer: &str, settings: &JobSettings) -> String {
    #[cfg(windows)]
    {
        format!(
            "SumatraPDF.exe -print-to \"{printer}\" -print-settings \"{}\" -silent -exit-when-done \"{pdf_display}\"",
            sumatra::print_settings_string(settings)
        )
    }
    #[cfg(not(windows))]
    {
        let opts = cups::lp_opts(settings).join(" ");
        format!("lp -d \"{printer}\" {opts} \"{pdf_display}\"")
    }
}

/// Pure capability gate: reject settings that would *silently* degrade output.
///
/// Per the honesty policy (`docs/03-requirements/print-settings.md` §160-168)
/// we hard-fail only where the failure is invisible to the user. Duplex is that
/// case: a driver that cannot duplex silently prints single-sided, discovered
/// only at the tray — so a duplex request against a non-duplex printer is an
/// error. Colour is deliberately *not* gated: a colour job on a mono printer
/// prints in grey, which is visible and expected, so `color_supported` is
/// accepted here without complaint.
///
/// Capabilities are passed in (the dispatcher reads them from printer
/// enumeration) to keep this function pure and trivially testable.
pub fn check_supported(
    settings: &JobSettings,
    printer: &str,
    duplex_supported: bool,
    color_supported: bool,
) -> Result<(), EngineError> {
    // Intentionally unused: colour degradation is visible, so it never blocks.
    let _ = color_supported;

    if matches!(settings.duplex, Duplex::Long | Duplex::Short) && !duplex_supported {
        return Err(EngineError::UnsupportedSetting {
            setting: "duplex".to_string(),
            printer: printer.to_string(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::queue::job::{Duplex, JobSettings};

    #[test]
    fn duplex_requires_driver_support() {
        let settings = JobSettings {
            duplex: Duplex::Long,
            ..JobSettings::default()
        };
        assert!(check_supported(&settings, "P", false, true).is_err());
        assert!(check_supported(&settings, "P", true, true).is_ok());
    }

    #[test]
    fn no_duplex_request_never_fails() {
        let settings = JobSettings::default(); // Duplex::None
        assert!(check_supported(&settings, "P", false, false).is_ok());
    }

    #[test]
    fn color_on_mono_printer_is_allowed() {
        // Colour job, printer reports no colour support: prints grey, not an error.
        let settings = JobSettings::default(); // ColorMode::Color
        assert!(check_supported(&settings, "P", true, false).is_ok());
    }

    #[test]
    fn unsupported_setting_names_duplex_and_printer() {
        let settings = JobSettings {
            duplex: Duplex::Short,
            ..JobSettings::default()
        };
        match check_supported(&settings, "Front Desk", false, true) {
            Err(EngineError::UnsupportedSetting { setting, printer }) => {
                assert_eq!(setting, "duplex");
                assert_eq!(printer, "Front Desk");
            }
            other => panic!("expected UnsupportedSetting, got {other:?}"),
        }
    }

    #[test]
    fn fr_040_page_range_is_dropped_for_image_inputs() {
        // The UI promises "images ignore the range"; an image becomes a
        // one-page PDF, so a range that excludes page 1 would spool nothing.
        let settings = JobSettings {
            page_ranges: Some("2-3".to_string()),
            ..JobSettings::default()
        };

        let pdf = effective_settings(&settings, "pdf");
        assert_eq!(pdf.page_ranges.as_deref(), Some("2-3"), "PDFs keep it");

        for ext in ["png", "JPG", "tiff", "webp", ""] {
            let img = effective_settings(&settings, ext);
            assert!(img.page_ranges.is_none(), "{ext} must drop the range");
            // Nothing else is narrowed.
            assert_eq!(img.copies, settings.copies);
            assert_eq!(img.paper_size, settings.paper_size);
        }

        // An image job without a range borrows the original — no needless clone.
        let plain = JobSettings::default();
        assert!(matches!(
            effective_settings(&plain, "png"),
            std::borrow::Cow::Borrowed(_)
        ));
    }

    #[test]
    fn fr_040_image_lp_args_carry_no_page_ranges() {
        // End-to-end through the flag builder: what reaches `lp` for an image
        // must not contain `page-ranges`.
        let settings = JobSettings {
            page_ranges: Some("2-3".to_string()),
            ..JobSettings::default()
        };
        let opts = cups::lp_opts(&effective_settings(&settings, "png"));
        assert!(
            !opts.iter().any(|o| o.starts_with("page-ranges=")),
            "image job must not be page-ranged: {opts:?}"
        );
        let opts = cups::lp_opts(&effective_settings(&settings, "pdf"));
        assert!(opts.iter().any(|o| o == "page-ranges=2-3"));
    }

    #[test]
    fn preview_command_shows_printer_and_pdf() {
        let settings = JobSettings::default();
        let cmd = preview_command("/tmp/job.pdf", "Office", &settings);
        assert!(
            cmd.contains("/tmp/job.pdf"),
            "preview names the file: {cmd}"
        );
        assert!(cmd.contains("Office"), "preview names the printer: {cmd}");
    }
}
