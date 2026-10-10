//! Print-queue IPC commands.
//!
//! Thin Tauri command wrappers over [`crate::queue::QueueHandle`] (held as
//! managed state) plus two stateless helpers the print dialog needs before it
//! submits: [`inspect_files`] (per-file metadata for the file list) and
//! [`preview_command`] (the honest "this is what we'll run" string), plus the
//! two-step folder flow a drop of a directory needs — [`inspect_files`] marks
//! which of the dropped entries are folders (and whether they hold subfolders)
//! so the UI can ask the recurse-or-not question (FR-004), and
//! [`expand_folders`] then walks them with the answer. The queue
//! itself pushes updates via the events in [`crate::ipc::events`]; these
//! commands are the request/response half of the contract in
//! `docs/04-architecture/ipc-and-cli.md`.

use std::path::Path;

use serde::Serialize;
use specta::Type;
use tauri::State;

use crate::engine::{ExpansionLimits, FolderExpansion, FolderScan};
use crate::queue::job::{JobBatch, PrintRequest, QueueSnapshot};
use crate::queue::QueueHandle;
use crate::util::error::IpcError;
use crate::util::paths::canonical_input;

/// Per-file inspection result for the print dialog's file list. Never errors the
/// whole batch: a missing or unsupported file comes back with `supported=false`
/// and a human `reason` rather than failing the call.
#[derive(Debug, Clone, Serialize, Type)]
pub struct FileInfo {
    /// The path as the caller sent it (canonicalized when it resolved).
    pub path: String,
    /// File name shown in the list.
    pub name: String,
    /// Lowercased extension without the dot (e.g. `"pdf"`), empty when none.
    pub ext: String,
    /// File size in bytes (0 when the file could not be read).
    pub bytes: u64,
    /// Page count when cheap to learn (PDFs under the probe limit), else `None`.
    pub pages: Option<u32>,
    /// Whether the engine can print this file.
    pub supported: bool,
    /// Why the file is not printable, when `supported` is false (English
    /// detail for logs; the UI translates [`FileInfo::reason_code`]).
    pub reason: Option<String>,
    /// Stable, machine-readable form of `reason` (FR-092), set exactly when
    /// `reason` is.
    pub reason_code: Option<RejectReason>,
    /// The path is a directory. Not printable as such, but not an error either:
    /// the caller runs it through [`expand_folders`] after asking the FR-004
    /// recurse-or-not question.
    pub is_dir: bool,
    /// Only meaningful when `is_dir`: whether the folder holds subfolders at
    /// all. When it does not there is nothing to recurse into, so the UI skips
    /// the question and expands top-level (FR-004 asks; it does not nag).
    pub has_subfolders: bool,
}

/// Why a path is not printable as-is. Stable wire values the UI maps to
/// translated text (FR-005, FR-092); never renamed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum RejectReason {
    /// The path does not exist (or vanished before it could be read).
    NotFound,
    /// The path could not be resolved at all (relative, empty, unreadable).
    InvalidPath,
    /// A folder: legal input that must go through `expand_folders` first.
    Folder,
    /// Exists but is neither a regular file nor a folder (device, socket…).
    NotAFile,
    /// Bigger than the configured `max_file_size_mb` ceiling.
    TooLarge,
    /// A GIF image: refused on purpose (decision D9), with its own sentence
    /// ("convert it to PNG or JPEG first").
    GifUnsupported,
    /// Any other extension the engine cannot print.
    UnsupportedType,
}

/// The [`RejectReason`] for a path [`canonical_input`] refused.
fn resolve_reason(e: &IpcError) -> RejectReason {
    match e.code() {
        crate::util::error::IpcErrorCode::FileNotFound => RejectReason::NotFound,
        _ => RejectReason::InvalidPath,
    }
}

/// Submit a print request. Validates and fans the files out into jobs, returning
/// the batch id and per-file job ids. Runs the (blocking) validation/probe off
/// the IPC thread.
///
/// Every path is resolved through [`canonical_input`] first (NFR-025), so the
/// queue only ever stores absolute, symlink-resolved paths and a relative one
/// can never be silently resolved against the process' working directory.
///
/// The page range is then bounds-checked against each PDF's real page count
/// (FR-042/FR-043) before anything is enqueued, so `"500-600"` on an 8-page
/// report is an error the user can read and fix — not a job that reaches `Done`
/// having printed nothing.
#[tauri::command]
#[specta::specta]
pub async fn print_files(
    state: State<'_, QueueHandle>,
    req: PrintRequest,
) -> Result<JobBatch, IpcError> {
    let handle = state.inner().clone();
    run_blocking(move || {
        let mut req = req;
        req.files = req
            .files
            .iter()
            .map(|f| canonical_input(f).map(|p| p.to_string_lossy().into_owned()))
            .collect::<Result<Vec<_>, _>>()?;
        if let Some(ranges) = req.settings.page_ranges.as_deref() {
            check_ranges_fit_documents(ranges, &req.files)?;
        }
        handle.submit(req)
    })
    .await
}

