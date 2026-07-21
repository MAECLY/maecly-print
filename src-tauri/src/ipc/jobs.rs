//! Print-queue IPC commands.
//!
//! Thin Tauri command wrappers over [`crate::queue::QueueHandle`] (held as
//! managed state) plus two stateless helpers the print dialog needs before it
//! submits: [`inspect_files`] (per-file metadata for the file list) and
//! [`preview_command`] (the honest "this is what we'll run" string), plus the
//! two-step folder flow a drop of a directory needs — [`classify_paths`] tells
//! the UI *which* of the dropped entries are folders so it can ask the
//! recurse-or-not question (FR-004), [`expand_folders`] then walks them with
//! the answer. The queue
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
    pub path: String,
    pub name: String,
    /// Lowercased extension without the dot (e.g. `"pdf"`), empty when none.
    pub ext: String,
    pub bytes: u64,
    pub pages: Option<u32>,
    pub supported: bool,
    /// Why the file is not printable, when `supported` is false.
    pub reason: Option<String>,
}

/// A folder the caller dropped or picked, waiting on the FR-004 question.
#[derive(Debug, Clone, Serialize, Type)]
pub struct FolderCandidate {
    /// Canonical path of the folder.
    pub path: String,
    /// Folder name, for the prompt.
    pub name: String,
    /// Whether it holds subfolders at all — when it does not, the UI can skip
    /// the recurse-or-not question entirely and expand top-level.
    pub has_subfolders: bool,
}

/// A dropped entry that could not be resolved (relative, missing, unreadable).
/// Reported instead of thrown so one bad path never voids a whole drop
/// (FR-059).
#[derive(Debug, Clone, Serialize, Type)]
pub struct RejectedPath {
    /// The path exactly as the caller sent it.
    pub path: String,
    /// User-facing explanation.
    pub reason: String,
}

/// A resolved drop/selection, split into the three things the UI does next:
/// print the files, ask about the folders, complain about the rest.
#[derive(Debug, Clone, Serialize, Type)]
pub struct PathSelection {
    /// Canonical paths of the entries that are plain files.
    pub files: Vec<String>,
    /// Folders the caller must run through [`expand_folders`] first.
    pub folders: Vec<FolderCandidate>,
    /// Entries that resolved to nothing usable.
    pub rejected: Vec<RejectedPath>,
}

/// Submit a print request. Validates and fans the files out into jobs, returning
/// the batch id and per-file job ids. Runs the (blocking) validation/probe off
/// the IPC thread.
///
/// Every path is resolved through [`canonical_input`] first (NFR-025), so the
/// queue only ever stores absolute, symlink-resolved paths and a relative one
/// can never be silently resolved against the process' working directory.
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
        handle.submit(req)
    })
    .await
}

