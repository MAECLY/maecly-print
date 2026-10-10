//! IPC error type.
//!
//! Serialized identically to both webview and CLI.
//! Schema: `{"code": "<Variant>", "message": "<inner string>", "reason"?: "<stable_reason>",
//! "params"?: {"<name>": "<value>"}}`.
//!
//! - `code` is the stable error family (the variant name). The frontend matches
//!   on it, so existing codes must never be renamed.
//! - `message` is the variant's payload (English detail for logs and the CLI),
//!   not the `#[error(..)]` `Display` string.
//! - `reason` + `params` are present only on a [`LocalizedError`]: a stable,
//!   machine-readable sub-code within the family plus its interpolation values,
//!   which the UI maps to a translated sentence (FR-092). `reason` strings are
//!   part of the contract too and must never be renamed.
//!
//! Mirrors the contract documented in `docs/04-architecture/ipc-and-cli.md` §4.

use std::collections::BTreeMap;

use serde::{Serialize, Serializer};
use specta::Type;
use thiserror::Error;

/// The stable error family of an [`IpcError`] — its `code` on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Type)]
pub enum IpcErrorCode {
    /// A named input file does not exist or cannot be read.
    FileNotFound,
    /// The request is structurally invalid (bad copies, page range, …).
    InvalidRequest,
    /// The engine cannot print this file format.
    UnsupportedFormat,
    /// A converter could not turn the input into a printable PDF.
    ConversionFailed,
    /// The OS spooler or printer backend rejected the job.
    PrintFailed,
    /// The driver cannot honour a requested setting (currently duplex).
    UnsupportedSetting,
    /// A required external program (CUPS `lp`, SumatraPDF) is missing.
    DependencyMissing,
    /// No queued job has the given id.
    JobNotFound,
    /// The action is not legal in the job's current state.
    InvalidState,
    /// No profile has the given id.
    ProfileNotFound,
    /// The GitHub-release update check could not complete.
    UpdateCheckFailed,
    /// Downloading or installing an update failed.
    UpdateInstallFailed,
    /// The downloaded update failed signature verification (FR-082).
    UpdateSignatureInvalid,
    /// FR-059: the PDF is encrypted or forbids printing. The reason says which
    /// (`password_required`, `password_incorrect`, `printing_not_allowed`,
    /// `unsupported_encryption`); the first two are fixed by supplying a
    /// password, the others are final.
    PdfProtected,
    /// A bug or app-state failure the user cannot fix by changing input.
    Internal,
}

impl IpcErrorCode {
    /// Every family, in declaration order (used by the catalog tests).
    pub const ALL: &'static [IpcErrorCode] = &[
        Self::FileNotFound,
        Self::InvalidRequest,
        Self::UnsupportedFormat,
        Self::ConversionFailed,
        Self::PrintFailed,
        Self::UnsupportedSetting,
        Self::DependencyMissing,
        Self::JobNotFound,
        Self::InvalidState,
        Self::ProfileNotFound,
        Self::UpdateCheckFailed,
        Self::UpdateInstallFailed,
        Self::UpdateSignatureInvalid,
        Self::PdfProtected,
        Self::Internal,
    ];

    /// The English family prefix used by `Display` (logs, CLI human output).
    fn label(self) -> &'static str {
        match self {
            Self::FileNotFound => "File not found",
            Self::InvalidRequest => "Invalid request",
            Self::UnsupportedFormat => "Unsupported file format",
            Self::ConversionFailed => "Conversion failed",
            Self::PrintFailed => "Printing failed",
            Self::UnsupportedSetting => "Unsupported setting",
            Self::DependencyMissing => "Missing dependency",
            Self::JobNotFound => "Job not found",
            Self::InvalidState => "Invalid state",
            Self::ProfileNotFound => "Profile not found",
            Self::UpdateCheckFailed => "Update check failed",
            Self::UpdateInstallFailed => "Update install failed",
            Self::UpdateSignatureInvalid => "Update signature invalid",
            Self::PdfProtected => "Protected PDF",
            Self::Internal => "Internal error",
        }
    }
}

/// A failure with a stable, localizable `reason` (and interpolation `params`)
/// inside an error family. The English `message` stays the log/CLI detail and
/// the UI's last-resort fallback.
///
/// It is a plain `std::error::Error`, so `anyhow`-based layers (the platform
/// printer code) can return one and [`IpcError`]'s `From<anyhow::Error>`
/// recovers it intact instead of flattening it to `Internal`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalizedError {
    /// The family this failure belongs to; serialized as `code`.
    pub code: IpcErrorCode,
    /// Stable machine-readable sub-code the UI maps to a translated sentence.
    /// Part of the wire contract: never rename an existing value.
    pub reason: &'static str,
    /// Interpolation values for the translated sentence, keyed by placeholder name.
    pub params: BTreeMap<String, String>,
    /// English detail for logs and the CLI, and the UI's fallback when it has no
    /// translation for `reason`.
    pub message: String,
}

