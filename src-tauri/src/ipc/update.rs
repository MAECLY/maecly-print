//! Update-check IPC command.
//!
//! [`check_for_updates`] resolves the project's GitHub slug, fetches the latest
//! release, and folds it against the running version into an [`UpdateCheck`].
//! There is no download/install here — the frontend surfaces the link and the
//! user decides.

use crate::update::{self, UpdateCheck};
use crate::util::error::IpcError;

/// Check GitHub for a newer release than the running build.
///
/// Runs off the IPC thread (a network round-trip). A misconfigured repository or
/// any network/rate-limit failure surfaces as [`IpcError::UpdateCheckFailed`]
/// with a short, user-facing message.
#[tauri::command]
#[specta::specta]
pub async fn check_for_updates() -> Result<UpdateCheck, IpcError> {
    super::run_blocking(|| {
        let (owner, repo) = update::repo_slug()
            .ok_or_else(|| IpcError::UpdateCheckFailed("repository not configured".to_string()))?;

        // The configured channel is read but does not change the endpoint yet:
        // every check hits the single `releases/latest` feed because separate
        // beta/nightly feeds don't exist in this version.
        let _channel = crate::settings::Config::load_or_default().updater.channel;

        let release = update::fetch_latest(&owner, &repo).map_err(IpcError::UpdateCheckFailed)?;
        Ok(update::evaluate(
            env!("CARGO_PKG_VERSION"),
            release,
            crate::queue::job::now_rfc3339(),
        ))
    })
    .await
}
