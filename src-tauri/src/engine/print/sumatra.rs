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
//! | paper                 | `paper=<A2…A6\|letter\|legal\|tabloid\|statement>` |
//! | tray                  | `bin=<bin name or number>`                 |
//! | copies > 1            | `<n>x` (e.g. `3x`)                         |
//! | orientation land/port | `landscape` / `portrait` *(auto omitted)*  |
//! | **quality**           | **not expressible — see below**            |
//!
//! The doc table lists only `duplex`/`duplexshort`; SumatraPDF also accepts
//! `simplex` and `noscale`, which we use for the "none" cases so every setting
//! is stated explicitly. Copies use `<n>x`, the only spelling SumatraPDF's
//! parser accepts; an unknown token such as `copies=N` is skipped without a
//! warning, which is how an earlier version printed one copy of everything.
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

/// The sizes SumatraPDF's `paper=` understands (its own vocabulary, matched
/// case-insensitively). Anything else cannot be expressed on its command line.
const SUMATRA_PAPERS: [&str; 9] = [
    "A2",
    "A3",
    "A4",
    "A5",
    "A6",
    "letter",
    "legal",
    "tabloid",
    "statement",
];

#[cfg(windows)]
use std::process::Command;

/// Whether `-print-settings` names a paper size.
///
/// A job always carries one ([`JobSettings::paper_size`] is not optional), so
/// [`PaperToken::FromSettings`] is the normal case. [`PaperToken::Omit`] is for
/// callers with no paper preference at all — the test page with no per-printer
/// paper override — where any `paper=` would override the driver's own default
/// (an A4 page forced onto a Letter-only printer stops it with "load A4").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaperToken {
    FromSettings,
    Omit,
}

/// Build SumatraPDF's `-print-settings` value from `settings`.
pub fn print_settings_string(settings: &JobSettings) -> String {
    print_settings_string_with(settings, PaperToken::FromSettings)
}

/// [`print_settings_string`] with an explicit choice about `paper=`.
pub fn print_settings_string_with(settings: &JobSettings, paper_choice: PaperToken) -> String {
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

    // Paper — only a size SumatraPDF can name; `auto` and any other size send
    // nothing (the printer's own setting applies), and
    // [`unsupported_notes`] says so for the latter. A caller with no paper
    // preference at all ([`PaperToken::Omit`]) sends none either.
    if paper_choice == PaperToken::FromSettings {
        if let Some(paper) = paper_token(&settings.paper_size) {
            parts.push(format!("paper={paper}"));
        }
    }

    // Tray: SumatraPDF takes the driver's own bin name or number. Only emitted
    // when the job names one — otherwise the printer's default bin wins.
    if let Some(tray) = bin_token(settings) {
        parts.push(format!("bin={tray}"));
    }

    // SumatraPDF spells copies `<n>x` (`3x`). It silently skips tokens it does
    // not know, so the `copies=N` spelling some docs use prints one copy.
    if settings.copies > 1 {
        parts.push(format!("{}x", settings.copies));
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
    sumatra_args_with(printer, settings, pdf, PaperToken::FromSettings)
}

/// [`sumatra_args`] with an explicit choice about `paper=`.
pub fn sumatra_args_with(
    printer: &str,
    settings: &JobSettings,
    pdf: &Path,
    paper_choice: PaperToken,
) -> Vec<String> {
    vec![
        "-print-to".to_string(),
        printer.to_string(),
        "-print-settings".to_string(),
        print_settings_string_with(settings, paper_choice),
        "-silent".to_string(),
        "-exit-when-done".to_string(),
        pdf.to_string_lossy().into_owned(),
    ]
}

/// Fill in the per-printer default tray (Printers → *Defaults for this
/// printer*) when the job itself names none.
///
/// `docs/05-features/printers-management.md` says those defaults "seed any
/// profile that targets this printer without an explicit override". A job's
/// own tray always wins; a blank override is no override. Pure, so the rule is
/// tested on every host; [`resolve_for_printer`] feeds it the saved value.
pub fn apply_printer_tray<'a>(
    settings: &'a JobSettings,
    printer_tray: Option<&str>,
) -> std::borrow::Cow<'a, JobSettings> {
    use std::borrow::Cow;
    let fallback = printer_tray.map(str::trim).filter(|t| !t.is_empty());
    match (settings.selected_tray(), fallback) {
        (None, Some(tray)) => Cow::Owned(JobSettings {
            tray: Some(tray.to_string()),
            ..settings.clone()
        }),
        _ => Cow::Borrowed(settings),
    }
}

