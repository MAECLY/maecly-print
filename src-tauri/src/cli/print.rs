//! `maeclyprint print …` (FR-008).
//!
//! The headless print hop. It resolves a profile plus the per-invocation
//! overrides into one [`JobSettings`], applies exactly the acceptance rules
//! [`crate::queue::QueueHandle::submit`] applies (existence, size ceiling,
//! supported format, settings and page-range validity), then walks the files
//! one at a time through the same engine the dispatcher uses:
//! [`crate::engine::convert`] → [`crate::engine::print::spool`].
//!
//! Two deliberate differences from the GUI queue, both because a CLI run is a
//! *foreground* operation that ends when the process ends:
//!
//! - **No `queue.json`, no persistence.** A headless run never touches the
//!   queue file, so it cannot race a running app for it (NFR-014). Its jobs are
//!   ephemeral by definition — the process holding them is about to exit.
//! - **Its own conversion cache.** Converted PDFs go to a private temp
//!   directory that is deleted on exit, rather than the shared conversions
//!   cache the GUI sweeps at boot. Neither process can delete the other's work.
//!
//! There is no retry/backoff either: a script wants an exit code, not a job
//! that quietly retries for half a minute.

use std::path::Path;
use std::sync::atomic::AtomicBool;

use serde::Serialize;

use crate::cli::args::{PrintArgs, SettingFlags};
use crate::cli::exit::{CliError, ExitCode};
use crate::cli::output::{table, Emitter};
use crate::cli::{files, handoff, profiles, Paths};
use crate::profiles::{Profile, ProfileDuplex};
use crate::queue::job::{Duplex, JobError, JobSettings};
use crate::util::error::IpcError;

/// What happened to one file.
#[derive(Debug, Clone, Serialize)]
pub struct FileOutcome {
    pub path: String,
    /// Where this file actually went. Per file because a file-type routing rule
    /// can split one batch across printers.
    pub printer: String,
    /// `done` or `failed` — the same vocabulary as [`crate::queue::job::JobState`].
    pub state: String,
    /// The spooler's own job id, when the backend reports one (CUPS does).
    pub os_job_id: Option<String>,
    pub error: Option<JobError>,
}

/// The document `print --json` emits: one batch, one resolved settings bundle,
/// one row per file.
#[derive(Debug, Clone, Serialize)]
pub struct PrintReport {
    pub batch_id: String,
    /// The batch target: the shared printer, or `<per file type>` when routing
    /// rules split the batch. The authoritative per-file value lives in
    /// [`FileOutcome::printer`].
    pub printer: String,
    pub profile_name: Option<String>,
    pub settings: JobSettings,
    pub files: Vec<FileOutcome>,
    /// The exit code the process will return (the worst among the files).
    pub exit_code: i32,
}