/// Bounds-check one batch-wide page range against every file it will actually
/// apply to.
///
/// Two deliberate narrowings:
/// - **PDFs only.** `engine::print::effective_settings` drops the range for
///   image inputs (an image is one page), so a range that image would not honour
///   anyway must not fail the batch — otherwise "1-5" would become unusable the
///   moment a PNG joins the selection.
/// - **Known page counts only.** `probe_pages` returns `None` for PDFs above the
///   probe ceiling or that will not parse; those pass through, since refusing to
///   print what we cannot measure is worse than trusting the user. Documents
///   that still need converting (DOCX, HTML, …) do take the range, but their
///   page count only exists after conversion, so they are not checked here.
///
/// The message names the file: one range covers the whole batch, so "which one?"
/// is the first thing the user needs to know.
///
/// `pub(crate)` because the headless surface calls exactly this function
/// (`cli::print::check_ranges`): `pluryprint print --range 500-600` used to
/// spool zero pages in silence where the dialog refused the same range, and two
/// copies of this rule would have drifted apart again.
pub(crate) fn check_ranges_fit_documents(
    ranges: &str,
    files: &[String],
) -> Result<(), crate::util::error::LocalizedError> {
    let spans = crate::engine::pagerange::parse(ranges)?;
    for file in files {
        let path = Path::new(file);
        let ext = crate::engine::extension_of(path);
        if ext != "pdf" {
            continue;
        }
        let Ok(meta) = std::fs::metadata(path) else {
            continue; // submit reports the missing file with its own error
        };
        let pages = crate::queue::probe_pages(path, &ext, meta.len());
        if let Err(e) = crate::engine::pagerange::check_bounds(&spans, pages) {
            let name = path
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| file.clone());
            // The name only: NFR-082 keeps absolute paths out of the log.
            tracing::warn!(event = "page_range_out_of_bounds", file = %name, ranges = %ranges, reason = %e.message);
            let message = format!("{name}: {}", e.message);
            let mut e = e.param("name", name);
            e.message = message;
            return Err(e);
        }
    }
    Ok(())
}

/// Expand folders into printable files with the user's FR-004 answer
/// (`top_level` vs `recursive`), one result per folder.
///
/// Discarded files come back in [`FolderExpansion::skipped`] with a reason
/// rather than vanishing (FR-005/FR-059); the caller logs them against the job.
/// Each root is walked under the default [`ExpansionLimits`], so dropping a
/// disk root truncates instead of hanging.
#[tauri::command]
#[specta::specta]
pub async fn expand_folders(
    paths: Vec<String>,
    scan: FolderScan,
) -> Result<Vec<FolderExpansion>, IpcError> {
    run_blocking(move || {
        // One config read for the whole call, matching submit/inspect.
        let max_mb = crate::settings::Config::load_or_default()
            .engine
            .sanitized()
            .max_file_size_mb;
        Ok(expand_selection(
            &paths,
            scan,
            ExpansionLimits::default(),
            max_mb,
        ))
    })
    .await
}

/// Path-injected core of [`expand_folders`]: the limits and the size ceiling are
/// arguments, so tests drive tiny ceilings without touching the user's config.
/// A path that is not a readable folder yields an empty expansion for that root
/// rather than failing the batch.
pub fn expand_selection(
    paths: &[String],
    scan: FolderScan,
    limits: ExpansionLimits,
    max_file_size_mb: u32,
) -> Vec<FolderExpansion> {
    paths
        .iter()
        .map(|raw| {
            let root = canonical_input(raw)
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_else(|_| raw.clone());
            let expansion =
                crate::engine::expand_folder(Path::new(&root), scan, limits, max_file_size_mb);
            // FR-059: the discarded files are reported to the caller *and*
            // written to the app log, so a silently thin batch can always be
            // explained after the fact.
            tracing::info!(
                event = "folder_expanded",
                root = %expansion.root,
                scan = ?expansion.scan,
                accepted = expansion.files.len(),
                skipped = expansion.skipped.len(),
                files_truncated = expansion.files_truncated,
                depth_truncated = expansion.depth_truncated,
            );
            for s in &expansion.skipped {
                tracing::debug!(
                    event = "folder_entry_skipped",
                    path = %s.path,
                    reason = ?s.reason,
                    detail = %s.detail,
                );
            }
            expansion
        })
        .collect()
}

