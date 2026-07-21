//! Input → PDF router.
//!
//! Every job's file is normalised to a single printable PDF before it reaches
//! the printer backend (ADR-0003's "convert-everything-to-PDF" strategy). This
//! module owns the routing decision; the actual rasterisation lives in
//! [`image`].
//!
//! Two outcomes:
//! - **Passthrough** (already a PDF): validated cheaply by its `%PDF-` header
//!   and printed in place — [`convert_to_pdf`] returns `Ok(None)`.
//! - **Convert** (a raster image): rendered into `cache_dir/<job_id>.pdf` and
//!   returned as `Ok(Some(path))`.
//!
//! Office documents, HTML and Markdown are out of scope here for now; their
//! extensions fall through to [`EngineError::UnsupportedFormat`].

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use super::{extension_of, EngineError};

pub mod image;

/// Raster image extensions we route to the image→PDF converter. Kept in sync
/// with [`super::SUPPORTED_EXTS`] minus `pdf`.
const IMAGE_EXTS: &[&str] = &["jpg", "jpeg", "png", "tif", "tiff", "bmp", "webp"];

/// HEIF-family extensions. Listed as MVP in
/// `docs/03-requirements/supported-formats.md`, but genuinely undecodable here:
/// the `image` crate ships no HEIF decoder at any feature combination, so
/// supporting it means a native dependency (libheif) or shelling out to
/// ImageMagick — an architecture decision, not a bug fix (see ADR-0003's
/// "pure-Rust codecs" line). Until that is taken, the honest answer is an
/// explicit "not supported yet" rather than the generic unsupported-format
/// message a user would read as "MaeclyPrint does not know what a .heic is".
pub const HEIF_EXTS: &[&str] = &["heic", "heif"];

/// The user-facing reason a HEIF-family file is refused. Shared with the file
/// inspection surface so the drop toast and the engine tell the same story.
pub const HEIF_UNSUPPORTED_REASON: &str =
    "HEIC is not supported yet — convert it to JPEG or PNG first";

/// Route `input` to the right converter and produce a printable PDF.
///
/// Returns:
/// - `Ok(None)` when `input` is already a valid PDF (print the original).
/// - `Ok(Some(path))` with the freshly-written cache PDF for image inputs.
/// - `Err(..)` for unreadable/corrupt files, unsupported formats, or when
///   `cancel` has been set.
///
/// `cancel` is checked at each stage boundary (decoding a large image is a
/// single blocking call that cannot itself be interrupted). `progress` receives
/// `0.0..=1.0` updates from the active converter.
pub fn convert_to_pdf(
    input: &Path,
    cache_dir: &Path,
    job_id: &str,
    cancel: &AtomicBool,
    progress: &(dyn Fn(f32) + Send + Sync),
) -> Result<Option<PathBuf>, EngineError> {
    check_cancel(cancel)?;

    let ext = extension_of(input);
    if ext.is_empty() {
        return Err(EngineError::UnsupportedFormat(format!(
            "{} has no file extension",
            input.display()
        )));
    }

    if ext == "pdf" {
        validate_pdf_header(input)?;
        check_cancel(cancel)?;
        return Ok(None);
    }

    if HEIF_EXTS.contains(&ext.as_str()) {
        return Err(EngineError::UnsupportedFormat(
            HEIF_UNSUPPORTED_REASON.to_string(),
        ));
    }

    if IMAGE_EXTS.contains(&ext.as_str()) {
        std::fs::create_dir_all(cache_dir).map_err(|e| {
            EngineError::ConversionFailed(format!(
                "cannot create cache directory {}: {e}",
                cache_dir.display()
            ))
        })?;
        let output = cache_dir.join(format!("{job_id}.pdf"));
        check_cancel(cancel)?;
        image::to_pdf(input, &output, progress)?;
        check_cancel(cancel)?;
        return Ok(Some(output));
    }

    Err(EngineError::UnsupportedFormat(ext))
}

/// Cheap structural check that `input` really is a PDF. We only read the first
/// bytes: a real converter would parse the whole file, but for passthrough all
/// we need is to reject a mislabelled/corrupt file before handing it to the
/// spooler.
fn validate_pdf_header(input: &Path) -> Result<(), EngineError> {
    use std::io::Read;
    let mut file = std::fs::File::open(input).map_err(|e| {
        EngineError::ConversionFailed(format!("cannot open {}: {e}", input.display()))
    })?;
    let mut header = [0u8; 5];
    let read = file.read(&mut header).map_err(|e| {
        EngineError::ConversionFailed(format!("cannot read {}: {e}", input.display()))
    })?;
    if read == 5 && &header == b"%PDF-" {
        Ok(())
    } else {
        Err(EngineError::ConversionFailed("not a valid PDF".to_string()))
    }
}

