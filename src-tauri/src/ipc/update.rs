//! Update-check IPC commands.
//!
//! [`check_for_updates`] resolves the project's GitHub slug, fetches the latest
//! release, and folds it against the running version into an [`UpdateCheck`].
//! There is no download/install here — the frontend surfaces the link and the
//! user decides. FR-082 (signature verification) and FR-083 (per-user install)
//! need a signed installer and land with the auto-updater (M4); the UI says so
//! rather than pretending otherwise.
//!
//! [`get_update_state`] and [`set_skipped_update_version`] back the FR-084
//! decision the app *can* honour today: "skip this version". The state itself
//! lives in [`crate::update::UpdateState`].
//!
//! FR-080's launch-time check is scheduled from `lib.rs`'s `setup`
//! ([`crate::update::run_startup_check`] after
//! [`crate::update::STARTUP_CHECK_DELAY`]) and reaches the UI as the
//! [`UpdateAvailable`] event declared below — the push counterpart to
//! [`check_for_updates`], for the user who is not on the About screen.

use serde::Serialize;

use crate::update::{self, UpdateCheck, UpdateState};
use crate::util::error::IpcError;

/// A launch-time check found a release worth mentioning (FR-080).
///
/// Emitted **only** when [`crate::update::run_startup_check`] returns a result:
/// the preference is on, the 24h window had elapsed, the release is strictly
/// newer, and it is not the one the user skipped (FR-084). Silence is the
/// normal case, so the frontend never has to filter this event — receiving it
/// is already the decision to tell the user.
///
/// Lives here rather than in [`crate::ipc::events`] because it belongs to the
/// update flow, the way `cli::pending::SelectionForwarded` belongs to the CLI
/// one; the event name is the kebab-cased struct name, `"update-available"`.
#[derive(Debug, Clone, Serialize, specta::Type, tauri_specta::Event)]
pub struct UpdateAvailable(pub UpdateCheck);

/// Check GitHub for a newer release than the running build.
///
/// Runs off the IPC thread (a network round-trip). A misconfigured repository or
/// any network/rate-limit failure surfaces as [`IpcError::UpdateCheckFailed`]
/// with a short, user-facing message.
///
/// A successful check stamps [`UpdateState::last_check_at`], so a manual check
/// postpones the next automatic one instead of stacking with it (FR-080). A
/// failure to persist that stamp is logged, never surfaced: the user asked for
/// a check, and they got one.
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
        let check = update::evaluate(
            env!("CARGO_PKG_VERSION"),
            release,
            crate::queue::job::now_rfc3339(),
        );

        let mut state = UpdateState::load();
        state.last_check_at = Some(check.checked_at.clone());
        if let Err(e) = state.save() {
            tracing::warn!(event = "update_state_save_failed", error = %e);
        }

        Ok(check)
    })
    .await
}

/// The persisted update bookkeeping: when the last check ran and which version
/// the user skipped (FR-084), so the About screen can render "you skipped
/// {version}" with a way back out. A skip that is no longer ahead of the
/// running build is cleared on the way out.
#[tauri::command]
#[specta::specta]
pub fn get_update_state() -> Result<UpdateState, IpcError> {
    // A skip at or below the running version is history (the user upgraded
    // past it), so it is dropped here rather than shown forever.
    Ok(UpdateState::load_current_from(
        &update::state_path(),
        env!("CARGO_PKG_VERSION"),
    ))
}

/// Record (or clear) the release the user chose to skip — FR-084's third
/// option, next to "install now" and "remind me later".
///
/// `None` clears the skip, which is how the UI offers an undo. The value is
/// stored verbatim (a release tag, `v1.2.0` or `1.2.0`); the comparison that
/// decides whether to stay silent is version-aware, not textual, so either form
/// works. Skipping is per-version: a *newer* release prompts again.
#[tauri::command]
#[specta::specta]
pub fn set_skipped_update_version(version: Option<String>) -> Result<(), IpcError> {
    let mut state = UpdateState::load();
    state.skipped_version = version
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty());
    state
        .save()
        .map_err(|e| IpcError::Internal(format!("could not save the update state: {e}")))
}
