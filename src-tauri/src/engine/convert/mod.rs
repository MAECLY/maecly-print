//! Input → PDF router.
//!
//! Every job's file is normalised to a single printable PDF before it reaches
//! the printer backend (ADR-0003's "convert-everything-to-PDF" strategy). This
//! module owns the routing decision ([`route`]); each converter lives in its own
//! submodule:
//!
//! | Route | Extensions | Converter |
//! |---|---|---|
//! | [`Route::Passthrough`] | `pdf` | none — `%PDF-` header check, printed in place |
//! | [`Route::Image`] | [`IMAGE_EXTS`] | in-process [`image`] crate |
//! | [`Route::Heif`] | [`heif::HEIF_EXTS`] | OS decoder: `sips` (macOS) / WIC (Windows) |
//! | [`Route::Office`] | [`office::OFFICE_EXTS`] | LibreOffice headless |
//! | [`Route::Html`] | [`html::HTML_EXTS`] | Edge / Chrome / Chromium headless |
//! | [`Route::Markdown`] | [`markdown::MARKDOWN_EXTS`] | Pandoc → HTML → browser |
//!
//! External converters run in a private scratch directory ([`WorkDir`], removed
//! when the conversion ends, success or not), are bounded by the conversion
//! timeout and the job's cancel flag ([`process::run_bounded`] kills the child
//! tree), and a missing program is [`EngineError::DependencyMissing`] carrying
//! the FR-058 remediation text, which pauses the job instead of failing it.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use super::deps::{self, Dependency};
use super::{extension_of, EngineError};

pub mod heif;
pub mod html;
pub mod image;
pub mod markdown;
pub mod office;
pub mod process;

#[cfg(test)]
mod external_tests;

/// Raster image extensions the in-process converter decodes.
pub const IMAGE_EXTS: &[&str] = &["jpg", "jpeg", "png", "tif", "tiff", "bmp", "webp"];

pub use heif::HEIF_EXTS;
pub use html::HTML_EXTS;
pub use markdown::MARKDOWN_EXTS;
pub use office::OFFICE_EXTS;

/// Which converter handles a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Route {
    Passthrough,
    Image,
    Heif,
    Office,
    Html,
    Markdown,
}

/// The converter for an extension (with or without a dot, any case), or
/// `None` when the engine cannot print it.
pub fn route(ext: &str) -> Option<Route> {
    let ext = ext.trim_start_matches('.').to_ascii_lowercase();
    let ext = ext.as_str();
    if ext == "pdf" {
        Some(Route::Passthrough)
    } else if IMAGE_EXTS.contains(&ext) {
        Some(Route::Image)
    } else if HEIF_EXTS.contains(&ext) {
        Some(Route::Heif)
    } else if OFFICE_EXTS.contains(&ext) {
        Some(Route::Office)
    } else if HTML_EXTS.contains(&ext) {
        Some(Route::Html)
    } else if MARKDOWN_EXTS.contains(&ext) {
        Some(Route::Markdown)
    } else {
        None
    }
}

/// Whether a file of this extension always becomes exactly one page (a
/// picture). Documents have an unknown page count until they are converted.
pub fn is_single_page(ext: &str) -> bool {
    matches!(route(ext), Some(Route::Image | Route::Heif))
}

/// A private scratch directory for one conversion, deleted on drop — so every
/// exit path (success, failure, cancel, timeout, unwinding) cleans up.
pub struct WorkDir(tempfile::TempDir);

impl WorkDir {
    /// Create a fresh scratch directory in the OS temp folder.
    pub fn new(job_id: &str) -> Result<Self, EngineError> {
        // Job ids are UUIDs; anything else is reduced to a safe prefix.
        let tag: String = job_id
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
            .take(40)
            .collect();
        tempfile::Builder::new()
            .prefix(&format!("maeclyprint-convert-{tag}-"))
            .tempdir()
            .map(WorkDir)
            .map_err(|e| {
                EngineError::ConversionFailed(format!("cannot create a conversion folder: {e}"))
            })
    }

