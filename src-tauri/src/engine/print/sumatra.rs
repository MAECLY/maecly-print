//! SumatraPDF backend: `-print-settings` string construction (pure, all
//! platforms), locating the executable via [`crate::engine::deps`], and, on
//! Windows, spooling.
//!
//! Flag mapping (`docs/03-requirements/print-settings.md` §SumatraPDF table).
//! The `-print-settings` value is a comma-separated keyword list; SumatraPDF is
//! order-independent, but we emit a fixed order for a stable, testable preview.
//!
//! | Setting               | Token(s)                                   |
//! |-----------------------|--------------------------------------------|
//! | page ranges           | bare `1-5,10` (prepended, no key)          |
//! | duplex none/long/short| `simplex` / `duplex` / `duplexshort`       |
//! | colour / monochrome   | `color` / `monochrome`                     |
//! | scaling fit/shrink/none| `fit` / `shrink` / `noscale`              |
//! | paper                 | `paper=A4\|Letter\|Legal\|A3`              |
//! | tray                  | `bin=<bin name or number>`                 |
//! | copies > 1            | `copies=<n>`                               |
//! | orientation land/port | `landscape` / `portrait` *(auto omitted)*  |
//! | **quality**           | **not expressible — see below**            |
//!
//! The doc table lists only `duplex`/`duplexshort`; SumatraPDF also accepts
//! `simplex` and `noscale`, which we use for the "none" cases so every setting
//! is stated explicitly. Copies use the doc's `copies=N` (not the older `Nx`).
//!
//! ## The quality gap
//!
//! SumatraPDF's `-print-settings` has no print-quality switch: its vocabulary
//! covers pages, orientation, scaling, colour, duplex, copies, `paper=` and
//! `bin=`, and nothing else. A draft/normal/high choice therefore **cannot** be
//! carried to the Windows spooler by this backend, and the printer prints at
//! whatever quality its own driver defaults to.
//!
//! We do not paper over that. There is no plausible fake for it either —
//! `resolution=NxM` exists, but MaeclyPrint does not know the device's DPI
//! ladder, so mapping "high" to an invented number would be a guess dressed up
//! as a setting. Instead [`quality_note`] states the limitation in the honesty
//! preview the user reads before printing, and [`spool`] logs it against the
//! job, so nobody is left believing a quality preset was applied.

use std::path::{Path, PathBuf};

use crate::engine::{deps, EngineError};
use crate::queue::job::{ColorMode, Duplex, JobSettings, Orientation, PaperSize, Quality, Scaling};

#[cfg(windows)]
use std::process::Command;

/// Build SumatraPDF's `-print-settings` value from `settings`.
pub fn print_settings_string(settings: &JobSettings) -> String {
    let mut parts: Vec<String> = Vec::new();

    // Page ranges first, as bare comma-separated tokens (SumatraPDF takes them
    // with no key). Whitespace-stripped so the join stays clean.
    if let Some(ranges) = &settings.page_ranges {
        let compact = strip_ws(ranges);
        if !compact.is_empty() {
            parts.push(compact);
        }
    }

    parts.push(
        match settings.duplex {
            Duplex::None => "simplex",
            Duplex::Long => "duplex",
            Duplex::Short => "duplexshort",
        }
        .to_string(),
    );

    parts.push(
        match settings.color_mode {
            ColorMode::Color => "color",
            ColorMode::Monochrome => "monochrome",
        }
        .to_string(),
    );

    parts.push(
        match settings.scaling {
            Scaling::Fit => "fit",
            Scaling::Shrink => "shrink",
            Scaling::None => "noscale",
        }
        .to_string(),
    );

    parts.push(format!("paper={}", paper(settings.paper_size)));

    // Tray: SumatraPDF takes the driver's own bin name or number. Only emitted
    // when the job names one — otherwise the printer's default bin wins.
    if let Some(tray) = bin_token(settings) {
        parts.push(format!("bin={tray}"));
    }

    if settings.copies > 1 {
        parts.push(format!("copies={}", settings.copies));
    }

    match settings.orientation {
        Orientation::Landscape => parts.push("landscape".to_string()),
        Orientation::Portrait => parts.push("portrait".to_string()),
        Orientation::Auto => {}
    }

    parts.join(",")
}

/// Full SumatraPDF argument vector:
/// `-print-to <printer> -print-settings <string> -silent -exit-when-done <pdf>`.
pub fn sumatra_args(printer: &str, settings: &JobSettings, pdf: &Path) -> Vec<String> {
    vec![
        "-print-to".to_string(),
        printer.to_string(),
        "-print-settings".to_string(),
        print_settings_string(settings),
        "-silent".to_string(),
        "-exit-when-done".to_string(),
        pdf.to_string_lossy().into_owned(),
    ]
}