impl LocalizedError {
    /// A localized error of family `code` with `reason` and no params yet; add
    /// them with [`LocalizedError::param`].
    ///
    /// `reason` must be registered in [`crate::util::reasons`] (checked in
    /// debug builds), so it is sure to have a sentence in every language.
    pub fn new(code: IpcErrorCode, reason: &'static str, message: impl Into<String>) -> Self {
        debug_assert!(
            crate::util::reasons::is_ipc_reason(code, reason),
            "unregistered IPC reason {code:?}.{reason}: add it to util::reasons and the catalogs"
        );
        Self {
            code,
            reason,
            params: BTreeMap::new(),
            message: message.into(),
        }
    }

    /// Attach one interpolation value (builder style).
    pub fn param(mut self, name: &str, value: impl ToString) -> Self {
        self.params.insert(name.to_string(), value.to_string());
        self
    }
}

impl std::fmt::Display for LocalizedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code.label(), self.message)
    }
}

impl std::error::Error for LocalizedError {}

impl From<LocalizedError> for IpcError {
    fn from(err: LocalizedError) -> Self {
        IpcError::Localized(Box::new(err))
    }
}

/// Every failure a Tauri command or a CLI subcommand can report.
///
/// The variant name is the stable `code` on the wire (see the module docs);
/// the payload is the English `message`.
#[derive(Debug, Error)]
pub enum IpcError {
    /// A file the request named does not exist or cannot be read. The payload
    /// is the offending path.
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

    /// A failure with no more specific family: a bug, an I/O error on app
    /// state, a poisoned lock. Not something the user can fix by changing input.
    #[error("Internal error: {0}")]
    Internal(String),

    /// A failure of any family that carries a stable `reason` the UI can
    /// translate. Serializes with the family's own `code` (never "Localized").
    #[error("{0}")]
    Localized(Box<LocalizedError>),
}

impl IpcError {
    /// Shorthand for a [`LocalizedError`] of `code` with `reason`.
    pub fn localized(code: IpcErrorCode, reason: &'static str, message: impl Into<String>) -> Self {
        LocalizedError::new(code, reason, message).into()
    }

    /// Attach an interpolation value to a localized error (a no-op on a plain
    /// variant, which has no `params`).
    pub fn with_param(mut self, name: &str, value: impl ToString) -> Self {
        if let IpcError::Localized(l) = &mut self {
            l.params.insert(name.to_string(), value.to_string());
        }
        self
    }

    /// The stable family code (the wire `code`).
    pub fn code(&self) -> IpcErrorCode {
        match self {
            IpcError::FileNotFound(_) => IpcErrorCode::FileNotFound,
            IpcError::InvalidRequest(_) => IpcErrorCode::InvalidRequest,
            IpcError::UnsupportedFormat(_) => IpcErrorCode::UnsupportedFormat,
            IpcError::ConversionFailed(_) => IpcErrorCode::ConversionFailed,
            IpcError::PrintFailed(_) => IpcErrorCode::PrintFailed,
            IpcError::UnsupportedSetting(_) => IpcErrorCode::UnsupportedSetting,
            IpcError::DependencyMissing(_) => IpcErrorCode::DependencyMissing,
            IpcError::JobNotFound(_) => IpcErrorCode::JobNotFound,
            IpcError::InvalidState(_) => IpcErrorCode::InvalidState,
            IpcError::ProfileNotFound(_) => IpcErrorCode::ProfileNotFound,
            IpcError::UpdateCheckFailed(_) => IpcErrorCode::UpdateCheckFailed,
            IpcError::UpdateInstallFailed(_) => IpcErrorCode::UpdateInstallFailed,
            IpcError::UpdateSignatureInvalid(_) => IpcErrorCode::UpdateSignatureInvalid,
            IpcError::Internal(_) => IpcErrorCode::Internal,
            IpcError::Localized(l) => l.code,
        }
    }