fn check_cancel(cancel: &AtomicBool) -> Result<(), EngineError> {
    if cancel.load(Ordering::Relaxed) {
        Err(EngineError::Cancelled)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::image::{ImageFormat, Rgb, RgbImage};
    use std::path::Path;
    use std::sync::atomic::AtomicBool;

    /// Encode a small deterministic gradient to `dir/name` in `fmt`, returning
    /// its path. Fixtures are generated in-test; nothing binary is committed.
    fn write_image(dir: &Path, name: &str, fmt: ImageFormat) -> PathBuf {
        let mut img = RgbImage::new(8, 8);
        for (x, y, px) in img.enumerate_pixels_mut() {
            *px = Rgb([(x * 32) as u8, (y * 32) as u8, 128]);
        }
        let path = dir.join(name);
        img.save_with_format(&path, fmt).expect("encode fixture");
        path
    }

    fn assert_one_page_pdf(path: &Path) {
        let doc = lopdf::Document::load(path).expect("cache PDF must parse");
        assert_eq!(doc.get_pages().len(), 1, "one page per image");
    }

    #[test]
    fn converts_every_supported_image_format() {
        let dir = tempfile::tempdir().unwrap();
        let cache = dir.path().join("cache");
        let cancel = AtomicBool::new(false);
        let progress = |_p: f32| {};

        for (name, fmt) in [
            ("a.png", ImageFormat::Png),
            ("a.jpg", ImageFormat::Jpeg),
            ("a.bmp", ImageFormat::Bmp),
            ("a.tif", ImageFormat::Tiff),
            ("a.webp", ImageFormat::WebP),
        ] {
            let src = write_image(dir.path(), name, fmt);
            let out = convert_to_pdf(&src, &cache, "job-1", &cancel, &progress)
                .unwrap_or_else(|e| panic!("convert {name}: {e}"));
            let pdf = out.expect("image conversion yields a cache PDF");
            assert!(pdf.exists(), "{name}: cache PDF should exist");
            assert_one_page_pdf(&pdf);
        }
    }

    #[test]
    fn pdf_passthrough_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let pdf = dir.path().join("in.pdf");
        std::fs::write(&pdf, b"%PDF-1.4\n1 0 obj<</Type/Catalog>>endobj\n").unwrap();
        let cancel = AtomicBool::new(false);
        let progress = |_p: f32| {};
        let out = convert_to_pdf(&pdf, dir.path(), "job", &cancel, &progress).unwrap();
        assert!(out.is_none(), "a real PDF is printed in place");
    }

    #[test]
    fn junk_bytes_named_pdf_are_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let pdf = dir.path().join("in.pdf");
        std::fs::write(&pdf, b"this is definitely not a pdf").unwrap();
        let cancel = AtomicBool::new(false);
        let progress = |_p: f32| {};
        let err = convert_to_pdf(&pdf, dir.path(), "job", &cancel, &progress).unwrap_err();
        assert!(matches!(err, EngineError::ConversionFailed(_)));
    }

    #[test]
    fn corrupt_image_is_conversion_failed() {
        let dir = tempfile::tempdir().unwrap();
        let png = dir.path().join("broken.png");
        // Valid PNG signature, garbage payload — decoding must fail cleanly.
        std::fs::write(&png, b"\x89PNG\r\n\x1a\nnot actually a real png stream").unwrap();
        let cancel = AtomicBool::new(false);
        let progress = |_p: f32| {};
        let err =
            convert_to_pdf(&png, &dir.path().join("cache"), "job", &cancel, &progress).unwrap_err();
        assert!(matches!(err, EngineError::ConversionFailed(_)));
    }

    #[test]
    fn unsupported_extension_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let txt = dir.path().join("notes.txt");
        std::fs::write(&txt, b"hello").unwrap();
        let cancel = AtomicBool::new(false);
        let progress = |_p: f32| {};
        let err = convert_to_pdf(&txt, dir.path(), "job", &cancel, &progress).unwrap_err();
        assert!(matches!(err, EngineError::UnsupportedFormat(_)));
    }

    #[test]
    fn fr_052_heic_is_refused_with_an_explicit_reason() {
        // Not decodable with the bundled codecs; the refusal must say so
        // instead of hiding behind the generic unsupported-format message.
        let dir = tempfile::tempdir().unwrap();
        let cancel = AtomicBool::new(false);
        let progress = |_p: f32| {};
        for name in ["photo.heic", "photo.HEIF"] {
            let src = dir.path().join(name);
            std::fs::write(&src, b"\0\0\0\x18ftypheic").unwrap();
            match convert_to_pdf(&src, dir.path(), "job", &cancel, &progress) {
                Err(EngineError::UnsupportedFormat(msg)) => {
                    assert_eq!(msg, HEIF_UNSUPPORTED_REASON, "{name}");
                }
                other => panic!("{name}: expected an explicit HEIC refusal, got {other:?}"),
            }
        }
    }

    #[test]
    fn missing_extension_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("Makefile");
        std::fs::write(&f, b"all:").unwrap();
        let cancel = AtomicBool::new(false);
        let progress = |_p: f32| {};
        let err = convert_to_pdf(&f, dir.path(), "job", &cancel, &progress).unwrap_err();
        assert!(matches!(err, EngineError::UnsupportedFormat(_)));
    }

    #[test]
    fn cancel_flag_aborts_before_work() {
        let dir = tempfile::tempdir().unwrap();
        let png = write_image(dir.path(), "x.png", ImageFormat::Png);
        let cancel = AtomicBool::new(true);
        let progress = |_p: f32| {};
        let err = convert_to_pdf(&png, dir.path(), "job", &cancel, &progress).unwrap_err();
        assert!(matches!(err, EngineError::Cancelled));
    }
}