/// Whether `dir` contains at least one subdirectory. Cheap single-level read;
/// an unreadable directory answers `false` (the expansion will report the real
/// problem).
fn has_subfolders(dir: &Path) -> bool {
    match std::fs::read_dir(dir) {
        Ok(entries) => entries.flatten().any(|e| e.path().is_dir()),
        Err(_) => false,
    }
}

/// A consistent snapshot of the whole queue (for the initial UI paint; live
/// updates arrive via the `queue-updated` event).
#[tauri::command]
#[specta::specta]
pub async fn queue_status(state: State<'_, QueueHandle>) -> Result<QueueSnapshot, IpcError> {
    Ok(state.snapshot())
}

/// Cancel a single job by id.
#[tauri::command]
#[specta::specta]
pub async fn cancel_job(state: State<'_, QueueHandle>, id: String) -> Result<(), IpcError> {
    state.cancel(id).await
}

/// Cancel every non-terminal job; returns how many were cancelled.
#[tauri::command]
#[specta::specta]
pub async fn cancel_all(state: State<'_, QueueHandle>) -> Result<u32, IpcError> {
    state.cancel_all().await
}

/// Pause a single job.
#[tauri::command]
#[specta::specta]
pub async fn pause_job(state: State<'_, QueueHandle>, id: String) -> Result<(), IpcError> {
    state.pause(id).await
}

/// Resume a single paused job.
#[tauri::command]
#[specta::specta]
pub async fn resume_job(state: State<'_, QueueHandle>, id: String) -> Result<(), IpcError> {
    state.resume(id).await
}

/// Change a paused job's printer or sides (e.g. one-sided, or another
/// printer, for a job paused because its printer cannot do what it asks).
/// The job stays paused; resume it to send it.
#[tauri::command]
#[specta::specta]
pub async fn adjust_job(
    state: State<'_, QueueHandle>,
    id: String,
    change: crate::queue::job::JobChange,
) -> Result<(), IpcError> {
    state.adjust(id, change).await
}

/// Retry a single failed job from the start.
#[tauri::command]
#[specta::specta]
pub async fn retry_job(state: State<'_, QueueHandle>, id: String) -> Result<(), IpcError> {
    state.retry(id).await
}

/// Pause the whole queue (stops starting new work; in-flight jobs continue).
#[tauri::command]
#[specta::specta]
pub async fn pause_queue(state: State<'_, QueueHandle>) -> Result<(), IpcError> {
    state.pause_queue();
    Ok(())
}

/// Resume the whole queue.
#[tauri::command]
#[specta::specta]
pub async fn resume_queue(state: State<'_, QueueHandle>) -> Result<(), IpcError> {
    state.resume_queue();
    Ok(())
}

/// Remove all terminal jobs from the queue; returns how many were removed.
#[tauri::command]
#[specta::specta]
pub async fn clear_done(state: State<'_, QueueHandle>) -> Result<u32, IpcError> {
    state.clear_done().await
}

/// Inspect a list of file paths for the print dialog. Each entry is classified
/// independently; a bad path yields `supported=false` with a reason rather than
/// failing the whole call.
#[tauri::command]
#[specta::specta]
pub async fn inspect_files(paths: Vec<String>) -> Result<Vec<FileInfo>, IpcError> {
    run_blocking(move || {
        // Load the size ceiling once for the whole list, matching submit.
        // `sanitized()` so a hand-edited `0` doesn't mark every file
        // unsupported.
        let max_mb = crate::settings::Config::load_or_default()
            .engine
            .sanitized()
            .max_file_size_mb;
        Ok(paths.iter().map(|p| inspect_one(p, max_mb)).collect())
    })
    .await
}

/// The exact command Pluryprint would run for `req` (honesty policy: no hidden
/// flags), for **every** file in the batch.
///
/// The dispatcher settles three things per file, so the preview does too:
/// the printer (an explicit request, else the file-type routing rule, else the
/// app default, else the OS default — a `.pdf → EPSON` rule can split a batch
/// across printers), the settings narrowed to the input (page ranges are
/// PDF-only), and the printer's own tray/paper defaults where the job leaves
/// them open. Files that come out identical share one command; when the batch
/// splits, each command is preceded by a comment naming its files, so the panel
/// never shows one file's command as if it were everyone's. See
/// [`preview_for_files`].
#[tauri::command]
#[specta::specta]
pub async fn preview_command(req: PrintRequest) -> Result<String, IpcError> {
    run_blocking(move || {
        // The same default chain the queue resolves: org policy, else app.
        let prefs = crate::ipc::printers::with_policy(
            crate::settings::Config::load_or_default().printers,
            &crate::settings::PrinterPolicy::load(),
        );
        let os_default = crate::platform::default_printer().ok().flatten();
        Ok(preview_for_files(
            &req.files,
            &req.settings,
            &prefs,
            os_default.as_deref(),
            crate::platform::queue_of,
        ))
    })
    .await
}