    /// The stable reason, when this is a [`LocalizedError`].
    pub fn reason(&self) -> Option<&'static str> {
        match self {
            IpcError::Localized(l) => Some(l.reason),
            _ => None,
        }
    }

    /// The wire `message`: the payload, not the `Display` string.
    pub fn message(&self) -> &str {
        match self {
            IpcError::FileNotFound(m)
            | IpcError::InvalidRequest(m)
            | IpcError::UnsupportedFormat(m)
            | IpcError::ConversionFailed(m)
            | IpcError::PrintFailed(m)
            | IpcError::UnsupportedSetting(m)
            | IpcError::DependencyMissing(m)
            | IpcError::JobNotFound(m)
            | IpcError::InvalidState(m)
            | IpcError::ProfileNotFound(m)
            | IpcError::UpdateCheckFailed(m)
            | IpcError::UpdateInstallFailed(m)
            | IpcError::UpdateSignatureInvalid(m)
            | IpcError::Internal(m) => m,
            IpcError::Localized(l) => &l.message,
        }
    }

    fn to_wire(&self) -> wire::IpcError {
        let (reason, params) = match self {
            IpcError::Localized(l) => (Some(l.reason.to_string()), l.params.clone()),
            _ => (None, BTreeMap::new()),
        };
        wire::IpcError {
            code: self.code(),
            message: self.message().to_string(),
            reason,
            params: (!params.is_empty()).then_some(params),
        }
    }
}

/// The serialized shape. A separate type so the Rust enum stays ergonomic to
/// construct and match while the wire stays one flat, stable object; it is
/// what the generated `IpcError` TypeScript type describes.
mod wire {
    use std::collections::BTreeMap;

    use serde::Serialize;
    use specta::Type;

    use super::IpcErrorCode;

    #[derive(Serialize, Type)]
    pub struct IpcError {
        /// Stable error family. Never renamed.
        pub code: IpcErrorCode,
        /// English detail: logs, the CLI, and the UI's fallback for an
        /// unknown code.
        pub message: String,
        /// Stable machine reason within `code` (e.g. `"rate_limited"`); the UI
        /// translates `code` + `reason` with `params`. Never renamed.
        #[serde(skip_serializing_if = "Option::is_none")]
        pub reason: Option<String>,
        /// Interpolation values for the translated `reason` sentence.
        #[serde(skip_serializing_if = "Option::is_none")]
        pub params: Option<BTreeMap<String, String>>,
    }
}

impl Serialize for IpcError {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.to_wire().serialize(serializer)
    }
}

impl Type for IpcError {
    fn inline(
        type_map: &mut specta::TypeCollection,
        generics: specta::Generics,
    ) -> specta::DataType {
        wire::IpcError::inline(type_map, generics)
    }

    fn reference(
        type_map: &mut specta::TypeCollection,
        generics: &[specta::DataType],
    ) -> specta::datatype::reference::Reference {
        wire::IpcError::reference(type_map, generics)
    }
}

impl From<std::io::Error> for IpcError {
    fn from(err: std::io::Error) -> Self {
        IpcError::Internal(err.to_string())
    }
}

impl From<anyhow::Error> for IpcError {
    /// A [`LocalizedError`] raised through `anyhow` keeps its code and reason,
    /// and so does an engine failure (an [`crate::engine::EngineError`], or
    /// the one inside a [`crate::engine::print::PrintError::Engine`], as the
    /// platform test page raises); anything else is an `Internal` failure.
    fn from(err: anyhow::Error) -> Self {
        use crate::engine::print::PrintError;
        let err = match err.downcast::<LocalizedError>() {
            Ok(localized) => return localized.into(),
            Err(other) => other,
        };
        let err = match err.downcast::<crate::engine::EngineError>() {
            Ok(engine) => return engine.into(),
            Err(other) => other,
        };
        match err.downcast::<PrintError>() {
            Ok(PrintError::Engine(engine)) => engine.into(),
            Ok(other) => IpcError::Internal(other.to_string()),
            Err(other) => IpcError::Internal(other.to_string()),
        }
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
        // A failure with a registered reason keeps it, in its own family, so
        // the UI (and the CLI's JSON) can name it precisely.
        let localized = |code: IpcErrorCode, f: crate::engine::Failure| match f.reason {
            Some(reason) => {
                let mut l = LocalizedError::new(code, reason, f.detail);
                l.params = f.params;
                IpcError::from(l)
            }
            None => plain(code, f.detail),
        };
        match err {
            E::ConversionFailed(f) | E::ConversionTimedOut(f) => {
                localized(IpcErrorCode::ConversionFailed, f)
            }
            E::PrintFailed(f) => localized(IpcErrorCode::PrintFailed, f),
            E::UnsupportedSetting { setting, printer } => IpcError::UnsupportedSetting(format!(
                "the driver for '{printer}' does not support {setting}"
            )),
            E::DependencyMissing(f) => localized(IpcErrorCode::DependencyMissing, f),
            E::UnsupportedFormat(f) => localized(IpcErrorCode::UnsupportedFormat, f),
            E::PdfProtected(p) => {
                let message = format!("protected PDF: {p}");
                let reason = p.reason();
                let l = LocalizedError::new(IpcErrorCode::PdfProtected, reason, message);
                match p {
                    crate::engine::PdfProtection::Unsupported(detail) => {
                        l.param("detail", detail).into()
                    }
                    _ => l.into(),
                }
            }
            E::Cancelled => {
                IpcError::localized(IpcErrorCode::InvalidState, "job_cancelled", "job cancelled")
            }
        }
    }
}

