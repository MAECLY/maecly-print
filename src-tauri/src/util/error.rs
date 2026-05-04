//! IPC error type.
//!
//! Serialized identically to both webview and CLI (when wired in M1).
//! Schema: `{"code": "<Variant>", "message": "<Display>"}`.
//! Mirrors the contract documented in `docs/04-architecture/ipc-and-cli.md` §4.

use serde::Serialize;
use specta::Type;
use thiserror::Error;

#[derive(Debug, Error, Serialize, Type)]
#[serde(tag = "code", content = "message")]
pub enum IpcError {
    #[error("File not found: {0}")]
    FileNotFound(String),

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