/// Pure core of [`preview_command`]: the queue id and capabilities of each
/// resolved printer come from `queue_of` (looked up once per printer), so a
/// display name previews as the queue id the spooler receives, and the
/// grouping is testable without an installed printer.
fn preview_for_files(
    files: &[String],
    settings: &crate::queue::job::JobSettings,
    prefs: &crate::settings::PrinterPrefs,
    os_default: Option<&str>,
    queue_of: impl Fn(&str) -> (String, crate::engine::print::PrinterCaps),
) -> String {
    use crate::queue::job::JobSettings;

    let placeholder = ["document.pdf".to_string()];
    let files = if files.is_empty() {
        &placeholder[..]
    } else {
        files
    };

    // resolved name → (queue id, caps), so each printer is looked up once.
    let mut queues: Vec<(String, String, crate::engine::print::PrinterCaps)> = Vec::new();
    // (queue id, effective settings) → display names, in batch order.
    let mut groups: Vec<(String, JobSettings, Vec<String>)> = Vec::new();
    for file in files {
        let path = Path::new(file);
        let display = path
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| file.clone());
        let resolved =
            resolve_preview_printer(prefs, settings.printer.as_deref(), &display, os_default);
        let printer = match queues.iter().find(|(n, _, _)| *n == resolved) {
            Some((_, id, _)) => id.clone(),
            None => {
                let (id, caps) = queue_of(&resolved);
                queues.push((resolved, id.clone(), caps));
                id
            }
        };
        let ext = crate::engine::extension_of(path);
        let narrowed = crate::engine::print::effective_settings(settings, &ext);
        let effective = crate::engine::print::apply_printer_overrides(
            &narrowed,
            &prefs.overrides.get(&printer).cloned().unwrap_or_default(),
        )
        .into_owned();
        match groups
            .iter_mut()
            .find(|(p, s, _)| *p == printer && *s == effective)
        {
            Some((_, _, names)) => names.push(display),
            None => groups.push((printer, effective, vec![display])),
        }
    }

    let mut blocks = Vec::with_capacity(groups.len());
    for (printer, effective, names) in &groups {
        let caps = queues
            .iter()
            .find(|(_, id, _)| id == printer)
            .map(|(_, _, c)| c.clone())
            .unwrap_or_default();
        let command = crate::engine::print::preview_command(&names[0], printer, effective, &caps);
        if groups.len() == 1 {
            blocks.push(command);
        } else {
            // `#` is a comment in both PowerShell and a POSIX shell; a line
            // break in a file name must not end it and start a command.
            let names = crate::engine::print::comment_text(&names.join(", "));
            blocks.push(format!("# {names}\n{command}"));
        }
    }
    blocks.join("\n\n")
}

/// The printer name the preview should show.
///
/// The app-level part of the chain is **not** re-implemented here: it delegates
/// to [`crate::ipc::printers::resolve_app_printer`], the same function
/// `queue::dispatcher::EngineRunner::resolve_printer` and `cli::print` call, so
/// the panel and the spooler can never disagree. That matters because the chain
/// has a step that is easy to forget: a file-type routing rule (`.pdf → EPSON`)
/// sits *between* the explicit request and the app default, and a preview that
/// skipped it showed `lp -d HL_L2350` for a job that then printed on the EPSON —
/// precisely the drift this panel exists to prevent. `file_name` is what makes
/// the rule apply, so the preview is per-file, like the dispatch is.
///
/// Only the last two steps are local: the OS default (not this layer's business
/// to read, hence an argument) and a visible placeholder when even that is
/// missing. Empty strings count as "not set" throughout.
///
/// Pure (prefs and both defaults are arguments) so the precedence can be tested
/// without a config file or an installed printer.
fn resolve_preview_printer(
    prefs: &crate::settings::PrinterPrefs,
    requested: Option<&str>,
    file_name: &str,
    os_default: Option<&str>,
) -> String {
    crate::ipc::printers::resolve_app_printer(prefs, requested, file_name)
        .or_else(|| {
            os_default
                .filter(|s| !s.is_empty())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
        })
        .unwrap_or_else(|| "<default printer>".to_string())
}

