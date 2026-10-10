//! SVG / SVGZ → vector PDF (FR-052a).
//!
//! **Stub (round-3 contract, owned by lane L2).** The route, the extensions
//! and this entry point are fixed; the conversion itself (`usvg` + `svg2pdf`
//! at 96 dpi, local-only resources, a size cap for `svgz`, fonts loaded once,
//! the page clamped to `MAX_PAGE_PT`) lands with L2. Until then an SVG job
//! fails cleanly instead of reaching the printer.

use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use crate::engine::EngineError;

/// Extensions this converter handles. `svgz` is gzip-compressed SVG.
pub const SVG_EXTS: &[&str] = &["svg", "svgz"];

/// Convert the SVG at `input` into a one-page PDF at `output`.
pub fn convert(
    input: &Path,
    output: &Path,
    cancel: &AtomicBool,
    timeout: Option<Duration>,
    progress: &(dyn Fn(f32) + Send + Sync),
) -> Result<(), EngineError> {
    let _ = (input, output, cancel, timeout, progress);
    Err(EngineError::ConversionFailed(
        "SVG conversion is not implemented yet".into(),
    ))
}
