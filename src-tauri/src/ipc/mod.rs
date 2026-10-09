pub mod diag;
pub mod events;
pub mod jobs;
pub mod notify;
pub mod printers;
pub mod profiles;
pub mod settings;
pub mod update;

use crate::util::error::IpcError;

/// Run a blocking closure off the IPC thread, normalizing a task panic to
/// [`IpcError::Internal`]. Shared by the settings/profiles/update commands,
/// whose closures already produce typed [`IpcError`]s (unlike the printer
/// commands' `anyhow`-based helper). Any `config.toml`/`profiles.toml` lock is
/// taken *inside* the closure, so a guard is never held across an `.await`.
pub(crate) async fn run_blocking<T, F>(f: F) -> Result<T, IpcError>
where
    F: FnOnce() -> Result<T, IpcError> + Send + 'static,
    T: Send + 'static,
{
    match tauri::async_runtime::spawn_blocking(f).await {
        Ok(r) => r,
        Err(e) => Err(IpcError::Internal(format!("task panicked: {e}"))),
    }
}