/// Classify one path for [`inspect_files`]. `max_mb` is the current size ceiling
/// (from `EngineCfg`); an oversized file is reported unsupported with a size
/// reason, which takes precedence over an unsupported-format reason.
///
/// The path is resolved first (NFR-025) and the canonical form is what comes
/// back in [`FileInfo::path`], so the caller submits exactly what was
/// inspected. A path that will not resolve is reported unsupported with the
/// resolution error, keeping the never-fail contract of the whole call.
fn inspect_one(path_str: &str, max_mb: u32) -> FileInfo {
    let info = classify_one(path_str, max_mb);
    // FR-059: never skip a file silently. The UI already toasts the reason, but
    // a toast is gone in three seconds — the app log is what can still explain
    // a thin batch tomorrow. Folders are not "skipped", they are a pending
    // question, so they stay out of the rejection log.
    if !info.supported && !info.is_dir {
        // `name`, not `path`: the canonical path carries the user name and
        // folder names, which NFR-082 keeps out of the default-level log.
        tracing::warn!(
            event = "file_rejected",
            file = %info.name,
            ext = %info.ext,
            bytes = info.bytes,
            reason = %info.reason.as_deref().unwrap_or("unknown"),
        );
    }
    info
}

/// The classification itself, without the logging side effect (so tests can
/// assert the mapping without asserting on a subscriber).
fn classify_one(path_str: &str, max_mb: u32) -> FileInfo {
    let resolved = match canonical_input(path_str) {
        Ok(p) => p,
        Err(e) => {
            let path = Path::new(path_str);
            let name = path
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| path_str.to_string());
            let code = resolve_reason(&e);
            let reason = match code {
                RejectReason::NotFound => "File not found".to_string(),
                _ => e.to_string(),
            };
            return unprintable(
                path_str.to_string(),
                name,
                crate::engine::extension_of(path),
                code,
                reason,
            );
        }
    };
    let path = resolved.as_path();
    let canonical = resolved.to_string_lossy().into_owned();
    let name = path
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| canonical.clone());
    let ext = crate::engine::extension_of(path);

    match std::fs::metadata(path) {
        Err(_) => unprintable(
            canonical,
            name,
            ext,
            RejectReason::NotFound,
            "File not found".to_string(),
        ),
        Ok(m) if m.is_dir() => FileInfo {
            // FR-004: folders are legal input, they just have to go through
            // `expand_folders` before they reach the file list.
            has_subfolders: has_subfolders(path),
            is_dir: true,
            ..unprintable(
                canonical,
                name,
                ext,
                RejectReason::Folder,
                "Folder — expand it to print the files inside".to_string(),
            )
        },
        Ok(m) if !m.is_file() => unprintable(
            canonical,
            name,
            ext,
            RejectReason::NotAFile,
            "Not a file".to_string(),
        ),
        Ok(m) => {
            // Precedence: size first (a 900 MB PDF is refused for its size, not
            // its type), then the format. A format whose converter is not
            // installed is still accepted here: its job pauses with the FR-058
            // remediation, and the Print Job screen already shows that text
            // for the dependencies the selected files need.
            let refusal = if crate::queue::exceeds_size_limit(m.len(), max_mb) {
                Some((
                    RejectReason::TooLarge,
                    format!("Larger than the {max_mb} MB limit"),
                ))
            } else if crate::engine::is_supported(&ext) {
                None
            } else if ext == "gif" {
                Some((
                    RejectReason::GifUnsupported,
                    "GIF images are not supported: convert them to PNG or JPEG first".to_string(),
                ))
            } else {
                Some((
                    RejectReason::UnsupportedType,
                    "Unsupported file type".to_string(),
                ))
            };
            let supported = refusal.is_none();
            let (reason_code, reason) = match refusal {
                Some((code, text)) => (Some(code), Some(text)),
                None => (None, None),
            };
            let pages = crate::queue::probe_pages(path, &ext, m.len());
            FileInfo {
                path: canonical,
                name,
                ext,
                bytes: m.len(),
                pages,
                supported,
                reason,
                reason_code,
                is_dir: false,
                has_subfolders: false,
            }
        }
    }
}

/// A `supported: false` [`FileInfo`] carrying `reason`. Keeps the six unchanging
/// fields in one place so each refusal branch reads as just its own decision.
fn unprintable(
    path: String,
    name: String,
    ext: String,
    reason_code: RejectReason,
    reason: String,
) -> FileInfo {
    FileInfo {
        path,
        name,
        ext,
        bytes: 0,
        pages: None,
        supported: false,
        reason: Some(reason),
        reason_code: Some(reason_code),
        is_dir: false,
        has_subfolders: false,
    }
}

