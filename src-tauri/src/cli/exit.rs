//! Exit codes (FR-009) and the CLI's error carrier.
//!
//! The table below is the *contract*: the numbers are public surface, exactly
//! as documented in `docs/04-architecture/ipc-and-cli.md` §3.7, and a script
//! that branches on them must keep working across releases. `cli_contract.rs`
//! pins every value, so changing one fails the build rather than silently
//! breaking a caller.
//!
//! Errors travel as a [`CliError`]: an [`IpcError`] (the same payload the
//! webview receives, so both surfaces report failures identically) plus the
//! exit code it maps to. [`exit_code_for`] is the default mapping; a call site
//! that knows better — "no printer is configured" arrives as
//! [`IpcError::PrintFailed`] but is really a printer problem — overrides it
//! with [`CliError::new`].

use crate::util::error::IpcError;

/// Every exit code the binary can return. Values are frozen public surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(i32)]
pub enum ExitCode {
    /// All requested operations succeeded.
    Success = 0,
    /// Generic error (catchall).
    Generic = 1,
    /// Invalid command-line usage.
    Usage = 2,
    /// Configuration error (unreadable/invalid `config.toml`, missing
    /// dependency the machine is expected to provide).
    Config = 3,
    /// Profile not found / invalid.
    Profile = 4,
    /// Printer not found / invalid (including a setting the driver refuses).
    Printer = 5,
    /// One or more files not found / unreadable.
    FileNotFound = 6,
    /// One or more files unsupported.
    UnsupportedFile = 7,
    /// Conversion failed (timeout or converter error).
    ConversionFailed = 8,
    /// Printing failed (spooler reported an error).
    PrintFailed = 9,
    /// Update check failed (network/signature).
    UpdateCheckFailed = 10,
    /// Update install failed.
    UpdateInstallFailed = 11,
    /// User cancelled.
    Cancelled = 12,
}

impl ExitCode {
    /// The numeric code handed to the OS.
    pub fn code(self) -> i32 {
        self as i32
    }

    /// The worse (higher) of two codes. A batch returns the worst code among
    /// its files, so one failure in fifty is never reported as success.
    pub fn worst(self, other: ExitCode) -> ExitCode {
        if other > self {
            other
        } else {
            self
        }
    }
}

/// Default [`IpcError`] → [`ExitCode`] mapping.
///
/// Two mappings deserve a word:
/// - [`IpcError::UnsupportedSetting`] is a *printer* problem (the driver cannot
///   honour duplex), not a usage error — the flag itself was legal — so it maps
///   to [`ExitCode::Printer`].
/// - [`IpcError::DependencyMissing`] means the machine lacks a program we shell
///   out to (CUPS `lp`, SumatraPDF). That is an environment/configuration
///   fault, so it maps to [`ExitCode::Config`].
///
/// [`IpcError::JobNotFound`] / [`IpcError::InvalidState`] have no dedicated
/// code in the documented table and fall back to [`ExitCode::Generic`].
pub fn exit_code_for(err: &IpcError) -> ExitCode {
    match err {
        IpcError::FileNotFound(_) => ExitCode::FileNotFound,
        IpcError::InvalidRequest(_) => ExitCode::Usage,
        IpcError::UnsupportedFormat(_) => ExitCode::UnsupportedFile,
        IpcError::ConversionFailed(_) => ExitCode::ConversionFailed,
        IpcError::PrintFailed(_) => ExitCode::PrintFailed,
        IpcError::UnsupportedSetting(_) => ExitCode::Printer,
        IpcError::DependencyMissing(_) => ExitCode::Config,
        IpcError::ProfileNotFound(_) => ExitCode::Profile,
        IpcError::UpdateCheckFailed(_) => ExitCode::UpdateCheckFailed,
        IpcError::JobNotFound(_) | IpcError::InvalidState(_) | IpcError::Internal(_) => {
            ExitCode::Generic
        }
    }
}

/// A CLI failure: what went wrong (in the shared IPC vocabulary) and which exit
/// code it produces.
#[derive(Debug)]
pub struct CliError {
    pub exit: ExitCode,
    pub error: IpcError,
}

impl CliError {
    /// Pair an error with an explicit exit code, overriding [`exit_code_for`].
    pub fn new(exit: ExitCode, error: IpcError) -> Self {
        Self { exit, error }
    }

    /// An invalid-usage error with `message`.
    pub fn usage(message: impl Into<String>) -> Self {
        Self::new(ExitCode::Usage, IpcError::InvalidRequest(message.into()))
    }

    /// A printer-level error with `message`.
    pub fn printer(message: impl Into<String>) -> Self {
        Self::new(ExitCode::Printer, IpcError::PrintFailed(message.into()))
    }
}

impl From<IpcError> for CliError {
    fn from(error: IpcError) -> Self {
        Self {
            exit: exit_code_for(&error),
            error,
        }
    }
}

impl From<crate::engine::EngineError> for CliError {
    fn from(err: crate::engine::EngineError) -> Self {
        IpcError::from(err).into()
    }
}

impl From<crate::engine::print::PrintError> for CliError {
    /// The print stage's own failures.
    ///
    /// A **timeout** is a printing failure ([`ExitCode::PrintFailed`]): the
    /// spooler took the job and never finished with it. A **missing printer**
    /// or **no printer at all** is [`ExitCode::Printer`] — where the GUI pauses
    /// such a job so the user can pick a replacement, a script needs to hear
    /// "this host cannot print that" straight away.
    fn from(err: crate::engine::print::PrintError) -> Self {
        use crate::engine::print::PrintError as P;
        match err {
            P::TimedOut { .. } => Self::new(
                ExitCode::PrintFailed,
                IpcError::PrintFailed(err_message(&err)),
            ),
            P::PrinterMissing { .. } | P::NoPrinter => {
                Self::new(ExitCode::Printer, IpcError::PrintFailed(err_message(&err)))
            }
            P::Engine(engine) => engine.into(),
        }
    }
}

/// `Display` of an error, in a form the user reads verbatim.
fn err_message(err: &impl std::fmt::Display) -> String {
    err.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fr_009_worst_wins_in_a_batch() {
        assert_eq!(
            ExitCode::Success.worst(ExitCode::PrintFailed),
            ExitCode::PrintFailed
        );
        assert_eq!(
            ExitCode::ConversionFailed.worst(ExitCode::Usage),
            ExitCode::ConversionFailed
        );
        assert_eq!(
            ExitCode::Success.worst(ExitCode::Success),
            ExitCode::Success
        );
    }

    #[test]
    fn fr_009_engine_errors_reach_their_documented_codes() {
        use crate::engine::EngineError;
        let cases = [
            (
                EngineError::ConversionFailed("x".into()),
                ExitCode::ConversionFailed,
            ),
            (EngineError::PrintFailed("x".into()), ExitCode::PrintFailed),
            (
                EngineError::UnsupportedSetting {
                    setting: "duplex".into(),
                    printer: "P".into(),
                },
                ExitCode::Printer,
            ),
            (
                EngineError::DependencyMissing("lp".into()),
                ExitCode::Config,
            ),
            (
                EngineError::UnsupportedFormat("docx".into()),
                ExitCode::UnsupportedFile,
            ),
        ];
        for (engine_err, expected) in cases {
            assert_eq!(CliError::from(engine_err).exit, expected);
        }
    }
}