/// The plain (reason-less) variant of an engine family.
fn plain(code: IpcErrorCode, message: String) -> IpcError {
    match code {
        IpcErrorCode::ConversionFailed => IpcError::ConversionFailed(message),
        IpcErrorCode::PrintFailed => IpcError::PrintFailed(message),
        IpcErrorCode::DependencyMissing => IpcError::DependencyMissing(message),
        IpcErrorCode::UnsupportedFormat => IpcError::UnsupportedFormat(message),
        _ => IpcError::Internal(message),
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
    fn plain_errors_carry_no_reason_or_params_on_the_wire() {
        let v = serde_json::to_value(IpcError::Internal("boom".into())).unwrap();
        assert_eq!(
            v,
            serde_json::json!({"code": "Internal", "message": "boom"})
        );
    }

    /// FR-092: a localized error keeps its family `code`, adds the stable
    /// `reason` and `params`, and keeps the English detail as `message`.
    #[test]
    fn fr_092_localized_error_serializes_code_reason_and_params() {
        let err = IpcError::from(
            LocalizedError::new(
                IpcErrorCode::UpdateCheckFailed,
                "bad_status",
                "github.com returned an unexpected status (500)",
            )
            .param("status", 500),
        );
        assert_eq!(err.code(), IpcErrorCode::UpdateCheckFailed);
        assert_eq!(err.reason(), Some("bad_status"));
        let v = serde_json::to_value(&err).unwrap();
        assert_eq!(v["code"], "UpdateCheckFailed");
        assert_eq!(v["reason"], "bad_status");
        assert_eq!(v["params"]["status"], "500");
        assert_eq!(
            v["message"],
            "github.com returned an unexpected status (500)"
        );
        assert_eq!(
            err.to_string(),
            "Update check failed: github.com returned an unexpected status (500)"
        );
    }

    #[test]
    fn fr_092_localized_error_survives_an_anyhow_round_trip() {
        let any: anyhow::Error =
            LocalizedError::new(IpcErrorCode::DependencyMissing, "ippfind_missing", "x").into();
        let ipc = IpcError::from(any);
        assert_eq!(ipc.code(), IpcErrorCode::DependencyMissing);
        assert_eq!(ipc.reason(), Some("ippfind_missing"));

        let plain = IpcError::from(anyhow::anyhow!("nope"));
        assert_eq!(plain.code(), IpcErrorCode::Internal);
        assert_eq!(plain.reason(), None);
    }

    /// FR-092: an engine failure that travels through `anyhow` (the platform
    /// test page spools through the engine) keeps its family and reason.
    #[test]
    fn fr_092_engine_failure_survives_an_anyhow_round_trip() {
        use crate::engine::print::PrintError;
        use crate::engine::Failure;
        let spool: anyhow::Error = PrintError::Engine(EngineError::PrintFailed(
            Failure::new("sumatra_exit", "SumatraPDF exited with 1").param("status", 1),
        ))
        .into();
        let ipc = IpcError::from(spool);
        assert_eq!(ipc.code(), IpcErrorCode::PrintFailed);
        assert_eq!(ipc.reason(), Some("sumatra_exit"));

        let engine: anyhow::Error =
            EngineError::DependencyMissing(Failure::new("cups", "no lp")).into();
        let ipc = IpcError::from(engine);
        assert_eq!(ipc.code(), IpcErrorCode::DependencyMissing);
        assert_eq!(ipc.reason(), Some("cups"));

        let timeout: anyhow::Error = PrintError::TimedOut { seconds: 5 }.into();
        assert_eq!(IpcError::from(timeout).code(), IpcErrorCode::Internal);
    }

    #[test]
    fn cancelled_maps_to_invalid_state() {
        let ipc: IpcError = EngineError::Cancelled.into();
        assert_eq!(ipc.code(), IpcErrorCode::InvalidState);
        assert_eq!(ipc.reason(), Some("job_cancelled"));
        assert_eq!(ipc.message(), "job cancelled");
    }
}
