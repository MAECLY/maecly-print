//! IPC error type.
//!
//! Serialized identically to both webview and CLI (when wired in M1).
//! Schema: `{"code": "<Variant>", "message": "<inner string>"}` — note that the
//! serialized `message` is the variant's payload (via `#[serde(content)]`), not
//! the `#[error(..)]` `Display` string. The frontend matches on the `code`
//! strings, so existing variants must never be renamed.
//! Mirrors the contract documented in `docs/04-architecture/ipc-and-cli.md` §4.

use serde::Serialize;
use specta::Type;
use thiserror::Error;

#[derive(Debug, Error, Serialize, Type)]
#[serde(tag = "code", content = "message")]
pub enum IpcError {
    #[error("File not found: {0}")]
    FileNotFound(String),

    /// The caller sent a structurally invalid request (empty file list, bad
    /// copies count, unparseable page range, …). The message is safe to show
    /// inline in the UI.
    #[error("Invalid request: {0}")]
    InvalidRequest(String),

    /// A file's format is not one the engine can print.
    #[error("Unsupported file format: {0}")]
    UnsupportedFormat(String),

    /// A converter could not turn the input into a printable PDF.
    #[error("Conversion failed: {0}")]
    ConversionFailed(String),

    /// The OS spooler/printer backend rejected the job.
    #[error("Printing failed: {0}")]
    PrintFailed(String),

    /// A requested print setting the driver cannot honour (currently duplex).
    #[error("Unsupported setting: {0}")]
    UnsupportedSetting(String),

    /// A required external program (CUPS `lp`, SumatraPDF) is missing.
    #[error("Missing dependency: {0}")]
    DependencyMissing(String),

    /// No job with the given id exists in the queue.
    #[error("Job not found: {0}")]
    JobNotFound(String),

    /// The action is not legal for the job's current state (e.g. pausing a job
    /// that is already printing, or retrying one that has not failed).
    #[error("Invalid state: {0}")]
    InvalidState(String),

    /// No profile with the given id exists (get/delete/set-default of an
    /// unknown profile). Distinct from [`IpcError::InvalidRequest`] so the UI
    /// can special-case a stale id (e.g. deleted in another window).
    #[error("Profile not found: {0}")]
    ProfileNotFound(String),

    /// The GitHub-release update check could not complete (repository not
    /// configured, network error, rate limit). The message is user-facing.
    #[error("Update check failed: {0}")]
    UpdateCheckFailed(String),

    /// Downloading or installing an update failed (network, disk, installer,
    /// or a copy of the app that cannot update itself). The message is
    /// user-facing.
    #[error("Update install failed: {0}")]
    UpdateInstallFailed(String),

    /// FR-082: the downloaded update did not carry a valid signature from the
    /// release key, so it was rejected and **nothing was installed**. Its own
    /// code so the UI can say exactly that.
    #[error("Update signature invalid: {0}")]
    UpdateSignatureInvalid(String),

    #[error("Internal error: {0}")]
    Internal(String),
}

impl From<std::io::Error> for IpcError {
    fn from(err: std::io::Error) -> Self {
        IpcError::Internal(err.to_string())
    }
}

impl From<anyhow::Error> for IpcError {
    fn from(err: anyhow::Error) -> Self {
        IpcError::Internal(err.to_string())
    }
}

impl From<crate::engine::EngineError> for IpcError {
    /// Map an engine failure onto the IPC boundary.
    ///
    /// Each engine variant has a matching `IpcError` code so the frontend can
    /// distinguish (e.g.) a conversion failure from a missing dependency.
    /// [`EngineError::Cancelled`](crate::engine::EngineError::Cancelled) has no
    /// dedicated code — cancellation is a control-flow outcome rather than a
    /// user-facing error — so it maps to [`IpcError::InvalidState`] with a plain
    /// "job cancelled" message; in practice the dispatcher intercepts
    /// cancellation before it ever reaches a command return value, so this arm
    /// is defensive.
    fn from(err: crate::engine::EngineError) -> Self {
        use crate::engine::EngineError as E;
        match err {
            E::ConversionFailed(m) => IpcError::ConversionFailed(m),
            E::PrintFailed(m) => IpcError::PrintFailed(m),
            E::UnsupportedSetting { setting, printer } => IpcError::UnsupportedSetting(format!(
                "the driver for '{printer}' does not support {setting}"
            )),
            E::DependencyMissing(m) => IpcError::DependencyMissing(m),
            E::UnsupportedFormat(m) => IpcError::UnsupportedFormat(m),
            E::Cancelled => IpcError::InvalidState("job cancelled".to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::EngineError;

    /// The serialized `message` must carry the payload string, keyed by the
    /// stable `code` the frontend switches on.
    #[test]
    fn serializes_code_and_message() {
        let v = serde_json::to_value(IpcError::JobNotFound("abc".into())).unwrap();
        assert_eq!(v["code"], "JobNotFound");
        assert_eq!(v["message"], "abc");

        let v = serde_json::to_value(IpcError::InvalidState("cannot pause".into())).unwrap();
        assert_eq!(v["code"], "InvalidState");
        assert_eq!(v["message"], "cannot pause");
    }

    #[test]
    fn engine_error_maps_to_matching_codes() {
        let cases = [
            (
                EngineError::ConversionFailed("x".into()),
                "ConversionFailed",
            ),
            (EngineError::PrintFailed("x".into()), "PrintFailed"),
            (
                EngineError::DependencyMissing("x".into()),
                "DependencyMissing",
            ),
            (
                EngineError::UnsupportedFormat("x".into()),
                "UnsupportedFormat",
            ),
        ];
        for (engine_err, expected_code) in cases {
            let ipc: IpcError = engine_err.into();
            let v = serde_json::to_value(ipc).unwrap();
            assert_eq!(v["code"], expected_code);
        }
    }

    #[test]
    fn unsupported_setting_names_setting_and_printer() {
        let ipc: IpcError = EngineError::UnsupportedSetting {
            setting: "duplex".into(),
            printer: "Front Desk".into(),
        }
        .into();
        match ipc {
            IpcError::UnsupportedSetting(msg) => {
                assert!(msg.contains("duplex"), "message names the setting: {msg}");
                assert!(
                    msg.contains("Front Desk"),
                    "message names the printer: {msg}"
                );
            }
            other => panic!("expected UnsupportedSetting, got {other:?}"),
        }
    }

    #[test]
    fn cancelled_maps_to_invalid_state() {
        let ipc: IpcError = EngineError::Cancelled.into();
        assert!(matches!(ipc, IpcError::InvalidState(_)));
    }
}