/// Turn a driver paper name SumatraPDF has no keyword for into the canonical
/// size it stands for, using `driver_paper` (the driver's own name → id →
/// size lookup on Windows).
///
/// Per-printer paper overrides and the sizes the Print Job screen offers are
/// the *driver's* names, which are localized ("Carta" for Letter on Spanish
/// Windows) or decorated ("A4 210 x 297 mm"). SumatraPDF's `paper=` only knows
/// its own keywords, so without this such a job printed on the driver's
/// default sheet with nothing but a log line to say so. A size SumatraPDF
/// already names, `auto`, or a name the driver does not resolve is left as is.
pub fn resolve_paper<'a>(
    settings: std::borrow::Cow<'a, JobSettings>,
    driver_paper: impl Fn(&str) -> Option<PaperSize>,
) -> std::borrow::Cow<'a, JobSettings> {
    if settings.paper_size.is_auto() || paper_token(&settings.paper_size).is_some() {
        return settings;
    }
    match driver_paper(settings.paper_size.as_str()).filter(|p| paper_token(p).is_some()) {
        Some(paper) => {
            let mut owned = settings.into_owned();
            owned.paper_size = paper;
            std::borrow::Cow::Owned(owned)
        }
        None => settings,
    }
}

/// The settings SumatraPDF is actually given for `printer`: the job's own,
/// plus the printer's saved default tray when the job names none, with a
/// driver paper name mapped to SumatraPDF's keyword ([`resolve_paper`]). Used
/// by both [`spool`] and the honesty preview, so the preview shows the same
/// `bin=` and `paper=`.
pub fn resolve_for_printer<'a>(
    settings: &'a JobSettings,
    printer: &str,
) -> std::borrow::Cow<'a, JobSettings> {
    let with_tray = if settings.selected_tray().is_some() {
        std::borrow::Cow::Borrowed(settings)
    } else {
        let prefs = crate::settings::Config::load_or_default().printers;
        let tray = prefs.overrides.get(printer).and_then(|o| o.tray.clone());
        apply_printer_tray(settings, tray.as_deref())
    };
    #[cfg(windows)]
    let driver_paper = |name: &str| crate::platform::paper_size_for_driver_name(printer, name);
    #[cfg(not(windows))]
    let driver_paper = |_: &str| None;
    resolve_paper(with_tray, driver_paper)
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
/// Written as a `#` comment so the honesty preview (a PowerShell line) stays
/// copy-pasteable.
pub fn quality_note(settings: &JobSettings) -> Option<&'static str> {
    match settings.quality {
        Quality::Normal => None,
        Quality::Draft => Some(
            "# note: SumatraPDF has no print-quality switch, so \"draft\" is NOT applied — the printer's own default quality is used.",
        ),
        Quality::High => Some(
            "# note: SumatraPDF has no print-quality switch, so \"high\" is NOT applied — the printer's own default quality is used.",
        ),
    }
}

/// SumatraPDF's own spelling of `paper`, or `None` for `auto` or a size it has
/// no keyword for. The classic four keep their established spelling
/// (`A4`, `Letter`, `Legal`, `A3`).
fn paper_token(paper: &PaperSize) -> Option<&str> {
    if paper.is_auto() {
        return None;
    }
    if PaperSize::CLASSIC.contains(paper) {
        return Some(paper.as_str());
    }
    SUMATRA_PAPERS
        .iter()
        .find(|p| p.eq_ignore_ascii_case(paper.as_str()))
        .copied()
}