/// Run `print`, returning the exit code for the batch.
///
/// Anything that invalidates the *whole* invocation (a missing file, an
/// unusable setting, no printer) is an `Err` and nothing is printed at all —
/// the same all-or-nothing acceptance the GUI has. Once the batch is accepted,
/// each file's outcome is recorded and the worst code wins.
pub fn run(paths: &Paths, emitter: Emitter, args: PrintArgs) -> Result<ExitCode, CliError> {
    // Engine limits come from the same (sanitized) config the queue reads at
    // boot, so a headless run accepts exactly the files the app would.
    let engine = crate::settings::Config::load_from(&paths.config)
        .engine
        .sanitized();

    let inputs = collect_inputs(&args)?;
    let expanded = files::expand_inputs(&inputs, engine.max_file_size_mb)?;

    let profile = match &args.profile {
        Some(name_or_id) => profiles::resolve(paths, name_or_id)?,
        None => profiles::default_profile(paths)?,
    };
    let settings = resolve_settings(&profile, args.printer.clone(), &args.settings)?;

    let files = accept_files(&expanded, engine.max_file_size_mb)?;
    check_ranges(&settings, &files)?;

    // Resolve and validate every file up front. A file-type routing rule can
    // send a `.pdf` and a `.jpg` of the same batch to different printers, and
    // duplex support is a property of the printer, so both the resolution and
    // the capability gate are per file. Doing the whole pass before printing
    // anything preserves the all-or-nothing acceptance described above: one
    // unusable target means nothing is spooled.
    let targets: Vec<(String, crate::engine::print::PrinterCaps, JobSettings)> = files
        .iter()
        .map(|file| {
            let name = Path::new(file)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| file.clone());
            let (printer, caps) = resolve_printer(paths, settings.printer.as_deref(), &name)?;
            // The printer's own tray/paper defaults fill what the job leaves
            // open, as in the queue; the check is against what will be sent.
            let over = crate::settings::Config::load_from(&paths.config)
                .printers
                .overrides
                .get(&printer)
                .cloned()
                .unwrap_or_default();
            let effective = crate::engine::print::apply_printer_overrides(&settings, &over);
            crate::engine::print::check_supported(&effective, &printer, &caps)?;
            Ok((printer, caps, effective.into_owned()))
        })
        .collect::<Result<_, CliError>>()?;
    // What the batch reports as its target: the common printer when routing did
    // not split it, otherwise a marker rather than one file's printer standing
    // in for the rest.
    let printer = match targets.first() {
        Some((first, _, _)) if targets.iter().all(|(p, _, _)| p == first) => first.clone(),
        Some(_) => "<per file type>".to_string(),
        None => String::new(),
    };
    // The spool wait is a print, so it is bounded by the print budget, not the
    // conversion one (the two used to share a knob).
    let timeout = match engine.print_timeout_seconds {
        0 => None,
        secs => Some(std::time::Duration::from_secs(u64::from(secs))),
    };

    // Private, self-deleting conversion cache — see the module docs.
    let cache = tempfile::Builder::new()
        .prefix("maeclyprint-cli-")
        .tempdir()
        .map_err(|e| {
            CliError::from(IpcError::Internal(format!(
                "could not create a conversion cache: {e}"
            )))
        })?;

    let batch_id = uuid::Uuid::now_v7().to_string();
    let mut batch = BatchOutcomes::with_capacity(files.len());

    for (index, file) in files.iter().enumerate() {
        let job_id = format!("{batch_id}-{index}");
        let (target, caps, effective) = targets
            .get(index)
            .cloned()
            .unwrap_or_else(|| (String::new(), Default::default(), settings.clone()));
        let outcome = print_one(
            Path::new(file),
            cache.path(),
            &job_id,
            &target,
            &caps,
            &effective,
            timeout,
        );
        batch.record(file, target, outcome);
    }

    let worst = batch.worst();
    let report = PrintReport {
        batch_id,
        printer,
        profile_name: Some(profile.name.clone()),
        settings,
        files: batch.into_files(),
        exit_code: worst.code(),
    };
    emitter.emit(&report, || render(&report));
    Ok(worst)
}

/// One batch's per-file outcomes plus the code the process will return.
///
/// Split out of [`run`] because it is the whole of FR-009's batch rule — every
/// file is recorded whatever happened to it, and the *worst* code wins, so one
/// failure in fifty is never reported as success — and inside `run` that rule
/// could only be exercised by spooling to a real printer.
#[derive(Debug)]
pub struct BatchOutcomes {
    files: Vec<FileOutcome>,
    worst: ExitCode,
}

impl BatchOutcomes {
    /// An empty batch, which reports [`ExitCode::Success`]: nothing failed.
    pub fn with_capacity(files: usize) -> Self {
        Self {
            files: Vec::with_capacity(files),
            worst: ExitCode::Success,
        }
    }

    /// Record what [`print_one`] made of one file: the spooler's job id on
    /// success, or the failure — which also raises the batch's code.
    pub fn record(
        &mut self,
        path: &str,
        printer: String,
        outcome: Result<Option<String>, CliError>,
    ) {
        let file = match outcome {
            Ok(os_job_id) => FileOutcome {
                path: path.to_string(),
                printer,
                state: "done".to_string(),
                os_job_id,
                error: None,
            },
            Err(err) => {
                self.worst = self.worst.worst(err.exit);
                FileOutcome {
                    path: path.to_string(),
                    printer,
                    state: "failed".to_string(),
                    os_job_id: None,
                    error: Some(job_error(&err.error)),
                }
            }
        };
        self.files.push(file);
    }

    /// The exit code the batch reports so far.
    pub fn worst(&self) -> ExitCode {
        self.worst
    }

    /// The recorded rows, in the order the files were printed.
    pub fn into_files(self) -> Vec<FileOutcome> {
        self.files
    }
}