/// The bin name to put in `bin=`, or `None` when there is nothing safe to say.
///
/// `-print-settings` is a **comma-separated** list, so a bin name containing a
/// comma cannot be expressed in it at all: emitting it would split into two
/// garbage tokens and SumatraPDF would print from some other tray. Such a name
/// is dropped (the printer's default bin is used) rather than mangled, and
/// [`spool`] logs the drop — the same rule as the quality gap: never claim a
/// setting we did not send.
fn bin_token(settings: &JobSettings) -> Option<&str> {
    settings.selected_tray().filter(|t| !t.contains(','))
}

/// The sentence to show the user when their quality preset cannot be honoured
/// on this backend, or `None` when there is nothing to warn about.
///
/// [`Quality::Normal`] returns `None` deliberately: "normal" is what an
/// unconfigured driver does anyway, so warning about it would be noise the user
/// learns to ignore. Anything else is a deliberate choice that will not happen,
/// and saying so is the whole point.
///
/// Written as a `::` batch comment so the honesty preview stays a single
/// copy-pasteable command line.
pub fn quality_note(settings: &JobSettings) -> Option<&'static str> {
    match settings.quality {
        Quality::Normal => None,
        Quality::Draft => Some(
            ":: note: SumatraPDF has no print-quality switch, so \"draft\" is NOT applied — the printer's own default quality is used.",
        ),
        Quality::High => Some(
            ":: note: SumatraPDF has no print-quality switch, so \"high\" is NOT applied — the printer's own default quality is used.",
        ),
    }
}

fn paper(paper: PaperSize) -> &'static str {
    match paper {
        PaperSize::A4 => "A4",
        PaperSize::Letter => "Letter",
        PaperSize::Legal => "Legal",
        PaperSize::A3 => "A3",
    }
}

fn strip_ws(s: &str) -> String {
    s.split_whitespace().collect()
}

/// Locate `SumatraPDF.exe` through the shared dependency resolver
/// ([`crate::engine::deps`]): a bundled copy next to the app first, then a
/// configured path, then `PATH`, then the usual install directories.
///
/// The failure carries the same remediation text the UI shows for a missing
/// dependency ([`deps::Spec::remediation`]), so the queue's job error and the
/// Print Job banner say the same thing.
///
/// On non-Windows platforms SumatraPDF is not used at all (CUPS handles
/// printing), and the resolver has no executable to look for there, so this is
/// always a `DependencyMissing` — see the `applicable` flag on the report.
pub fn locate() -> Result<PathBuf, EngineError> {
    let os = deps::Os::current();
    let spec = deps::spec(deps::Dependency::SumatraPdf, os);

    if let deps::Resolution::Found { path, .. } =
        deps::resolve_on_host(deps::Dependency::SumatraPdf, os, None)
    {
        return Ok(path);
    }

    Err(EngineError::DependencyMissing(if spec.applicable() {
        spec.remediation()
    } else {
        "SumatraPDF is only used on Windows".to_string()
    }))
}