/// Every setting the SumatraPDF command line cannot carry, as `#` comment
/// lines for the honesty preview (and the job log): quality, and a paper size
/// outside SumatraPDF's vocabulary.
pub fn unsupported_notes(settings: &JobSettings) -> Vec<String> {
    let mut notes: Vec<String> = quality_note(settings)
        .map(str::to_string)
        .into_iter()
        .collect();
    if settings.copies > 1 {
        notes.push(format!(
            "# note: SumatraPDF has no collate switch, so \"{}\" is NOT applied — the driver's own collation is used for the {} copies.",
            if settings.collate { "collated" } else { "uncollated" },
            settings.copies
        ));
    }
    if !settings.paper_size.is_auto() && paper_token(&settings.paper_size).is_none() {
        notes.push(format!(
            "# note: SumatraPDF has no keyword for the paper size \"{}\", so it is NOT applied — the printer's own paper setting is used.",
            settings.paper_size
        ));
    }
    notes
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

/// The file name a job is spooled under, from its `title` (the original
/// file's name). Pure.
///
/// SumatraPDF has no job-title flag and names the spooler document after the
/// file it prints, so the PDF is handed to it under this name
/// ([`stage_for_spool`]): the Windows print queue then shows `report.pdf` for
/// `report.docx` instead of a cache UUID. It is the CUPS backend's name
/// ([`super::cups::document_file_name`], so both platforms title a job the
/// same way) with the characters Windows forbids in a file name replaced by
/// `_` and no trailing dot or space before the extension.
pub fn spool_file_name(title: &str) -> String {
    // Replaced first: on Windows `a:b.png` parses as drive `a:` plus `b.png`,
    // and the name would lose its first part.
    let safe: String = title
        .chars()
        .map(|c| {
            if matches!(c, '<' | '>' | ':' | '"' | '|' | '?' | '*') {
                '_'
            } else {
                c
            }
        })
        .collect();
    let name = super::cups::document_file_name(&safe);
    let stem = name.strip_suffix(".pdf").unwrap_or(&name);
    let clean = stem.trim_end_matches(['.', ' ']);
    if clean.is_empty() {
        "document.pdf".to_string()
    } else {
        format!("{clean}.pdf")
    }
}

/// A private folder holding `pdf` under [`spool_file_name`]`(title)`: a hard
/// link when the volume allows it, a copy otherwise. The folder (and the
/// link) disappears when the returned guard drops, after SumatraPDF has
/// handed the job to the spooler, which keeps its own copy of the data.
pub fn stage_for_spool(pdf: &Path, title: &str) -> std::io::Result<(tempfile::TempDir, PathBuf)> {
    let dir = tempfile::Builder::new()
        .prefix("maeclyprint-spool-")
        .tempdir()?;
    let staged = dir.path().join(spool_file_name(title));
    if std::fs::hard_link(pdf, &staged).is_err() {
        std::fs::copy(pdf, &staged)?;
    }
    Ok((dir, staged))
}

/// At most this many bytes of SumatraPDF's output are quoted in an error.
#[cfg_attr(not(windows), allow(dead_code))]
const LOG_TAIL: usize = 600;

/// The last [`LOG_TAIL`] bytes of SumatraPDF's own output, for an error
/// message. Empty when there is none.
#[cfg_attr(not(windows), allow(dead_code))]
fn log_tail(log: &Path) -> String {
    let Ok(bytes) = std::fs::read(log) else {
        return String::new();
    };
    let start = bytes.len().saturating_sub(LOG_TAIL);
    String::from_utf8_lossy(&bytes[start..]).trim().to_string()
}

/// Spool `pdf` via SumatraPDF, hiding the child's console window. SumatraPDF
/// reports no OS job id itself, so the spooler's job list is compared before
/// and after ([`super::windows::find_spooled_job`]); `os_job_id` is `None` when
/// either list could not be read, or no new job of this user carries the
/// spooled file's name (it already left, or it cannot be told apart from
/// another).
///
/// SumatraPDF has no job-title flag, so the job is titled by printing the PDF
/// under `title`'s name ([`stage_for_spool`]). Its diagnostic output (it logs
/// its whole start-up to an inherited stdout) goes to a file next to that
/// copy, never to our own stdout, where it would corrupt `print --json`; the
/// tail of it is quoted when it fails.
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
///
/// The printer's saved default tray is applied when the job names none
/// ([`resolve_for_printer`]).
#[cfg(windows)]
pub fn spool(
    pdf: &Path,
    printer: &str,
    settings: &JobSettings,
    title: &str,
    timeout: Option<std::time::Duration>,
) -> Result<super::SpoolOutcome, super::PrintError> {
    let settings = resolve_for_printer(settings, printer);
    spool_with(
        pdf,
        printer,
        &settings,
        title,
        timeout,
        PaperToken::FromSettings,
    )
}

/// [`spool`] with `settings` taken as final and an explicit `paper=` choice.
/// For callers that resolved the printer defaults themselves (the test page).
#[cfg(windows)]
pub fn spool_with(
    pdf: &Path,
    printer: &str,
    settings: &JobSettings,
    title: &str,
    timeout: Option<std::time::Duration>,
    paper_choice: PaperToken,
) -> Result<super::SpoolOutcome, super::PrintError> {
    use std::os::windows::process::CommandExt;

    // Suppress the transient console window when spawning the child.
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    for note in unsupported_notes(settings) {
        tracing::warn!(
            event = "print_setting_not_applied",
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
    let (stage, staged) = stage_for_spool(pdf, title).map_err(|e| {
        EngineError::PrintFailed(format!("cannot prepare the file for the print queue: {e}"))
    })?;
    let log_path = stage.path().join("sumatrapdf.log");
    let (log_out, log_err) = std::fs::File::create(&log_path)
        .and_then(|f| Ok((f.try_clone()?, f)))
        .map_err(|e| EngineError::PrintFailed(format!("cannot create the print log: {e}")))?;
    let args = sumatra_args_with(printer, settings, &staged, paper_choice);
    // SumatraPDF reports no job id, so take the spooler's job list before and
    // after: the new job carrying this file's path is the one to track. If the
    // "before" list cannot be read there is nothing to compare against, so the
    // job is not tracked at all rather than guessed.
    let before = match super::windows::job_ids(printer) {
        Ok(ids) => Some(ids),
        Err(e) => {
            tracing::warn!(
                event = "os_job_untracked",
                printer = %printer,
                reason = "the job list could not be read before spooling",
                error = %e,
            );
            None
        }
    };
    let mut child = Command::new(exe)
        .args(&args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::from(log_out))
        .stderr(std::process::Stdio::from(log_err))
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map_err(|e| EngineError::PrintFailed(format!("failed to run SumatraPDF: {e}")))?;

    let status = super::wait_with_timeout(&mut child, timeout)?;

    if status.success() {
        let document = staged.to_string_lossy();
        Ok(super::SpoolOutcome {
            os_job_id: before
                .as_deref()
                .and_then(|before| super::windows::find_spooled_job(printer, before, &document))
                .map(|id| id.to_string()),
        })
    } else {
        let tail = log_tail(&log_path);
        let detail = if tail.is_empty() {
            String::new()
        } else {
            format!(": {tail}")
        };
        Err(EngineError::PrintFailed(format!("SumatraPDF exited with {status}{detail}")).into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn strs(v: &[String]) -> Vec<&str> {
        v.iter().map(String::as_str).collect()
    }

    /// The Windows queue shows the user's file name, as CUPS does, never the
    /// cache path or a UUID, and the name is always a legal Windows file name.
    #[test]
    fn fr_035_the_spooled_document_is_named_after_the_users_file() {
        assert_eq!(spool_file_name("report.pdf"), "report.pdf");
        assert_eq!(spool_file_name("letter.docx"), "letter.pdf");
        assert_eq!(spool_file_name("Factura 03.xlsx"), "Factura 03.pdf");
        assert_eq!(spool_file_name("a:b*c?.png"), "a_b_c_.pdf");
        assert_eq!(spool_file_name("  "), "document.pdf");
        assert_eq!(spool_file_name("dots...txt"), "dots.pdf");
        let long = "x".repeat(300) + ".docx";
        let name = spool_file_name(&long);
        assert!(name.len() <= 210 && name.ends_with(".pdf"), "{name}");
    }

    #[test]
    fn fr_035_staging_links_the_pdf_under_its_title_and_cleans_up() {
        let dir = tempfile::tempdir().unwrap();
        let pdf = dir.path().join("0192-uuid.pdf");
        std::fs::write(&pdf, b"%PDF-1.4\n%%EOF\n").unwrap();
        let (guard, staged) = stage_for_spool(&pdf, "Quarterly report.docx").unwrap();
        assert_eq!(staged.file_name().unwrap(), "Quarterly report.pdf");
        assert_eq!(
            std::fs::read(&staged).unwrap(),
            std::fs::read(&pdf).unwrap()
        );
        let folder = guard.path().to_path_buf();
        drop(guard);
        assert!(!folder.exists());
        assert!(pdf.exists(), "the source file is never touched (NFR-011)");
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
            collate: true,
        };
        assert_eq!(
            print_settings_string(&settings),
            "1-5,10,duplex,monochrome,noscale,paper=A3,bin=Drawer 1,2x,landscape"
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
            assert!(note.starts_with("# "), "stays copy-pasteable: {note}");
            assert!(note.contains("NOT applied"), "{note}");
        }
    }

    /// SumatraPDF names a fixed set of sizes; anything else is stated as not
    /// applied rather than mangled into a token, and `auto` says nothing.
    #[test]
    fn paper_outside_sumatras_vocabulary_is_declared_not_applied() {
        let with = |name: &str| JobSettings {
            paper_size: PaperSize::parse(name).unwrap(),
            ..JobSettings::default()
        };
        assert!(print_settings_string(&with("Tabloid")).contains("paper=tabloid"));
        assert!(print_settings_string(&with("a5")).contains("paper=A5"));
        assert!(unsupported_notes(&with("A5")).is_empty());

        let photo = with("4x6");
        assert!(!print_settings_string(&photo).contains("paper="));
        let notes = unsupported_notes(&photo);
        assert_eq!(notes.len(), 1);
        assert!(notes[0].contains("4x6") && notes[0].contains("NOT applied"));

        let auto = with("auto");
        assert!(!print_settings_string(&auto).contains("paper="));
        assert!(unsupported_notes(&auto).is_empty());
    }

    #[test]
    fn collation_is_declared_not_applied_for_several_copies() {
        let with = |copies, collate| JobSettings {
            copies,
            collate,
            ..JobSettings::default()
        };
        assert!(unsupported_notes(&with(1, false)).is_empty());
        let notes = unsupported_notes(&with(3, false));
        assert!(notes
            .iter()
            .any(|n| n.contains("uncollated") && n.contains("NOT applied")));
        assert!(!print_settings_string(&with(3, false)).contains("collate"));
    }

    /// FR-041: SumatraPDF's `-print-settings` only understands copies as
    /// `<n>x`; it skips an unknown `copies=N` token without a word, which
    /// printed exactly one copy whatever the user asked for.
    #[test]
    fn fr_041_copies_use_sumatras_nx_token() {
        let with = |copies| JobSettings {
            copies,
            ..JobSettings::default()
        };
        assert_eq!(
            print_settings_string(&with(3)),
            "simplex,color,fit,paper=A4,3x"
        );
        assert!(!print_settings_string(&with(3)).contains("copies"));
        // One copy is the default and is not stated at all.
        assert_eq!(
            print_settings_string(&with(1)),
            "simplex,color,fit,paper=A4"
        );
    }

    /// The printer's saved default tray reaches `bin=` when the job names no
    /// tray, and never overrides a tray the job did name.
    #[test]
    fn fr_041_printer_default_tray_fills_in_only_when_the_job_has_none() {
        let none = JobSettings::default();
        let filled = apply_printer_tray(&none, Some("Manual Feed"));
        assert_eq!(filled.tray.as_deref(), Some("Manual Feed"));
        assert!(print_settings_string(&filled).contains("bin=Manual Feed"));

        let own = JobSettings {
            tray: Some("Tray 2".into()),
            ..JobSettings::default()
        };
        assert_eq!(
            apply_printer_tray(&own, Some("Manual Feed"))
                .tray
                .as_deref(),
            Some("Tray 2")
        );
        // Blank and absent overrides change nothing.
        assert!(apply_printer_tray(&none, Some("  ")).tray.is_none());
        assert!(apply_printer_tray(&none, None).tray.is_none());
    }

    /// With no paper preference at all the token is left out, so the driver's
    /// own default paper is used instead of a forced A4.
    #[test]
    fn paper_token_can_be_omitted() {
        let s = print_settings_string_with(&JobSettings::default(), PaperToken::Omit);
        assert_eq!(s, "simplex,color,fit");
        let args = sumatra_args_with(
            "P",
            &JobSettings::default(),
            Path::new("C:/t.pdf"),
            PaperToken::Omit,
        );
        assert!(!args.iter().any(|a| a.contains("paper=")));
    }

    #[test]
    fn short_edge_portrait_shrink() {
        let settings = JobSettings {
            duplex: Duplex::Short,
            orientation: Orientation::Portrait,
            scaling: Scaling::Shrink,
            paper_size: PaperSize::LETTER,
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

    /// FR-041: a driver paper name — a per-printer override "Carta", or a
    /// size picked from the printer's own list — reaches SumatraPDF as the
    /// keyword for the same sheet instead of being dropped.
    #[test]
    fn fr_041_driver_paper_names_reach_sumatra_as_its_keyword() {
        use std::borrow::Cow;
        let driver = |name: &str| match name {
            "Carta" => Some(PaperSize::LETTER),
            "A4 210 x 297 mm" => Some(PaperSize::A4),
            "Sobre DL" => None,
            _ => None,
        };
        let job = |paper: &str| JobSettings {
            paper_size: PaperSize::parse(paper).unwrap(),
            ..JobSettings::default()
        };

        let carta = job("Carta");
        let resolved = resolve_paper(Cow::Borrowed(&carta), driver);
        assert_eq!(resolved.paper_size, PaperSize::LETTER);
        assert!(print_settings_string(&resolved).contains("paper=Letter"));
        assert!(unsupported_notes(&resolved).is_empty());

        let decorated = job("A4 210 x 297 mm");
        let resolved = resolve_paper(Cow::Borrowed(&decorated), driver);
        assert!(print_settings_string(&resolved).contains("paper=A4"));

        // Not resolvable: left alone, and the note still says so.
        let envelope = job("Sobre DL");
        let resolved = resolve_paper(Cow::Borrowed(&envelope), driver);
        assert!(matches!(resolved, Cow::Borrowed(_)));
        assert!(!print_settings_string(&resolved).contains("paper="));
        assert_eq!(unsupported_notes(&resolved).len(), 1);

        // Already a SumatraPDF keyword, or `auto`: the driver is not asked.
        let never = |_: &str| -> Option<PaperSize> { panic!("must not be asked") };
        let a4 = job("A4");
        assert!(matches!(
            resolve_paper(Cow::Borrowed(&a4), never),
            Cow::Borrowed(_)
        ));
        let auto = job("auto");
        assert!(matches!(
            resolve_paper(Cow::Borrowed(&auto), never),
            Cow::Borrowed(_)
        ));
    }
}