/// Bounds-check the resolved page range against the documents it will apply to,
/// before anything is spooled.
///
/// This calls the *same* [`crate::ipc::jobs::check_ranges_fit_documents`] the
/// print dialog calls, deliberately: `--range 500-600` on an 8-page report used
/// to be accepted here, spooled, and print nothing at all, while the GUI refused
/// it with a message naming the file. A second implementation would drift again,
/// so there is only one.
fn check_ranges(settings: &JobSettings, files: &[String]) -> Result<(), CliError> {
    let Some(ranges) = settings.page_ranges.as_deref() else {
        return Ok(());
    };
    crate::ipc::jobs::check_ranges_fit_documents(ranges, files)
        .map_err(|e| CliError::from(IpcError::InvalidRequest(e)))
}

/// Convert one file (PDFs print in place) and hand it to the spooler.
///
/// "Done" means the OS accepted the job, not that paper came out: no spooler
/// tells us the latter synchronously, and claiming otherwise would be the kind
/// of dishonesty the print-settings policy forbids.
fn print_one(
    input: &Path,
    cache_dir: &Path,
    job_id: &str,
    printer: &str,
    caps: &crate::engine::print::PrinterCaps,
    settings: &JobSettings,
    timeout: Option<std::time::Duration>,
) -> Result<Option<String>, CliError> {
    // The CLI has no cancel channel (Ctrl-C kills the process), so the flag the
    // engine polls is always false here.
    let cancel = AtomicBool::new(false);
    let converted = crate::engine::convert::convert_to_pdf(
        input,
        cache_dir,
        job_id,
        &cancel,
        &|_| {},
        timeout,
    )?;

    let pdf = converted.unwrap_or_else(|| input.to_path_buf());
    let title = input
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| input.display().to_string());

    // Page ranges are a PDF-only concept, so the engine narrows them per input
    // exactly as the dispatcher does — an image would otherwise spool empty.
    let settings =
        crate::engine::print::effective_settings(settings, &crate::engine::extension_of(input));
    let outcome = crate::engine::print::spool(&pdf, printer, &settings, caps, &title, timeout)?;
    // The basename (`title`), never the absolute path (NFR-082).
    tracing::info!(event = "cli_job_printed", file = %title, printer = %printer);
    Ok(outcome.os_job_id)
}

/// The file list, from `--files` or from a `--files-from` handoff payload.
/// Exactly one of the two is present (clap enforces the group).
fn collect_inputs(args: &PrintArgs) -> Result<Vec<String>, CliError> {
    match &args.files_from {
        Some(path) => Ok(handoff::read_file(path)?.paths),
        None => Ok(args.files.clone()),
    }
}

/// Fold a profile and the CLI overrides into one settings bundle, then validate
/// it the way the queue would.
///
/// `--printer` beats the profile's printer; every other flag overrides its
/// profile counterpart and leaves the rest alone.
pub fn resolve_settings(
    profile: &Profile,
    printer: Option<String>,
    flags: &SettingFlags,
) -> Result<JobSettings, CliError> {
    let mut settings = from_profile(profile);

    if let Some(printer) = printer.filter(|p| !p.trim().is_empty()) {
        settings.printer = Some(printer);
    }
    if let Some(mode) = flags.color_mode() {
        settings.color_mode = mode;
    }
    if let Some(duplex) = flags.duplex {
        settings.duplex = duplex.to_engine();
    }
    if let Some(copies) = flags.copies {
        settings.copies = copies;
    }
    if let Some(paper) = flags.paper {
        settings.paper_size = paper.to_engine();
    }
    if let Some(scale) = flags.scale {
        settings.scaling = scale.to_engine();
    }
    if let Some(orientation) = flags.orientation {
        settings.orientation = orientation.to_engine();
    }
    if let Some(range) = &flags.range {
        settings.page_ranges = Some(range.clone());
    }
    if let Some(collate) = flags.collate() {
        settings.collate = collate;
    }

    settings
        .validate()
        .map_err(|e| CliError::from(IpcError::InvalidRequest(e)))?;
    if let Some(ranges) = &settings.page_ranges {
        crate::engine::pagerange::parse(ranges)
            .map_err(|e| CliError::from(IpcError::InvalidRequest(e)))?;
    }
    Ok(settings)
}