    pub fn path(&self) -> &Path {
        self.0.path()
    }
}

/// Route `input` to the right converter and produce a printable PDF.
///
/// Returns:
/// - `Ok(None)` when `input` is already a valid PDF (print the original).
/// - `Ok(Some(path))` with the freshly-written `cache_dir/<job_id>.pdf`.
/// - `Err(..)` for unreadable/corrupt files, unsupported formats, a missing
///   converter ([`EngineError::DependencyMissing`]), a timeout, or when
///   `cancel` has been set.
///
/// `cancel` is checked at each stage boundary and polled continuously while an
/// external converter runs (raising it kills the converter). `timeout` bounds
/// each external converter on its own; the dispatcher additionally raises
/// `cancel` when its conversion timeout fires. `progress` receives
/// `0.0..=1.0` updates.
pub fn convert_to_pdf(
    input: &Path,
    cache_dir: &Path,
    job_id: &str,
    cancel: &AtomicBool,
    progress: &(dyn Fn(f32) + Send + Sync),
    timeout: Option<Duration>,
) -> Result<Option<PathBuf>, EngineError> {
    check_cancel(cancel)?;

    let ext = extension_of(input);
    if ext.is_empty() {
        return Err(EngineError::UnsupportedFormat(format!(
            "{} has no file extension",
            input.display()
        )));
    }
    let Some(route) = route(&ext) else {
        return Err(EngineError::UnsupportedFormat(ext));
    };

    if route == Route::Passthrough {
        validate_pdf_header(input)?;
        check_cancel(cancel)?;
        return Ok(None);
    }

    // Resolve the converter before touching the disk, so a missing program
    // pauses the job without leaving anything behind.
    let tools = match route {
        Route::Office => vec![locate(Dependency::LibreOffice)?],
        Route::Html => vec![locate(Dependency::Chromium)?],
        Route::Markdown => vec![locate(Dependency::Pandoc)?, locate(Dependency::Chromium)?],
        _ => Vec::new(),
    };

    std::fs::create_dir_all(cache_dir).map_err(|e| {
        EngineError::ConversionFailed(format!(
            "cannot create cache directory {}: {e}",
            cache_dir.display()
        ))
    })?;
    let output = cache_dir.join(format!("{job_id}.pdf"));
    check_cancel(cancel)?;

    let result = run_route(
        route, &tools, input, &output, job_id, cancel, progress, timeout,
    )
    .and_then(|()| check_cancel(cancel));
    match result {
        Ok(()) => Ok(Some(output)),
        Err(e) => {
            // Never leave a half-written cache PDF behind.
            let _ = std::fs::remove_file(&output);
            Err(e)
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn run_route(
    route: Route,
    tools: &[PathBuf],
    input: &Path,
    output: &Path,
    job_id: &str,
    cancel: &AtomicBool,
    progress: &(dyn Fn(f32) + Send + Sync),
    timeout: Option<Duration>,
) -> Result<(), EngineError> {
    let tool = |i: usize| -> Result<&Path, EngineError> {
        tools.get(i).map(PathBuf::as_path).ok_or_else(|| {
            EngineError::ConversionFailed("internal error: converter not resolved".to_string())
        })
    };
    match route {
        Route::Passthrough => Ok(()),
        Route::Image => image::to_pdf(input, output, progress),
        Route::Heif => {
            let work = WorkDir::new(job_id)?;
            heif::convert(input, output, &work, cancel, timeout, progress)
        }
        Route::Office => {
            let work = WorkDir::new(job_id)?;
            progress(0.0);
            office::convert_with(tool(0)?, input, output, &work, cancel, timeout)?;
            progress(1.0);
            Ok(())
        }
        Route::Html => {
            let work = WorkDir::new(job_id)?;
            progress(0.0);
            html::convert_with(tool(0)?, input, output, &work, cancel, timeout)?;
            progress(1.0);
            Ok(())
        }
        Route::Markdown => {
            let work = WorkDir::new(job_id)?;
            progress(0.0);
            markdown::convert_with(tool(0)?, tool(1)?, input, output, &work, cancel, timeout)?;
            progress(1.0);
            Ok(())
        }
    }
}

/// Find `dep` on this machine, or explain how to install it (FR-058).
pub fn locate(dep: Dependency) -> Result<PathBuf, EngineError> {
    let os = deps::Os::current();
    match deps::resolve_on_host(dep, os, None) {
        deps::Resolution::Found { path, .. } => Ok(path),
        deps::Resolution::Missing => Err(EngineError::DependencyMissing(
            deps::spec(dep, os).remediation(),
        )),
    }
}

/// Move a converter's `produced` PDF to `output`, after checking it really is
/// a PDF. `log` is the converter's stderr, quoted when it is not.
pub(crate) fn take_output(
    produced: &Path,
    output: &Path,
    label: &str,
    log: &Path,
) -> Result<(), EngineError> {
    if validate_pdf_header(produced).is_err() {
        let log = std::fs::read_to_string(log).unwrap_or_default();
        let log = log.trim();
        let skip = log.chars().count().saturating_sub(400);
        let detail: String = log.chars().skip(skip).collect();
        return Err(EngineError::ConversionFailed(if detail.is_empty() {
            format!("{label} could not convert this file to PDF")
        } else {
            format!("{label} could not convert this file to PDF: {detail}")
        }));
    }
    if std::fs::rename(produced, output).is_err() {
        // Scratch and cache can sit on different volumes; fall back to a copy.
        std::fs::copy(produced, output).map_err(|e| {
            EngineError::ConversionFailed(format!("cannot write {}: {e}", output.display()))
        })?;
    }
    Ok(())
}

/// `file://` URL for a local absolute path, percent-encoded. Used for the
/// LibreOffice profile location, the page the browser prints and the
/// Markdown `<base href>`.
pub fn file_url(path: &Path) -> String {
    file_url_from(&path.to_string_lossy(), cfg!(windows))
}

/// [`file_url`] with the path flavour explicit, so both are testable anywhere.
pub fn file_url_from(path: &str, windows: bool) -> String {
    use percent_encoding::{utf8_percent_encode, AsciiSet, NON_ALPHANUMERIC};
    const KEEP: &AsciiSet = &NON_ALPHANUMERIC
        .remove(b'/')
        .remove(b'-')
        .remove(b'.')
        .remove(b'_')
        .remove(b'~')
        .remove(b':');
    if !windows {
        return format!("file://{}", utf8_percent_encode(path, KEEP));
    }
    let mut p = path.replace('\\', "/");
    if let Some(rest) = p.strip_prefix("//?/UNC/") {
        p = format!("//{rest}");
    } else if let Some(rest) = p.strip_prefix("//?/") {
        p = rest.to_string();
    }
    let encoded = utf8_percent_encode(&p, KEEP).to_string();
    match encoded.strip_prefix("//") {
        // UNC share: the server is the URL host.
        Some(unc) => format!("file://{unc}"),
        None => format!("file:///{encoded}"),
    }
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
            let out = convert_to_pdf(&src, &cache, "job-1", &cancel, &progress, None)
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
        let out = convert_to_pdf(&pdf, dir.path(), "job", &cancel, &progress, None).unwrap();
        assert!(out.is_none(), "a real PDF is printed in place");
    }

    #[test]
    fn junk_bytes_named_pdf_are_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let pdf = dir.path().join("in.pdf");
        std::fs::write(&pdf, b"this is definitely not a pdf").unwrap();
        let cancel = AtomicBool::new(false);
        let progress = |_p: f32| {};
        let err = convert_to_pdf(&pdf, dir.path(), "job", &cancel, &progress, None).unwrap_err();
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
        let err = convert_to_pdf(
            &png,
            &dir.path().join("cache"),
            "job",
            &cancel,
            &progress,
            None,
        )
        .unwrap_err();
        assert!(matches!(err, EngineError::ConversionFailed(_)));
    }

    #[test]
    fn unsupported_extension_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("setup.exe");
        std::fs::write(&exe, b"MZ").unwrap();
        let cancel = AtomicBool::new(false);
        let progress = |_p: f32| {};
        let err = convert_to_pdf(&exe, dir.path(), "job", &cancel, &progress, None).unwrap_err();
        assert!(matches!(err, EngineError::UnsupportedFormat(_)));
    }

    #[test]
    fn fr_052_to_057_every_supported_extension_has_a_route() {
        for ext in crate::engine::SUPPORTED_EXTS {
            assert!(route(ext).is_some(), "{ext} is advertised but not routed");
        }
        // ...and every routed extension is advertised.
        let routed = IMAGE_EXTS
            .iter()
            .chain(HEIF_EXTS)
            .chain(OFFICE_EXTS)
            .chain(HTML_EXTS)
            .chain(MARKDOWN_EXTS)
            .chain(&["pdf"]);
        for ext in routed {
            assert!(
                crate::engine::is_supported(ext),
                "{ext} is routed but not advertised"
            );
        }
        assert_eq!(route("DOCX"), Some(Route::Office));
        assert_eq!(route(".htm"), Some(Route::Html));
        assert_eq!(route("markdown"), Some(Route::Markdown));
        assert_eq!(route("heif"), Some(Route::Heif));
        assert_eq!(route("exe"), None);
    }

    #[test]
    fn only_pictures_are_known_to_be_one_page() {
        assert!(is_single_page("png"));
        assert!(is_single_page("HEIC"));
        assert!(!is_single_page("pdf"));
        assert!(!is_single_page("docx"));
        assert!(!is_single_page("md"));
    }

    #[test]
    fn file_urls_are_percent_encoded_for_both_path_flavours() {
        assert_eq!(
            file_url_from("/Users/ana/Mis documentos/año #1.html", false),
            "file:///Users/ana/Mis%20documentos/a%C3%B1o%20%231.html"
        );
        assert_eq!(
            file_url_from(r"C:\Users\ana\My Docs\a.html", true),
            "file:///C:/Users/ana/My%20Docs/a.html"
        );
        assert_eq!(file_url_from(r"\\?\C:\x\y.md", true), "file:///C:/x/y.md");
        assert_eq!(
            file_url_from(r"\\server\share\z.htm", true),
            "file://server/share/z.htm"
        );
    }

    #[test]
    fn take_output_rejects_a_non_pdf_and_quotes_the_log() {
        let dir = tempfile::tempdir().unwrap();
        let produced = dir.path().join("p.pdf");
        std::fs::write(&produced, b"garbage").unwrap();
        let log = dir.path().join("log");
        std::fs::write(&log, b"Error: source file could not be loaded").unwrap();
        let err =
            take_output(&produced, &dir.path().join("o.pdf"), "LibreOffice", &log).unwrap_err();
        assert!(
            matches!(&err, EngineError::ConversionFailed(m) if m.contains("could not be loaded")),
            "{err:?}"
        );
    }

    #[test]
    fn work_dirs_are_removed_when_dropped() {
        let work = WorkDir::new("0192-abc/../evil").unwrap();
        let path = work.path().to_path_buf();
        assert!(path.is_dir());
        assert!(
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("maeclyprint-convert-0192-abcevil-"),
            "{path:?}"
        );
        drop(work);
        assert!(!path.exists());
    }

    #[test]
    fn missing_extension_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("Makefile");
        std::fs::write(&f, b"all:").unwrap();
        let cancel = AtomicBool::new(false);
        let progress = |_p: f32| {};
        let err = convert_to_pdf(&f, dir.path(), "job", &cancel, &progress, None).unwrap_err();
        assert!(matches!(err, EngineError::UnsupportedFormat(_)));
    }

    #[test]
    fn cancel_flag_aborts_before_work() {
        let dir = tempfile::tempdir().unwrap();
        let png = write_image(dir.path(), "x.png", ImageFormat::Png);
        let cancel = AtomicBool::new(true);
        let progress = |_p: f32| {};
        let err = convert_to_pdf(&png, dir.path(), "job", &cancel, &progress, None).unwrap_err();
        assert!(matches!(err, EngineError::Cancelled));
    }
}
