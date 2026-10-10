//! Plain text → PDF (FR-057).
//!
//! **Stub (round-3 contract, owned by lane L4).** The final converter decodes
//! the file (BOM, then strict UTF-8, else Windows-1252), wraps it in a `<pre>`
//! page and renders it through the HTML route, with no LibreOffice. Until it
//! lands, text keeps printing through LibreOffice, exactly as before the route
//! existed.

use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use super::{office, PageSetup, WorkDir};
use crate::engine::deps::Dependency;
use crate::engine::EngineError;

/// Extensions this converter handles.
pub const TEXT_EXTS: &[&str] = &["txt"];

/// Convert the text file at `input` into a PDF at `output`, laid out on the
/// job's paper (`page`).
pub fn convert(
    input: &Path,
    output: &Path,
    work: &WorkDir,
    cancel: &AtomicBool,
    timeout: Option<Duration>,
    page: &PageSetup,
) -> Result<(), EngineError> {
    let _ = page;
    let soffice = super::locate(Dependency::LibreOffice)?;
    office::convert_with(&soffice, input, output, work, cancel, timeout)
}
