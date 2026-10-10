//! FR-058: guided installation of a missing dependency (winget, Homebrew, the
//! Microsoft Store or the vendor's web page), always after the user's explicit
//! consent, never silently (decision D13).
//!
//! **Stub (round-3 contract, owned by lane L7).** The command is registered so
//! L7 fills it in without touching `lib.rs`; its arguments are L7's to shape.

use crate::util::error::IpcError;

/// Install the dependency `id` (its serialized
/// [`crate::engine::deps::Dependency`] id) the way the user agreed to.
#[tauri::command]
#[specta::specta]
pub async fn install_dependency(id: String) -> Result<(), IpcError> {
    Err(IpcError::Internal(format!(
        "installing {id} from Pluryprint is not implemented yet"
    )))
}
