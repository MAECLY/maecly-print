//! HTML → PDF through the operating system's own web view (FR-055, D15):
//! WebView2 `PrintToPdf` on Windows, `WKWebView` printing on macOS. Runs only
//! inside the render worker ([`crate::worker::render`]), never in the app.
//!
//! **Stub (round-3 contract, owned by lane L4).** The per-OS modules are
//! declared here so L4 fills them in without touching shared files.

#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(windows)]
pub mod windows;