/// Resolve a drag-and-drop / file-picker selection and split files from folders
/// (FR-004, NFR-025).
///
/// The first half of the folder flow: the UI calls this with whatever the OS
/// handed it, and asks the recurse-or-not question only when `folders` is not
/// empty (and only when a folder reports `has_subfolders`). Never errors — a
/// path that will not resolve lands in `rejected` with a reason.
#[tauri::command]
#[specta::specta]
pub async fn classify_paths(paths: Vec<String>) -> Result<PathSelection, IpcError> {
    run_blocking(move || Ok(classify_selection(&paths))).await
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

/// Path-injected core of [`classify_paths`]: pure resolution, no config, no
/// Tauri state, so it is directly testable against a tempdir.
pub fn classify_selection(paths: &[String]) -> PathSelection {
    let mut out = PathSelection {
        files: Vec::new(),
        folders: Vec::new(),
        rejected: Vec::new(),
    };
    for raw in paths {
        let resolved = match canonical_input(raw) {
            Ok(p) => p,
            Err(e) => {
                out.rejected.push(RejectedPath {
                    path: raw.clone(),
                    reason: e.to_string(),
                });
                continue;
            }
        };
        let display = resolved.to_string_lossy().into_owned();
        if resolved.is_dir() {
            out.folders.push(FolderCandidate {
                name: resolved
                    .file_name()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_else(|| display.clone()),
                has_subfolders: has_subfolders(&resolved),
                path: display,
            });
        } else if resolved.is_file() {
            out.files.push(display);
        } else {
            out.rejected.push(RejectedPath {
                path: raw.clone(),
                reason: "Not a file or folder".to_string(),
            });
        }
    }
    out
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

/// The exact command MaeclyPrint would run for `req` (honesty policy: no hidden
/// flags). Resolves the printer name the same way the dispatcher will —
/// requested, else the app default, else the OS default, else a
/// `<default printer>` placeholder — and previews against the first file's name.
#[tauri::command]
#[specta::specta]
pub async fn preview_command(req: PrintRequest) -> Result<String, IpcError> {
    run_blocking(move || {
        let printer = req
            .settings
            .printer
            .clone()
            .filter(|s| !s.is_empty())
            .or_else(|| {
                crate::settings::Config::load_or_default()
                    .printers
                    .default_printer
                    .filter(|s| !s.is_empty())
            })
            .or_else(|| {
                crate::platform::default_printer()
                    .ok()
                    .flatten()
                    .filter(|s| !s.is_empty())
            })
            .unwrap_or_else(|| "<default printer>".to_string());

        let display = req
            .files
            .first()
            .map(|f| {
                Path::new(f)
                    .file_name()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_else(|| f.clone())
            })
            .unwrap_or_else(|| "document.pdf".to_string());

        Ok(crate::engine::print::preview_command(
            &display,
            &printer,
            &req.settings,
        ))
    })
    .await
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
    let resolved = match canonical_input(path_str) {
        Ok(p) => p,
        Err(e) => {
            let path = Path::new(path_str);
            return FileInfo {
                name: path
                    .file_name()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_else(|| path_str.to_string()),
                ext: crate::engine::extension_of(path),
                path: path_str.to_string(),
                bytes: 0,
                pages: None,
                supported: false,
                reason: Some(match e {
                    IpcError::FileNotFound(_) => "File not found".to_string(),
                    other => other.to_string(),
                }),
            };
        }
    };
    let path = resolved.as_path();
    let canonical = resolved.to_string_lossy().into_owned();
    let path_str = canonical.as_str();
    let name = path
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| path_str.to_string());
    let ext = crate::engine::extension_of(path);

    match std::fs::metadata(path) {
        Err(_) => FileInfo {
            path: path_str.to_string(),
            name,
            ext,
            bytes: 0,
            pages: None,
            supported: false,
            reason: Some("File not found".to_string()),
        },
        Ok(m) if m.is_dir() => FileInfo {
            path: path_str.to_string(),
            name,
            ext,
            bytes: 0,
            pages: None,
            supported: false,
            // FR-004: folders are legal input, they just have to go through
            // `expand_folders` before they reach the file list.
            reason: Some("Folder — expand it to print the files inside".to_string()),
        },
        Ok(m) if !m.is_file() => FileInfo {
            path: path_str.to_string(),
            name,
            ext,
            bytes: 0,
            pages: None,
            supported: false,
            reason: Some("Not a file".to_string()),
        },
        Ok(m) => {
            let (supported, reason) = if crate::queue::exceeds_size_limit(m.len(), max_mb) {
                (false, Some(format!("Larger than the {max_mb} MB limit")))
            } else if crate::engine::is_supported(&ext) {
                (true, None)
            } else {
                (false, Some("Unsupported file type".to_string()))
            };
            let pages = crate::queue::probe_pages(path, &ext, m.len());
            FileInfo {
                path: path_str.to_string(),
                name,
                ext,
                bytes: m.len(),
                pages,
                supported,
                reason,
            }
        }
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
