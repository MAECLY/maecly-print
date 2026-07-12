//! Print engine: format router, converters (everything → PDF) and per-OS
//! printer adapters (see `docs/04-architecture/print-engine.md` and ADR-0003).
//!
//! The engine has three pure-ish concerns, one per submodule:
//!
//! - [`pagerange`] — parse/validate `"1-5,10"` page-range expressions.
//! - [`convert`] — turn a supported input file into a printable PDF (images are
//!   rasterised into a one-page PDF; PDFs pass through untouched).
//! - [`print`] — translate [`crate::queue::job::JobSettings`] into the
//!   SumatraPDF (Windows) / CUPS `lp` (Unix) invocation and spool it.
//!
//! Everything is small and testable: the flag builders and parsers are pure
//! functions with co-located unit tests; only the `spool`/`cancel` entry points
//! actually spawn a process, and those are `cfg`-gated per platform.

use std::path::Path;

pub mod convert;
pub mod pagerange;
pub mod print;

/// File extensions the engine can currently turn into a printable PDF: PDFs
/// (passthrough) plus the raster image formats the bundled `image` codecs
/// decode. Office/HTML/Markdown converters (LibreOffice, Chromium, Pandoc) are
/// documented in the architecture but not yet wired in here.
pub const SUPPORTED_EXTS: &[&str] = &["pdf", "jpg", "jpeg", "png", "tif", "tiff", "bmp", "webp"];

/// Whether `ext` (with or without a leading dot, any case) names a format the
/// engine can print.
pub fn is_supported(ext: &str) -> bool {
    let ext = ext.trim_start_matches('.').to_ascii_lowercase();
    SUPPORTED_EXTS.contains(&ext.as_str())
}

/// The lowercased extension of `path` without the leading dot, or an empty
/// string when the path has none. Shared by the router and callers that need to
/// classify a file the same way the engine does.
pub fn extension_of(path: &Path) -> String {
    path.extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default()
}

/// Every way the engine can fail. Messages are user-facing: they surface in the
/// queue UI and the job log, so they are written for a person, not a developer.
#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    /// A converter could not turn the input into a valid PDF (bad/corrupt input,
    /// unreadable file, unsupported image sub-format such as CMYK JPEG).
    #[error("conversion failed: {0}")]
    ConversionFailed(String),

    /// The OS spooler/printer backend rejected the job.
    #[error("printing failed: {0}")]
    PrintFailed(String),

    /// A requested setting the driver cannot honour and that would otherwise
    /// silently degrade the output (currently only duplex — see
    /// [`print::check_supported`]).
    #[error("the driver for '{printer}' does not support {setting}")]
    UnsupportedSetting { setting: String, printer: String },

    /// A required external program (CUPS `lp`, SumatraPDF) is not installed or
    /// could not be located.
    #[error("missing dependency: {0}")]
    DependencyMissing(String),

    /// The file's format is not one the engine can print at all.
    #[error("unsupported file format: {0}")]
    UnsupportedFormat(String),

    /// The job's cancellation flag was set; work stopped at a stage boundary.
    #[error("job cancelled")]
    Cancelled,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn is_supported_is_case_insensitive_and_dot_tolerant() {
        assert!(is_supported("pdf"));
        assert!(is_supported("PDF"));
        assert!(is_supported("JpEg"));
        assert!(is_supported(".png"));
        assert!(is_supported("TIFF"));
        assert!(!is_supported("docx"));
        assert!(!is_supported("gif")); // decodable by `image` but not enabled
        assert!(!is_supported(""));
    }

    #[test]
    fn supported_exts_are_the_expected_set() {
        assert_eq!(
            SUPPORTED_EXTS,
            &["pdf", "jpg", "jpeg", "png", "tif", "tiff", "bmp", "webp"]
        );
    }

    #[test]
    fn extension_of_lowercases_and_handles_missing() {
        assert_eq!(extension_of(Path::new("/a/b/report.PDF")), "pdf");
        assert_eq!(extension_of(Path::new("/a/b/photo.JPeG")), "jpeg");
        assert_eq!(extension_of(Path::new("/a/b/Makefile")), "");
    }
}