/// A profile's engine-facing settings. The `<default>` printer sentinel becomes
/// `None`, which the dispatch step resolves against the app/OS default, and so
/// does the `auto` tray: [`JobSettings::tray`] names a bin the driver knows, and
/// "auto" is the absence of a choice rather than a bin name to pass through.
pub fn from_profile(profile: &Profile) -> JobSettings {
    let printer = profile.printer.trim();
    let tray = profile.tray.trim();
    JobSettings {
        printer: (!printer.is_empty() && printer != "<default>").then(|| printer.to_string()),
        duplex: match profile.duplex {
            ProfileDuplex::Off => Duplex::None,
            ProfileDuplex::LongEdge => Duplex::Long,
            ProfileDuplex::ShortEdge => Duplex::Short,
        },
        color_mode: profile.color_mode,
        copies: profile.copies,
        paper_size: profile.paper_size.clone(),
        orientation: profile.orientation,
        scaling: profile.scaling,
        page_ranges: profile.page_range.clone(),
        quality: profile.quality,
        tray: (!tray.is_empty() && !tray.eq_ignore_ascii_case("auto")).then(|| tray.to_string()),
        collate: profile.collate,
    }
}

/// Apply the queue's acceptance rules to every file before anything is
/// printed — it must resolve to an existing regular file, fit the configured
/// size ceiling and be a format the engine can print — and return the
/// canonical paths to print. One bad file rejects the batch, exactly as
/// `print_files` does in the UI.
///
/// Resolution goes through [`crate::util::paths::canonical_input`], the same
/// gate every externally-supplied path passes (NFR-025), so a `..` segment or a
/// symlink names the same job here as it would in the app.
fn accept_files(files: &[String], max_mb: u32) -> Result<Vec<String>, CliError> {
    let mut accepted = Vec::with_capacity(files.len());

    for file in files {
        let path = crate::util::paths::canonical_input(file)?;
        let meta = std::fs::metadata(&path)
            .map_err(|_| CliError::from(IpcError::FileNotFound(file.clone())))?;
        if !meta.is_file() {
            return Err(CliError::usage(format!("{file} is not a file")));
        }
        if crate::queue::exceeds_size_limit(meta.len(), max_mb) {
            return Err(CliError::usage(format!(
                "{file} is larger than the {max_mb} MB limit"
            )));
        }
        let ext = crate::engine::extension_of(&path);
        if !crate::engine::is_supported(&ext) {
            return Err(CliError::from(IpcError::UnsupportedFormat(format!(
                "{file} is not a printable file type"
            ))));
        }
        accepted.push(path.to_string_lossy().into_owned());
    }
    Ok(accepted)
}

/// Resolve the target printer and whether it can duplex, mirroring the
/// dispatcher's [`crate::engine::print::PrintError`] semantics: the requested
/// name, else MaeclyPrint's app default, else the OS default; a name the OS
/// does not enumerate is refused rather than handed to the spooler.
///
/// Both failures are [`ExitCode::Printer`], so a script can tell "this host is
/// misconfigured" from "the job itself failed". Where the GUI *pauses* such a
/// job for the user to pick a replacement, the CLI has no one to ask and says
/// so immediately.
fn resolve_printer(
    paths: &Paths,
    requested: Option<&str>,
    file_name: &str,
) -> Result<(String, crate::engine::print::PrinterCaps), CliError> {
    // Same order as the queue: explicit request, then the file-type routing
    // rule, then the default (org policy, else app) — one definition, in
    // `ipc::printers`.
    let prefs = crate::ipc::printers::with_policy(
        crate::settings::Config::load_from(&paths.config).printers,
        &crate::settings::PrinterPolicy::load(),
    );
    let name = match crate::ipc::printers::resolve_app_printer(&prefs, requested, file_name) {
        Some(p) => p,
        None => crate::platform::default_printer()
            .ok()
            .flatten()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                CliError::printer("no printer is configured; pass --printer or set a default")
            })?,
    };
    // A printer disabled in MaeclyPrint is refused by name, like the queue does.
    crate::ipc::printers::ensure_enabled(&prefs, &name)
        .map_err(|e| CliError::printer(e.to_string()))?;

    // A failed or empty enumeration is not evidence of absence (the platform
    // backends are best effort), so it keeps trusting the name and lets the
    // spooler speak — the dispatcher makes the same call.
    let enumerated = crate::platform::list_printers()
        .ok()
        .filter(|list| !list.is_empty());
    let Some(list) = enumerated else {
        return Ok((name, crate::engine::print::PrinterCaps::unknown()));
    };
    // `--printer` accepts the id or the display name `printers list` shows,
    // but `lp -d` only knows the queue id — so spool to the matched id.
    use crate::ipc::printers::{match_installed, PrinterMatch};
    match match_installed(&list, &name) {
        PrinterMatch::Found(p) => {
            // A display name can hide a queue id the user disabled.
            crate::ipc::printers::ensure_enabled(&prefs, &p.id)
                .map_err(|e| CliError::printer(e.to_string()))?;
            Ok((p.id.clone(), crate::platform::printer_caps(p)))
        }
        PrinterMatch::Ambiguous(ids) => Err(CliError::printer(format!(
            "printer name '{name}' matches several queues ({}) — pass the queue id",
            ids.join(", ")
        ))),
        PrinterMatch::Missing => Err(CliError::printer(format!(
            "printer '{name}' is not installed — choose another printer"
        ))),
    }
}

