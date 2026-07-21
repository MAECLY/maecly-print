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
    let (printer, duplex_supported) = resolve_printer(paths, settings.printer.as_deref())?;
    crate::engine::print::check_supported(&settings, &printer, duplex_supported, true)?;
    let timeout = match engine.conversion_timeout_seconds {
        0 => None,
        secs => Some(std::time::Duration::from_secs(secs as u64)),
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
    let mut worst = ExitCode::Success;
    let mut outcomes = Vec::with_capacity(files.len());

    for (index, file) in files.iter().enumerate() {
        let job_id = format!("{batch_id}-{index}");
        match print_one(
            Path::new(file),
            cache.path(),
            &job_id,
            &printer,
            &settings,
            timeout,
        ) {
            Ok(os_job_id) => outcomes.push(FileOutcome {
                path: file.clone(),
                state: "done".to_string(),
                os_job_id,
                error: None,
            }),
            Err(err) => {
                worst = worst.worst(err.exit);
                outcomes.push(FileOutcome {
                    path: file.clone(),
                    state: "failed".to_string(),
                    os_job_id: None,
                    error: Some(job_error(&err.error)),
                });
            }
        }
    }

    let report = PrintReport {
        batch_id,
        printer,
        profile_name: Some(profile.name.clone()),
        settings,
        files: outcomes,
        exit_code: worst.code(),
    };
    emitter.emit(&report, || render(&report));
    Ok(worst)
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
    settings: &JobSettings,
    timeout: Option<std::time::Duration>,
) -> Result<Option<String>, CliError> {
    // The CLI has no cancel channel (Ctrl-C kills the process), so the flag the
    // engine polls is always false here.
    let cancel = AtomicBool::new(false);
    let converted =
        crate::engine::convert::convert_to_pdf(input, cache_dir, job_id, &cancel, &|_| {})?;

    let pdf = converted.unwrap_or_else(|| input.to_path_buf());
    let title = input
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| input.display().to_string());

    // Page ranges are a PDF-only concept, so the engine narrows them per input
    // exactly as the dispatcher does — an image would otherwise spool empty.
    let settings =
        crate::engine::print::effective_settings(settings, &crate::engine::extension_of(input));
    let outcome = crate::engine::print::spool(&pdf, printer, &settings, &title, timeout)?;
    tracing::info!(event = "cli_job_printed", file = %input.display(), printer = %printer);
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
/// profile counterpart and leaves the rest alone. The profile's `quality` and
/// `tray` have no engine counterpart in [`JobSettings`] and are therefore not
/// carried over — the engine cannot honour them yet, and pretending otherwise
/// would show the user a setting that does nothing.
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
/// `None`, which the dispatch step resolves against the app/OS default.
pub fn from_profile(profile: &Profile) -> JobSettings {
    let printer = profile.printer.trim();
    JobSettings {
        printer: (!printer.is_empty() && printer != "<default>").then(|| printer.to_string()),
        duplex: match profile.duplex {
            ProfileDuplex::Off => Duplex::None,
            ProfileDuplex::LongEdge => Duplex::Long,
            ProfileDuplex::ShortEdge => Duplex::Short,
        },
        color_mode: profile.color_mode,
        copies: profile.copies,
        paper_size: profile.paper_size,
        orientation: profile.orientation,
        scaling: profile.scaling,
        page_ranges: profile.page_range.clone(),
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
fn resolve_printer(paths: &Paths, requested: Option<&str>) -> Result<(String, bool), CliError> {
    let name = match requested {
        Some(p) if !p.trim().is_empty() => p.to_string(),
        _ => crate::settings::Config::load_from(&paths.config)
            .printers
            .default_printer
            .filter(|s| !s.is_empty())
            .or_else(|| {
                crate::platform::default_printer()
                    .ok()
                    .flatten()
                    .filter(|s| !s.is_empty())
            })
            .ok_or_else(|| {
                CliError::printer("no printer is configured; pass --printer or set a default")
            })?,
    };

    // A failed or empty enumeration is not evidence of absence (the platform
    // backends are best effort), so it keeps trusting the name and lets the
    // spooler speak — the dispatcher makes the same call.
    let enumerated = crate::platform::list_printers()
        .ok()
        .filter(|list| !list.is_empty());
    let Some(list) = enumerated else {
        return Ok((name, true));
    };
    match list.iter().find(|p| p.id == name || p.name == name) {
        Some(p) => Ok((name, p.duplex)),
        None => Err(CliError::printer(format!(
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
        assert_eq!(settings.paper_size, PaperSize::Letter);
        assert_eq!(settings.scaling, Scaling::None);
        assert_eq!(settings.page_ranges.as_deref(), Some("1-5,8"));
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
}
