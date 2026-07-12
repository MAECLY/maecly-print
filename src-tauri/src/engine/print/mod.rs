//! Printer dispatch facade.
//!
//! One printable PDF plus a [`JobSettings`] becomes a real spooler invocation
//! here. The per-backend flag translation lives in [`cups`] (CUPS `lp`, Unix)
//! and [`sumatra`] (SumatraPDF, Windows); this module picks the backend for the
//! current platform and also exposes:
//!
//! - [`preview_command`] — the exact command string shown in the UI's "this is
//!   what we'll run" panel (honesty policy: no hidden flags), and
//! - [`check_supported`] — the pure capability gate the dispatcher calls before
//!   spooling.

use std::path::Path;

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

/// Spool `pdf` to `printer` with `settings`, tagging the job with `title`.
///
/// Dispatches to CUPS on Unix and SumatraPDF on Windows. On any other platform
/// there is no print backend, so it reports [`EngineError::DependencyMissing`].
pub fn spool(
    pdf: &Path,
    printer: &str,
    settings: &JobSettings,
    title: &str,
) -> Result<SpoolOutcome, EngineError> {
    #[cfg(unix)]
    {
        cups::spool(pdf, printer, settings, title)
    }
    #[cfg(windows)]
    {
        sumatra::spool(pdf, printer, settings, title)
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (pdf, printer, settings, title);
        Err(EngineError::DependencyMissing(
            "no print backend is available on this platform".to_string(),
        ))
    }
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
