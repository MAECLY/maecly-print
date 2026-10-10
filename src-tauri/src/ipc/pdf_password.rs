//! FR-059: the password prompt for encrypted PDFs.
//!
//! A job whose PDF needs a password pauses with `pdf_protected` /
//! `password_required` ([`crate::queue::job::Job::needs_password`]). The GUI
//! asks the user for it (per job, optionally for the whole batch) and sends it
//! here; the dispatcher keeps it in memory only and resumes the job. It is
//! never written to `queue.json`, a profile or a log (NFR-082), so after a
//! restart the job simply asks again.

use tauri::State;

use crate::engine::PdfPassword;
use crate::queue::QueueHandle;
use crate::util::error::{IpcError, IpcErrorCode};

/// Supply the password of job `id`'s encrypted PDF and resume it. With
/// `apply_to_batch`, the same password is tried on the batch's other
/// protected PDFs too. A wrong password pauses the job again with
/// `password_incorrect`.
#[tauri::command]
#[specta::specta]
pub async fn job_supply_password(
    state: State<'_, QueueHandle>,
    id: String,
    password: String,
    apply_to_batch: bool,
) -> Result<(), IpcError> {
    let Some(password) = PdfPassword::new(password) else {
        return Err(IpcError::localized(
            IpcErrorCode::InvalidRequest,
            "password_empty",
            "no password was entered",
        ));
    };
    state.supply_password(id, password, apply_to_batch).await
}
