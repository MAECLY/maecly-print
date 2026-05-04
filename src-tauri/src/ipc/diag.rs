//! Diagnostic IPC commands.
//!
//! `app_info` is the canary command for this milestone: it proves the React
//! ↔ Rust round-trip works end-to-end and that filesystem paths resolve as
//! expected on the host OS.

use serde::Serialize;
use specta::Type;

use crate::util::{error::IpcError, paths};

#[derive(Debug, Clone, Serialize, Type)]
pub struct AppInfo {
    pub version: String,
    pub build_date: String,
    pub config_dir: String,
    pub log_dir: String,
    pub data_dir: String,
}

#[tauri::command]
#[specta::specta]
pub fn app_info() -> Result<AppInfo, IpcError> {
    Ok(AppInfo {
        version: env!("CARGO_PKG_VERSION").to_string(),
        build_date: env!("BUILD_DATE").to_string(),
        config_dir: paths::config_dir().display().to_string(),
        log_dir: paths::log_dir().display().to_string(),
        data_dir: paths::data_dir().display().to_string(),
    })
}
