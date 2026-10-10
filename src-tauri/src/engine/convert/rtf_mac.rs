//! RTF → PDF on macOS through the system's `textutil` (FR-057), with no
//! LibreOffice.
//!
//! **Stub (round-3 contract, owned by lane L4).** [`available`] says whether
//! this converter is ready to take RTF from LibreOffice; it stays `false` until
//! L4 lands the conversion (`textutil -convert html` → the HTML route).

use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use super::{PageSetup, WorkDir};
use crate::engine::EngineError;

/// Whether RTF is converted here instead of by LibreOffice.
pub fn available() -> bool {
    false
}

/// Convert the RTF at `input` into a PDF at `output`.
pub fn convert(
    input: &Path,
    output: &Path,
    work: &WorkDir,
    cancel: &AtomicBool,
    timeout: Option<Duration>,
    page: &PageSetup,
) -> Result<(), EngineError> {
    let _ = (input, output, work, cancel, timeout, page);
    Err(EngineError::ConversionFailed(
        "RTF conversion through textutil is not implemented yet".into(),
    ))
}
