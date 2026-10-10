//! Small cross-cutting helpers: the IPC error type shared by every command
//! and CLI path, filesystem path resolution, and Chromium discovery.

pub mod chromium;
pub mod error;
pub mod legacy;
pub mod paths;