/// An [`IpcError`] as the `{code, message}` pair the queue stores on a job, so
/// a CLI report and a queue entry describe a failure identically.
fn job_error(err: &IpcError) -> JobError {
    let code = serde_json::to_value(err)
        .ok()
        .and_then(|v| v["code"].as_str().map(str::to_string))
        .unwrap_or_else(|| "Internal".to_string());
    JobError::new(code, err.to_string())
}

/// Human-mode rendering: the resolved target, then one row per file.
fn render(report: &PrintReport) -> String {
    let rows: Vec<Vec<String>> = report
        .files
        .iter()
        .map(|f| {
            vec![
                f.state.clone(),
                f.path.clone(),
                f.error
                    .as_ref()
                    .map(|e| e.message.clone())
                    .unwrap_or_default(),
            ]
        })
        .collect();
    format!(
        "Printer: {}\nProfile: {}\n{}",
        report.printer,
        report.profile_name.as_deref().unwrap_or("<none>"),
        table(&["STATE", "FILE", "ERROR"], &rows)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::args::{CliDuplex, CliPaper, CliScale};
    use crate::queue::job::{ColorMode, PaperSize, Scaling};

    fn quick() -> Profile {
        crate::profiles::builtins().remove(0)
    }

    #[test]
    fn fr_008_profile_supplies_the_baseline_settings() {
        let duplex_bw = crate::profiles::builtins().remove(1);
        let settings = resolve_settings(&duplex_bw, None, &SettingFlags::default()).unwrap();
        assert_eq!(settings.duplex, Duplex::Long);
        assert_eq!(settings.color_mode, ColorMode::Monochrome);
        // "<default>" never leaks into the engine as a printer name.
        assert_eq!(settings.printer, None);
    }

    #[test]
    fn fr_008_flags_override_the_profile() {
        let flags = SettingFlags {
            mono: true,
            duplex: Some(CliDuplex::ShortEdge),
            copies: Some(3),
            paper: Some(CliPaper::Letter),
            scale: Some(CliScale::Actual),
            range: Some("1-5,8".to_string()),
            ..Default::default()
        };
        let settings = resolve_settings(&quick(), Some("Front Desk".to_string()), &flags).unwrap();
        assert_eq!(settings.printer.as_deref(), Some("Front Desk"));
        assert_eq!(settings.color_mode, ColorMode::Monochrome);
        assert_eq!(settings.duplex, Duplex::Short);
        assert_eq!(settings.copies, 3);
        assert_eq!(settings.paper_size, PaperSize::LETTER);
        assert_eq!(settings.scaling, Scaling::None);
        assert_eq!(settings.page_ranges.as_deref(), Some("1-5,8"));
    }

    #[test]
    fn fr_008_collate_flags_override_the_profile() {
        use crate::cli::args::{Cli, Command};
        use clap::Parser;

        let profile = quick();
        assert!(
            resolve_settings(&profile, None, &SettingFlags::default())
                .unwrap()
                .collate
        );

        let parsed =
            Cli::try_parse_from(["maeclyprint", "print", "--files", "a.pdf", "--no-collate"])
                .expect("--no-collate parses");
        let Some(Command::Print(args)) = parsed.command else {
            panic!("expected print");
        };
        assert!(
            !resolve_settings(&profile, None, &args.settings)
                .unwrap()
                .collate
        );

        let parsed = Cli::try_parse_from(["maeclyprint", "print", "--files", "a.pdf", "--collate"])
            .expect("--collate parses");
        let Some(Command::Print(args)) = parsed.command else {
            panic!("expected print");
        };
        assert!(
            resolve_settings(&profile, None, &args.settings)
                .unwrap()
                .collate
        );

        assert!(
            Cli::try_parse_from([
                "maeclyprint",
                "print",
                "--files",
                "a.pdf",
                "--collate",
                "--no-collate"
            ])
            .is_err(),
            "the two flags conflict"
        );
    }

    #[test]
    fn fr_009_bad_settings_are_usage_errors() {
        let too_many = SettingFlags {
            copies: Some(0),
            ..Default::default()
        };
        assert_eq!(
            resolve_settings(&quick(), None, &too_many)
                .unwrap_err()
                .exit,
            ExitCode::Usage
        );

        let bad_range = SettingFlags {
            range: Some("5-1".to_string()),
            ..Default::default()
        };
        assert_eq!(
            resolve_settings(&quick(), None, &bad_range)
                .unwrap_err()
                .exit,
            ExitCode::Usage
        );
    }

    #[test]
    fn fr_009_job_error_keeps_the_ipc_code() {
        let err = job_error(&IpcError::UnsupportedFormat("a.docx".into()));
        assert_eq!(err.code, "UnsupportedFormat");
        assert!(err.message.contains("a.docx"));
    }

    /// A real 1-page PDF, written by the engine's own image converter so the
    /// page count the bounds check reads is the one production reads.
    fn one_page_pdf(dir: &Path, name: &str) -> std::path::PathBuf {
        use ::image::{ImageFormat, Rgb, RgbImage};
        let mut img = RgbImage::new(4, 4);
        for (x, y, px) in img.enumerate_pixels_mut() {
            *px = Rgb([(x * 60) as u8, (y * 60) as u8, 10]);
        }
        let src = dir.join("src.png");
        img.save_with_format(&src, ImageFormat::Png).unwrap();
        let out = dir.join(name);
        crate::engine::convert::image::to_pdf(&src, &out, &|_p| {}).unwrap();
        out
    }

    #[test]
    fn fr_042_a_range_past_the_document_is_refused_before_spooling() {
        // The defect: the GUI refused "500-600" on a short PDF while the CLI
        // accepted it, spooled, and printed nothing. Both now go through
        // `ipc::jobs::check_ranges_fit_documents`.
        let dir = tempfile::tempdir().unwrap();
        let pdf = one_page_pdf(dir.path(), "report.pdf");
        let files = vec![pdf.to_string_lossy().into_owned()];

        let flags = SettingFlags {
            range: Some("500-600".to_string()),
            ..Default::default()
        };
        // The grammar is fine, so nothing catches this before the bounds check.
        let settings = resolve_settings(&quick(), None, &flags).unwrap();

        let err = check_ranges(&settings, &files).unwrap_err();
        assert_eq!(err.exit, ExitCode::Usage);
        let message = err.error.to_string();
        assert!(
            message.contains("report.pdf"),
            "the message names the file: {message}"
        );

        // A range that fits, and no range at all, both pass.
        let fits = SettingFlags {
            range: Some("1".to_string()),
            ..Default::default()
        };
        assert!(check_ranges(&resolve_settings(&quick(), None, &fits).unwrap(), &files).is_ok());
        assert!(check_ranges(
            &resolve_settings(&quick(), None, &SettingFlags::default()).unwrap(),
            &files
        )
        .is_ok());
    }

    #[test]
    fn fr_009_a_batch_records_every_file_and_reports_the_worst_code() {
        let mut batch = BatchOutcomes::with_capacity(3);
        assert_eq!(batch.worst(), ExitCode::Success, "an empty batch succeeded");

        batch.record(
            "/tmp/a.pdf",
            "Front Desk".to_string(),
            Ok(Some("42".into())),
        );
        assert_eq!(batch.worst(), ExitCode::Success);

        batch.record(
            "/tmp/b.pdf",
            "Front Desk".to_string(),
            Err(CliError::from(IpcError::PrintFailed("spooler".into()))),
        );
        batch.record(
            "/tmp/c.pdf",
            "Front Desk".to_string(),
            Err(CliError::from(IpcError::FileNotFound("/tmp/c.pdf".into()))),
        );

        // PrintFailed (9) outranks FileNotFound (6): the worst code wins, not
        // the last one.
        assert_eq!(batch.worst(), ExitCode::PrintFailed);

        let files = batch.into_files();
        assert_eq!(files.len(), 3, "a failure never drops a row");
        assert_eq!(files[0].state, "done");
        assert_eq!(files[0].os_job_id.as_deref(), Some("42"));
        assert_eq!(files[1].state, "failed");
        assert_eq!(files[1].error.as_ref().unwrap().code, "PrintFailed");
        assert_eq!(files[2].error.as_ref().unwrap().code, "FileNotFound");
    }
}