/// Run a blocking closure off the IPC thread, normalizing a task panic to
/// [`IpcError::Internal`]. The closure returns an [`IpcError`] directly (unlike
/// the printer commands' `anyhow`-based helper) because these paths already
/// produce typed errors.
async fn run_blocking<T, F>(f: F) -> Result<T, IpcError>
where
    F: FnOnce() -> Result<T, IpcError> + Send + 'static,
    T: Send + 'static,
{
    match tauri::async_runtime::spawn_blocking(f).await {
        Ok(r) => r,
        Err(e) => Err(IpcError::Internal(format!("queue task panicked: {e}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// A mixed batch is not previewed as if every file were the first one:
    /// the PDF's page range and the routed printer show up on their own line.
    #[test]
    fn preview_lists_one_command_per_distinct_file_group() {
        use crate::engine::print::PrinterCaps;
        use crate::queue::job::JobSettings;

        let mut prefs = crate::settings::PrinterPrefs::default();
        prefs.routes.insert(".pdf".into(), "EPSON".into());
        let settings = JobSettings {
            page_ranges: Some("1-2".into()),
            ..JobSettings::default()
        };
        let files = vec![
            "/d/chart.png".to_string(),
            "/d/report.pdf".to_string(),
            "/d/photo.png".to_string(),
        ];
        let out = preview_for_files(&files, &settings, &prefs, Some("Office"), |n| {
            (n.to_string(), PrinterCaps::unknown())
        });
        let blocks: Vec<&str> = out.split("\n\n").collect();
        assert_eq!(blocks.len(), 2, "{out}");

        let images = blocks[0];
        assert!(images.contains("chart.png, photo.png"), "{images}");
        assert!(images.contains("Office"), "{images}");
        assert!(
            !images.contains("page-ranges") && !images.contains("1-2"),
            "{images}"
        );

        let pdf = blocks[1];
        assert!(pdf.contains("report.pdf"), "{pdf}");
        assert!(pdf.contains("EPSON"), "routed printer shown: {pdf}");
        assert!(pdf.contains("1-2"), "PDF keeps its range: {pdf}");

        // A uniform batch stays a single, comment-free command.
        let same = preview_for_files(
            &["/d/a.pdf".to_string(), "/d/b.pdf".to_string()],
            &JobSettings::default(),
            &crate::settings::PrinterPrefs::default(),
            Some("Office"),
            |n| (n.to_string(), PrinterCaps::unknown()),
        );
        assert!(!same.contains("\n\n") && !same.starts_with('#'), "{same}");
        assert!(same.contains("a.pdf"), "{same}");
    }

    /// A display name (a profile's or the Print Job dropdown's) previews as the
    /// queue id the dispatcher really spools to, and the printer's per-queue
    /// defaults are looked up under that id.
    #[test]
    fn preview_names_the_queue_id_for_a_display_name() {
        use crate::engine::print::PrinterCaps;
        use crate::queue::job::JobSettings;

        let settings = JobSettings {
            printer: Some("HP LaserJet Pro M404".into()),
            ..JobSettings::default()
        };
        let out = preview_for_files(
            &["/d/a.pdf".to_string()],
            &settings,
            &crate::settings::PrinterPrefs::default(),
            None,
            |n| {
                assert_eq!(n, "HP LaserJet Pro M404");
                ("HP_LaserJet_Pro_M404".to_string(), PrinterCaps::unknown())
            },
        );
        assert!(out.contains("HP_LaserJet_Pro_M404"), "{out}");
        assert!(!out.contains("HP LaserJet Pro M404"), "{out}");
    }

    /// A 1-page PDF written by the real image converter, so the page count the
    /// bounds check reads is the one production would read.
    fn one_page_pdf(dir: &Path, name: &str) -> PathBuf {
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

    fn write(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
        let p = dir.join(name);
        std::fs::write(&p, bytes).unwrap();
        p
    }

    fn inspect(path: &Path, max_mb: u32) -> FileInfo {
        classify_one(&path.to_string_lossy(), max_mb)
    }

    // -- inspect_one -------------------------------------------------------

    #[test]
    fn inspect_reports_a_missing_file_without_failing() {
        let dir = tempfile::tempdir().unwrap();
        let info = classify_one(&dir.path().join("ghost.pdf").to_string_lossy(), 100);
        assert!(!info.supported);
        assert_eq!(info.reason.as_deref(), Some("File not found"));
        assert_eq!(info.reason_code, Some(RejectReason::NotFound));
        assert_eq!(info.name, "ghost.pdf");
        assert!(!info.is_dir);
    }

    /// NFR-025: a relative path is refused, never resolved against the
    /// process' working directory — and refused per entry, not for the call.
    #[test]
    fn nfr_025_inspect_refuses_a_relative_path() {
        let info = classify_one("relative/doc.pdf", 100);
        assert!(!info.supported);
        assert!(!info.is_dir);
        assert!(
            info.reason.as_deref().unwrap_or("").contains("absolute"),
            "reason: {:?}",
            info.reason
        );
    }

    #[test]
    fn fr_004_inspect_reports_a_folder_as_expandable() {
        let dir = tempfile::tempdir().unwrap();
        let flat = dir.path().join("flat");
        std::fs::create_dir(&flat).unwrap();
        write(&flat, "a.pdf", b"%PDF-1.4\n");

        let info = inspect(&flat, 100);
        assert!(info.is_dir, "a directory must be flagged, not just refused");
        assert!(!info.supported);
        assert!(!info.has_subfolders, "no subfolders → no FR-004 question");
        assert!(info.reason.as_deref().unwrap().contains("Folder"));

        let nested = dir.path().join("nested");
        std::fs::create_dir_all(nested.join("inner")).unwrap();
        let info = inspect(&nested, 100);
        assert!(info.is_dir);
        assert!(info.has_subfolders, "subfolders → ask recurse-or-not");
    }

    #[test]
    fn inspect_size_limit_wins_over_format() {
        let dir = tempfile::tempdir().unwrap();
        // 2 MB against a 1 MB ceiling, with a perfectly printable extension.
        let big = write(dir.path(), "big.pdf", &vec![b'x'; 2 * 1024 * 1024]);
        let info = inspect(&big, 1);
        assert!(!info.supported);
        assert_eq!(info.reason.as_deref(), Some("Larger than the 1 MB limit"));
        // FR-092: the stable code the UI translates rides along.
        assert_eq!(info.reason_code, Some(RejectReason::TooLarge));
    }

    #[test]
    fn fr_052_to_057_inspect_accepts_the_converted_formats() {
        let dir = tempfile::tempdir().unwrap();
        let heic = write(dir.path(), "photo.HEIC", b"\0\0\0\x18ftypheic");
        let info = inspect(&heic, 100);
        assert!(info.supported, "reason: {:?}", info.reason);
        assert_eq!(info.ext, "heic", "extension is lowercased");
        assert_eq!(info.pages, Some(1), "a photo is one page");

        for name in [
            "a.docx", "a.xlsx", "a.pptx", "a.odt", "a.ods", "a.odp", "a.doc", "a.rtf", "a.txt",
            "a.html", "a.htm", "a.md",
        ] {
            let f = write(dir.path(), name, b"content");
            let info = inspect(&f, 100);
            assert!(info.supported, "{name}: {:?}", info.reason);
            assert_eq!(info.pages, None, "{name}: pages unknown until converted");
        }

        // Anything else still gets the generic message.
        let exe = write(dir.path(), "setup.exe", b"MZ");
        assert_eq!(
            inspect(&exe, 100).reason.as_deref(),
            Some("Unsupported file type")
        );
        assert_eq!(
            inspect(&exe, 100).reason_code,
            Some(RejectReason::UnsupportedType)
        );

        // D9: a GIF is refused with its own reason.
        let gif = write(dir.path(), "anim.GIF", b"GIF89a");
        let info = inspect(&gif, 100);
        assert!(!info.supported);
        assert_eq!(info.reason_code, Some(RejectReason::GifUnsupported));
    }

    #[test]
    fn inspect_accepts_a_real_pdf_with_its_page_count() {
        let dir = tempfile::tempdir().unwrap();
        let pdf = one_page_pdf(dir.path(), "doc.pdf");
        let info = inspect(&pdf, 100);
        assert!(info.supported, "reason: {:?}", info.reason);
        assert_eq!(info.reason, None);
        assert_eq!(info.pages, Some(1));
        assert!(info.bytes > 0);
    }

    #[test]
    fn nfr_025_inspect_returns_the_canonical_path() {
        let dir = tempfile::tempdir().unwrap();
        let pdf = write(dir.path(), "doc.pdf", b"%PDF-1.4\n");
        let indirect = dir.path().join("sub").join("..").join("doc.pdf");
        std::fs::create_dir_all(dir.path().join("sub")).unwrap();
        let info = classify_one(&indirect.to_string_lossy(), 100);
        assert!(info.supported);
        assert_eq!(
            std::fs::canonicalize(&info.path).unwrap(),
            std::fs::canonicalize(&pdf).unwrap(),
            "the caller must submit exactly what was inspected"
        );
    }

    // -- page-range bounds (FR-042) ---------------------------------------

    #[test]
    fn fr_042_range_past_the_document_is_rejected_by_name() {
        let dir = tempfile::tempdir().unwrap();
        let pdf = one_page_pdf(dir.path(), "report.pdf");
        let files = vec![pdf.to_string_lossy().into_owned()];

        let err = check_ranges_fit_documents("2-3", &files).unwrap_err();
        // FR-092: the UI's sentence gets the file name and a numeric count
        // for its plural; the English detail keeps naming both.
        assert_eq!(err.reason, "page_range_outside");
        assert_eq!(err.params["name"], "report.pdf");
        assert_eq!(err.params["span"], "2-3");
        assert_eq!(err.params["count"], "1");
        let err = err.message;
        assert!(err.starts_with("report.pdf:"), "names the file: {err}");
        assert!(err.contains("1 page"), "names the document length: {err}");

        assert!(check_ranges_fit_documents("1", &files).is_ok());
    }

    #[test]
    fn fr_042_images_never_fail_the_bounds_check() {
        // The engine drops the range for images, so it must not gate them —
        // otherwise one PNG would make "1-5" unusable for the whole batch.
        let dir = tempfile::tempdir().unwrap();
        let png = write(dir.path(), "chart.png", b"\x89PNG\r\n\x1a\n");
        let files = vec![png.to_string_lossy().into_owned()];
        assert!(check_ranges_fit_documents("2-3", &files).is_ok());
    }

    #[test]
    fn fr_042_unprobeable_pdf_passes_through() {
        // Junk named .pdf: lopdf will not parse it, so the page count is
        // unknown and the range cannot be bounded.
        let dir = tempfile::tempdir().unwrap();
        let pdf = write(dir.path(), "weird.pdf", b"%PDF-1.4 but not really");
        let files = vec![pdf.to_string_lossy().into_owned()];
        assert!(check_ranges_fit_documents("40-50", &files).is_ok());
    }

    #[test]
    fn fr_042_missing_file_is_left_to_submit() {
        let dir = tempfile::tempdir().unwrap();
        let files = vec![dir.path().join("ghost.pdf").to_string_lossy().into_owned()];
        assert!(check_ranges_fit_documents("40-50", &files).is_ok());
    }

    #[test]
    fn fr_042_bad_grammar_is_reported_before_any_file_is_read() {
        assert!(check_ranges_fit_documents("5-1", &[]).is_err());
    }

    // -- preview printer resolution ---------------------------------------

    fn prefs(default: Option<&str>, routes: &[(&str, &str)]) -> crate::settings::PrinterPrefs {
        let mut prefs = crate::settings::PrinterPrefs {
            default_printer: default.map(str::to_string),
            ..Default::default()
        };
        for (ext, printer) in routes {
            prefs
                .routes
                .insert((*ext).to_string(), (*printer).to_string());
        }
        prefs
    }

    #[test]
    fn preview_printer_follows_request_then_app_then_os() {
        let p = prefs(Some("Office"), &[]);
        assert_eq!(
            resolve_preview_printer(&p, Some("Front Desk"), "a.pdf", Some("OS")),
            "Front Desk"
        );
        assert_eq!(
            resolve_preview_printer(&p, None, "a.pdf", Some("OS")),
            "Office"
        );
        let empty = prefs(None, &[]);
        assert_eq!(
            resolve_preview_printer(&empty, None, "a.pdf", Some("OS")),
            "OS"
        );
        assert_eq!(
            resolve_preview_printer(&empty, None, "a.pdf", None),
            "<default printer>"
        );
    }

    #[test]
    fn preview_printer_treats_empty_strings_as_unset() {
        let p = prefs(Some(""), &[]);
        assert_eq!(
            resolve_preview_printer(&p, Some(""), "a.pdf", Some("OS")),
            "OS",
            "a blank config value must not beat the OS default"
        );
        assert_eq!(
            resolve_preview_printer(&p, Some(""), "a.pdf", Some("")),
            "<default printer>"
        );
    }

    /// The panel used to skip file-type routing entirely, so a ".pdf → EPSON"
    /// rule previewed `lp -d HL_L2350` for a job that then printed on the EPSON.
    /// The preview must follow the *same* chain the dispatcher does, per file.
    #[test]
    fn fr_preview_printer_honours_file_type_routing_like_the_dispatcher() {
        let p = prefs(Some("HL_L2350"), &[(".pdf", "EPSON_L15160")]);

        // The routed type previews (and prints) on the routed printer.
        assert_eq!(
            resolve_preview_printer(&p, None, "Q3 Report.pdf", Some("OS")),
            "EPSON_L15160"
        );
        // A type with no rule still falls through to the app default.
        assert_eq!(
            resolve_preview_printer(&p, None, "notes.docx", Some("OS")),
            "HL_L2350"
        );
        // An explicit choice still beats the rule, exactly as at dispatch.
        assert_eq!(
            resolve_preview_printer(&p, Some("Front Desk"), "Q3 Report.pdf", Some("OS")),
            "Front Desk"
        );
        // And this is the very chain the print path uses — not a copy of it.
        assert_eq!(
            crate::ipc::printers::resolve_app_printer(&p, None, "Q3 Report.pdf").as_deref(),
            Some("EPSON_L15160")
        );
    }
}