/// Spool `pdf` via SumatraPDF, hiding the child's console window. SumatraPDF
/// prints synchronously and reports no OS job id, so `os_job_id` is `None`.
/// `title` is accepted for parity with the CUPS backend but SumatraPDF has no
/// job-title flag, so it is not forwarded.
///
/// The wait is bounded by `timeout` and the child killed when it expires (see
/// [`super::wait_with_timeout`]): SumatraPDF blocks until the spooler accepts
/// the job, so a driver dialog or a stalled spooler would otherwise hold the
/// queue's single print slot forever.
///
/// Anything the command line cannot express — a quality preset ([`quality_note`])
/// or a bin name with a comma in it ([`bin_token`]) — is logged against the job
/// before spooling, so the job log records what was *not* sent as well as what
/// was.
#[cfg(windows)]
pub fn spool(
    pdf: &Path,
    printer: &str,
    settings: &JobSettings,
    title: &str,
    timeout: Option<std::time::Duration>,
) -> Result<super::SpoolOutcome, super::PrintError> {
    use std::os::windows::process::CommandExt;

    // Suppress the transient console window when spawning the child.
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let _ = title;

    if let Some(note) = quality_note(settings) {
        tracing::warn!(
            event = "print_setting_not_applied",
            setting = "quality",
            backend = "sumatra",
            printer = %printer,
            message = %note,
        );
    }
    if settings.selected_tray().is_some() && bin_token(settings).is_none() {
        tracing::warn!(
            event = "print_setting_not_applied",
            setting = "tray",
            backend = "sumatra",
            printer = %printer,
            message = "the bin name contains a comma, which -print-settings cannot express",
        );
    }

    let exe = locate()?;
    let args = sumatra_args(printer, settings, pdf);
    let mut child = Command::new(exe)
        .args(&args)
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map_err(|e| EngineError::PrintFailed(format!("failed to run SumatraPDF: {e}")))?;

    let status = super::wait_with_timeout(&mut child, timeout)?;

    if status.success() {
        Ok(super::SpoolOutcome { os_job_id: None })
    } else {
        Err(EngineError::PrintFailed(format!("SumatraPDF exited with {status}")).into())
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
    fn defaults_string_is_explicit() {
        assert_eq!(
            print_settings_string(&JobSettings::default()),
            "simplex,color,fit,paper=A4"
        );
    }

    #[test]
    fn full_string_order_and_tokens() {
        let settings = JobSettings {
            printer: None,
            duplex: Duplex::Long,
            color_mode: ColorMode::Monochrome,
            copies: 2,
            paper_size: PaperSize::A3,
            orientation: Orientation::Landscape,
            scaling: Scaling::None,
            page_ranges: Some("1-5, 10".to_string()),
            quality: Quality::High,
            tray: Some("Drawer 1".to_string()),
        };
        assert_eq!(
            print_settings_string(&settings),
            "1-5,10,duplex,monochrome,noscale,paper=A3,bin=Drawer 1,copies=2,landscape"
        );
    }

    /// FR-041: the tray the profile editor offers must reach SumatraPDF's
    /// `bin=`, and the quality it offers must not be silently swallowed.
    #[test]
    fn fr_041_tray_reaches_bin_and_quality_is_declared_unsupported() {
        let with = |quality, tray: Option<&str>| JobSettings {
            quality,
            tray: tray.map(String::from),
            ..JobSettings::default()
        };

        assert_eq!(
            print_settings_string(&with(Quality::Normal, Some("Tray 2"))),
            "simplex,color,fit,paper=A4,bin=Tray 2"
        );
        // No tray → no token at all, so the printer's default bin is used.
        assert!(!print_settings_string(&with(Quality::Normal, None)).contains("bin="));
        // A blank name is not a bin name.
        assert!(!print_settings_string(&with(Quality::Normal, Some(" "))).contains("bin="));
        // A comma cannot survive the comma-separated list, so it is dropped
        // rather than mangled into a token that selects the wrong tray.
        assert!(
            !print_settings_string(&with(Quality::Normal, Some("Tray 2, Legal"))).contains("bin=")
        );

        // Quality has no token to emit — and is never faked as one.
        for quality in [Quality::Draft, Quality::Normal, Quality::High] {
            let s = print_settings_string(&with(quality, None));
            assert!(!s.contains("quality"), "{quality:?}: {s}");
            assert!(!s.contains("resolution"), "{quality:?}: {s}");
        }

        // …instead it is stated out loud, for every choice that is not the
        // driver's own default behaviour.
        assert!(quality_note(&with(Quality::Normal, None)).is_none());
        for quality in [Quality::Draft, Quality::High] {
            let note = quality_note(&with(quality, None)).expect("non-default quality is declared");
            assert!(note.starts_with("::"), "stays copy-pasteable: {note}");
            assert!(note.contains("NOT applied"), "{note}");
        }
    }

    #[test]
    fn short_edge_portrait_shrink() {
        let settings = JobSettings {
            duplex: Duplex::Short,
            orientation: Orientation::Portrait,
            scaling: Scaling::Shrink,
            paper_size: PaperSize::Letter,
            ..JobSettings::default()
        };
        assert_eq!(
            print_settings_string(&settings),
            "duplexshort,color,shrink,paper=Letter,portrait"
        );
    }

    #[test]
    fn args_wrap_the_settings_string() {
        let args = sumatra_args(
            "HP LaserJet",
            &JobSettings::default(),
            Path::new("C:/x.pdf"),
        );
        assert_eq!(
            strs(&args),
            vec![
                "-print-to",
                "HP LaserJet",
                "-print-settings",
                "simplex,color,fit,paper=A4",
                "-silent",
                "-exit-when-done",
                "C:/x.pdf",
            ]
        );
    }

    #[cfg(not(windows))]
    #[test]
    fn locate_is_dependency_missing_off_windows() {
        assert!(matches!(locate(), Err(EngineError::DependencyMissing(_))));
    }
}
