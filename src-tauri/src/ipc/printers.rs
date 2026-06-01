//! Printer enumeration IPC commands.
//!
//! These commands surface the host's *real* printers. The OS-specific work
//! lives in [`crate::platform`] (CUPS CLI on Unix, the winspool API on
//! Windows); this module only defines the wire types and the thin Tauri
//! command wrappers, keeping the binding-generation surface clean.
//!
//! Capability fields are deliberately permissive: not every printer or OS
//! reports every attribute (a label printer has no toner levels; CUPS may not
//! expose a resolution list). Absent data is `None`/empty rather than faked,
//! and the UI hides whatever is missing. See
//! `docs/04-architecture/printer-enumeration.md`.

use serde::Serialize;
use specta::Type;

use crate::util::error::IpcError;

/// Coarse printer family, used by the UI only to pick an icon.
#[derive(Debug, Clone, Copy, Serialize, Type)]
#[serde(rename_all = "lowercase")]
pub enum PrinterKind {
    Laser,
    Mfp,
    Wide,
    Label,
    Virtual,
}

/// Normalized printer state (a small superset-free view of the OS state).
#[derive(Debug, Clone, Copy, Serialize, Type)]
#[serde(rename_all = "lowercase")]
pub enum PrinterStatus {
    Ready,
    Offline,
    Error,
    Busy,
}

/// One input tray/bin. `paper` and `level` are optional because most OS APIs
/// expose the tray *name* but not the loaded media or fill level.
#[derive(Debug, Clone, Serialize, Type)]
pub struct Tray {
    pub name: String,
    pub paper: Option<String>,
    /// Fill level in `0.0..=1.0`, if the device reports it.
    pub level: Option<f64>,
}

/// Toner/ink supply levels in `0.0..=1.0`. Any channel the device does not
/// report stays `None`; an all-`None` value renders no supply section.
#[derive(Debug, Clone, Default, Serialize, Type)]
pub struct Supplies {
    pub cyan: Option<f64>,
    pub magenta: Option<f64>,
    pub yellow: Option<f64>,
    pub black: Option<f64>,
}

/// A printer installed on the system, enriched with whatever capabilities the
/// OS could be coaxed into reporting.
#[derive(Debug, Clone, Serialize, Type)]
pub struct Printer {
    /// Stable spooler/queue key (used as the React list key).
    pub id: String,
    /// Human-friendly name (falls back to the queue id).
    pub name: String,
    pub kind: PrinterKind,
    pub color: bool,
    pub duplex: bool,
    pub status: PrinterStatus,
    pub is_default: bool,
    pub location: String,
    /// Readable connection/port string, e.g. `USB`, `EPSON … (IPP)`.
    pub connection: String,
    pub driver: String,
    pub shared: bool,
    pub queued_jobs: u32,
    pub max_dpi: Option<u32>,
    pub papers: Vec<String>,
    pub trays: Vec<Tray>,
    pub supplies: Supplies,
    pub default_tray: Option<String>,
    pub default_paper: Option<String>,
}

/// A printer found on the network that is not (necessarily) installed yet.
#[derive(Debug, Clone, Serialize, Type)]
pub struct DiscoveredPrinter {
    pub id: String,
    pub name: String,
    pub connection: String,
    pub kind: PrinterKind,
    /// Discovery transport, e.g. `mDNS · IPP`.
    pub via: String,
}

/// List every printer the OS spooler knows about, with capabilities.
#[tauri::command]
#[specta::specta]
pub async fn list_printers() -> Result<Vec<Printer>, IpcError> {
    run_blocking(crate::platform::list_printers).await
}

/// The OS default printer's id, or `None` when no default is configured.
#[tauri::command]
#[specta::specta]
pub async fn default_printer() -> Result<Option<String>, IpcError> {
    run_blocking(crate::platform::default_printer).await
}

/// Best-effort network discovery of not-yet-installed printers (mDNS/Bonjour
/// on Unix; empty on Windows until WSD lands).
#[tauri::command]
#[specta::specta]
pub async fn discover_printers() -> Result<Vec<DiscoveredPrinter>, IpcError> {
    run_blocking(crate::platform::discover_printers).await
}

// ---------------------------------------------------------------------------
// Preferences (persisted MaeclyPrint-level state) + OS actions
// ---------------------------------------------------------------------------

/// Read the persisted MaeclyPrint printer preferences.
#[tauri::command]
#[specta::specta]
pub async fn get_printer_prefs() -> Result<crate::settings::PrinterPrefs, IpcError> {
    run_blocking(|| Ok(crate::settings::Config::load_or_default().printers)).await
}

/// Replace the persisted printer preferences and return the saved value.
#[tauri::command]
#[specta::specta]
pub async fn set_printer_prefs(
    prefs: crate::settings::PrinterPrefs,
) -> Result<crate::settings::PrinterPrefs, IpcError> {
    run_blocking(move || {
        let mut cfg = crate::settings::Config::load_or_default();
        cfg.printers = prefs;
        cfg.save()?;
        Ok(cfg.printers)
    })
    .await
}

/// Set the OS user-default printer (no elevation required).
#[tauri::command]
#[specta::specta]
pub async fn set_default_printer(name: String) -> Result<(), IpcError> {
    run_blocking(move || crate::platform::set_os_default(&name)).await
}

/// Send a test page to the named printer.
#[tauri::command]
#[specta::specta]
pub async fn print_test_page(name: String) -> Result<(), IpcError> {
    run_blocking(move || crate::platform::print_test_page(&name)).await
}

/// Open the OS "Printers" settings pane so the user can add a printer manually.
#[tauri::command]
#[specta::specta]
pub async fn open_printer_settings() -> Result<(), IpcError> {
    run_blocking(crate::platform::open_printer_settings).await
}

/// Run a blocking enumeration closure off the IPC thread so the webview never
/// stalls on a slow spooler, normalizing both the join error and the inner
/// `anyhow` error into [`IpcError`].
async fn run_blocking<T, F>(f: F) -> Result<T, IpcError>
where
    F: FnOnce() -> anyhow::Result<T> + Send + 'static,
    T: Send + 'static,
{
    let joined = tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| IpcError::Internal(format!("printer task panicked: {e}")))?;
    joined.map_err(IpcError::from)
}
