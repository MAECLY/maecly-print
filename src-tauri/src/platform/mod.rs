//! OS-specific printer enumeration backends.
//!
//! - **Unix** (macOS, Linux): CUPS, driven through the `lpstat` / `lpoptions`
//!   / `ippfind` CLIs. We parse the locale-independent IPP keyword output
//!   (e.g. `printer-state=3`, `marker-levels=…`) rather than the localized
//!   human prose that `lpstat -l` prints.
//! - **Windows**: the print spooler via the `windows` crate (`EnumPrinters` +
//!   `DeviceCapabilities` + `GetDefaultPrinter`).
//!
//! Each backend exposes the same three functions returning the shared IPC
//! types from [`crate::ipc::printers`]. They return `anyhow::Result`; the IPC
//! layer maps failures to `IpcError`.

#[cfg(unix)]
mod cups;
pub mod testpage;
#[cfg(windows)]
mod windows;

#[cfg(unix)]
pub use cups::{
    add_printer, default_printer, discover_printers, list_printers, open_printer_settings,
    print_test_page, set_os_default,
};

#[cfg(windows)]
pub use windows::{
    add_printer, default_printer, discover_printers, list_printers, open_printer_settings,
    print_test_page, set_os_default,
};

// Fallback for any other target so the crate always compiles.
#[cfg(not(any(unix, windows)))]
pub fn list_printers() -> anyhow::Result<Vec<crate::ipc::printers::Printer>> {
    Ok(Vec::new())
}
#[cfg(not(any(unix, windows)))]
pub fn default_printer() -> anyhow::Result<Option<String>> {
    Ok(None)
}
#[cfg(not(any(unix, windows)))]
pub fn discover_printers() -> anyhow::Result<Vec<crate::ipc::printers::DiscoveredPrinter>> {
    Ok(Vec::new())
}
#[cfg(not(any(unix, windows)))]
pub fn set_os_default(_name: &str) -> anyhow::Result<()> {
    Ok(())
}
#[cfg(not(any(unix, windows)))]
pub fn add_printer(_name: &str, _uri: &str) -> anyhow::Result<()> {
    Ok(())
}
#[cfg(not(any(unix, windows)))]
pub fn print_test_page(_name: &str) -> anyhow::Result<()> {
    Ok(())
}
#[cfg(not(any(unix, windows)))]
pub fn open_printer_settings() -> anyhow::Result<()> {
    Ok(())
}
