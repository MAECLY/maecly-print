//! Print-queue IPC commands.
//!
//! Thin Tauri command wrappers over [`crate::queue::QueueHandle`] (held as
//! managed state) plus two stateless helpers the print dialog needs before it
//! submits: [`inspect_files`] (per-file metadata for the file list) and
//! [`preview_command`] (the honest "this is what we'll run" string). The queue
//! itself pushes updates via the events in [`crate::ipc::events`]; these
//! commands are the request/response half of the contract in
//! `docs/04-architecture/ipc-and-cli.md`.

use std::path::Path;

use serde::Serialize;
use specta::Type;
use tauri::State;

use crate::queue::job::{JobBatch, PrintRequest, QueueSnapshot};
use crate::queue::QueueHandle;
use crate::util::error::IpcError;

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

/// Submit a print request. Validates and fans the files out into jobs, returning
/// the batch id and per-file job ids. Runs the (blocking) validation/probe off
/// the IPC thread.
#[tauri::command]
#[specta::specta]
pub async fn print_files(
    state: State<'_, QueueHandle>,
    req: PrintRequest,
) -> Result<JobBatch, IpcError> {
    let handle = state.inner().clone();
    run_blocking(move || handle.submit(req)).await
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
    run_blocking(move || Ok(paths.iter().map(|p| inspect_one(p)).collect())).await
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

/// Classify one path for [`inspect_files`].
fn inspect_one(path_str: &str) -> FileInfo {
    let path = Path::new(path_str);
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
            let supported = crate::engine::is_supported(&ext);
            let reason = if supported {
                None
            } else {
                Some("Unsupported file type".to_string())
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
