//! SVG / SVGZ → vector PDF (FR-052a).
//!
//! The drawing is parsed by `usvg` and written by `svg2pdf` (both from the
//! resvg/Typst projects, MIT OR Apache-2.0): shapes, gradients and text stay
//! vector; only filter effects are rasterised. The page is the drawing's own
//! size at 96 dpi (CSS px → 0.75 pt, the same scale as the raster image
//! route), clamped to [`MAX_PAGE_PT`]; fitting it to the paper is left to the
//! printer's scaling flags, exactly like an image.
//!
//! **Isolation (decision D10).** A hostile drawing can make a parser panic,
//! and the release profile aborts on panic, so the conversion runs in a
//! hidden worker process (`pluryprint --worker svg <in> <out>`, see
//! [`crate::worker::svg`]) bounded by the conversion timeout and the job's
//! cancel flag. When no worker executable sits next to the current one (unit
//! tests), it runs on a thread instead.
//!
//! **Security.** Nothing is fetched from the network and no script runs:
//! usvg has no network code, ignores `<script>`, `<foreignObject>` and
//! animation, and its XML parser (`roxmltree`) has no external entities, so
//! XXE cannot read a file. Its default `<image>` resolver, though, opens any
//! absolute path; [`local_only`] replaces it: only files inside the SVG's own
//! folder load, never a UNC share (which would leak NTLM credentials over SMB
//! on Windows), a `file://host/` URL or a `../` outside it. Text uses the
//! system fonts, loaded only when the drawing has text. An `.svgz` is
//! inflated with a cap ([`SVG_MAX_BYTES`]).

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;

use crate::engine::{EngineError, Failure};

/// Extensions this converter handles. `svgz` is gzip-compressed SVG.
pub const SVG_EXTS: &[&str] = &["svg", "svgz"];

/// Largest SVG document accepted, after inflating an `.svgz`.
pub const SVG_MAX_BYTES: u64 = 64 * 1024 * 1024;

/// The drawing's CSS pixels per inch.
const SVG_DPI: f32 = 96.0;

/// Largest page side, in points (the image route's ceiling, 200 in).
const MAX_PAGE_PT: f32 = 14_400.0;

/// Convert the SVG at `input` into a one-page PDF at `output`.
pub fn convert(
    input: &Path,
    output: &Path,
    cancel: &AtomicBool,
    timeout: Option<Duration>,
    progress: &(dyn Fn(f32) + Send + Sync),
) -> Result<(), EngineError> {
    progress(0.0);
    match worker_exe() {
        Some(exe) => convert_with_worker(&exe, input, output, cancel, timeout)?,
        None => {
            let (input, output) = (input.to_path_buf(), output.to_path_buf());
            super::heif::run_abandonable("svg", cancel, timeout, move || {
                convert_in_process(&input, &output)
            })?;
        }
    }
    progress(1.0);
    Ok(())
}

/// The executable that runs `--worker svg`: the app itself. From the
/// console CLI (`pluryprint-cli`) it is the app sitting next to it. `None`
/// when neither is this process (a test harness).
fn worker_exe() -> Option<PathBuf> {
    let me = std::env::current_exe().ok()?;
    let stem = me.file_stem()?.to_string_lossy().to_ascii_lowercase();
    if stem == "pluryprint" {
        return Some(me);
    }
    let sibling = me.with_file_name(if cfg!(windows) {
        "pluryprint.exe"
    } else {
        "pluryprint"
    });
    (stem == "pluryprint-cli" && sibling.is_file()).then_some(sibling)
}

/// Convert in the worker process `exe` (`<exe> --worker svg <in> <out>`),
/// killing it on cancel or timeout. A failure the worker reports keeps its
/// stable reason ([`report_failure`]); a worker that dies (a panic aborts)
/// is `svg_convert_failed`.
pub fn convert_with_worker(
    exe: &Path,
    input: &Path,
    output: &Path,
    cancel: &AtomicBool,
    timeout: Option<Duration>,
) -> Result<(), EngineError> {
    let tag = output
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let work = super::WorkDir::new(&tag)?;
    let log = work.path().join("svg-worker.log");
    let mut cmd = std::process::Command::new(exe);
    cmd.arg(crate::worker::WORKER_FLAG)
        .arg(crate::worker::WorkerKind::Svg.as_arg())
        .arg(input)
        .arg(output)
        .current_dir(work.path());
    match super::process::run_bounded(cmd, "SVG converter", cancel, timeout, &log) {
        Ok(()) if output.is_file() => Ok(()),
        Ok(()) => Err(convert_failed("the SVG worker wrote no PDF")),
        Err(EngineError::ConversionFailed(f)) => {
            let reported = std::fs::read_to_string(&log)
                .ok()
                .and_then(|text| parse_failure(&text));
            Err(EngineError::ConversionFailed(reported.unwrap_or_else(
                || Failure::new("svg_convert_failed", f.detail),
            )))
        }
        Err(e) => Err(e),
    }
}

/// The marker that starts the failure line a worker prints on stderr.
const FAILURE_MARKER: &str = "pluryprint-failure ";

/// The line the worker prints on stderr for `failure`, so the parent keeps
/// its reason and params ([`parse_failure`]).
pub fn report_failure(failure: &Failure) -> String {
    let json = serde_json::json!({
        "reason": failure.reason,
        "params": failure.params,
        "detail": failure.detail,
    });
    format!("{FAILURE_MARKER}{json}")
}

/// The failure a worker reported in `stderr`, if it reported one. Only
/// reasons of this route are accepted (they are `&'static str`). Pure.
fn parse_failure(stderr: &str) -> Option<Failure> {
    let line = stderr
        .lines()
        .rev()
        .find_map(|l| l.strip_prefix(FAILURE_MARKER))?;
    let value: serde_json::Value = serde_json::from_str(line).ok()?;
    let reason = value.get("reason")?.as_str()?;
    let reason = ["svg_invalid", "svg_too_large", "svg_convert_failed"]
        .into_iter()
        .chain(["cannot_read_input", "cannot_write_output"])
        .find(|r| *r == reason)?;
    let detail = value.get("detail")?.as_str().unwrap_or_default();
    let mut failure = Failure::new(reason, detail);
    if let Some(params) = value.get("params").and_then(|p| p.as_object()) {
        for (name, v) in params {
            if let Some(v) = v.as_str() {
                failure = failure.param(name, v);
            }
        }
    }
    Some(failure)
}

/// Convert in this process. The worker's whole job; also the fallback when
/// there is no worker executable.
pub fn convert_in_process(input: &Path, output: &Path) -> Result<(), EngineError> {
    let pdf = render(input)?;
    std::fs::write(output, pdf).map_err(|e| {
        EngineError::ConversionFailed(Failure::new(
            "cannot_write_output",
            format!("cannot write {}: {e}", output.display()),
        ))
    })
}

/// The PDF bytes of the drawing at `input`.
fn render(input: &Path) -> Result<Vec<u8>, EngineError> {
    let data = read_svg(input)?;
    let dir = input
        .parent()
        .and_then(|d| d.canonicalize().ok())
        .unwrap_or_default();

    let mut opt = usvg::Options {
        resources_dir: Some(dir.clone()),
        ..usvg::Options::default()
    };
    if has_text(&data) {
        opt.fontdb = system_fonts();
    }
    opt.image_href_resolver = usvg::ImageHrefResolver {
        resolve_data: usvg::ImageHrefResolver::default_data_resolver(),
        resolve_string: Box::new(move |href, o| {
            let path = local_only(href, &dir)?;
            (usvg::ImageHrefResolver::default_string_resolver())(&path.to_string_lossy(), o)
        }),
    };
    let tree = usvg::Tree::from_data(&data, &opt).map_err(|e| {
        EngineError::ConversionFailed(Failure::new("svg_invalid", format!("not a valid SVG: {e}")))
    })?;

    let size = tree.size();
    let page = svg2pdf::PageOptions {
        dpi: page_dpi(size.width(), size.height()),
    };
    svg2pdf::to_pdf(&tree, svg2pdf::ConversionOptions::default(), page)
        .map_err(|e| convert_failed(&format!("the SVG could not be converted: {e:?}")))
}

/// The dpi that maps the drawing onto the page: 96, or more for a drawing
/// whose longer side would exceed [`MAX_PAGE_PT`] (shrinking it uniformly).
/// Pure.
fn page_dpi(width_px: f32, height_px: f32) -> f32 {
    let longest_pt = width_px.max(height_px) * 72.0 / SVG_DPI;
    if longest_pt > MAX_PAGE_PT {
        SVG_DPI * longest_pt / MAX_PAGE_PT
    } else {
        SVG_DPI
    }
}

/// Read the SVG text, inflating an `.svgz` (sniffed, not trusted from the
/// extension) — either way at most [`SVG_MAX_BYTES`].
fn read_svg(input: &Path) -> Result<Vec<u8>, EngineError> {
    let cannot_read = |e: std::io::Error| {
        let name = input
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        EngineError::ConversionFailed(
            Failure::new(
                "cannot_read_input",
                format!("cannot read {}: {e}", input.display()),
            )
            .param("file", name),
        )
    };
    let file = std::fs::File::open(input).map_err(cannot_read)?;
    let mut raw = Vec::new();
    file.take(SVG_MAX_BYTES + 1)
        .read_to_end(&mut raw)
        .map_err(cannot_read)?;
    let data = if raw.starts_with(&[0x1f, 0x8b]) {
        let mut inflated = Vec::new();
        flate2::read::GzDecoder::new(raw.as_slice())
            .take(SVG_MAX_BYTES + 1)
            .read_to_end(&mut inflated)
            .map_err(|e| {
                EngineError::ConversionFailed(Failure::new(
                    "svg_invalid",
                    format!("not a valid SVGZ: {e}"),
                ))
            })?;
        inflated
    } else {
        raw
    };
    if data.len() as u64 > SVG_MAX_BYTES {
        let limit = format!("{} MB", SVG_MAX_BYTES / (1024 * 1024));
        return Err(EngineError::ConversionFailed(
            Failure::new(
                "svg_too_large",
                format!("the SVG is larger than {limit} once decompressed"),
            )
            .param("limit", limit),
        ));
    }
    Ok(data)
}

/// Whether the drawing may need fonts (`<text>`), so fonts are only loaded
/// for drawings that use them. Pure.
fn has_text(svg: &[u8]) -> bool {
    svg.windows(5).any(|w| w.eq_ignore_ascii_case(b"<text"))
        || svg.windows(6).any(|w| w.eq_ignore_ascii_case(b":text "))
}

/// The system fonts, loaded once per process.
fn system_fonts() -> Arc<usvg::fontdb::Database> {
    static FONTS: std::sync::OnceLock<Arc<usvg::fontdb::Database>> = std::sync::OnceLock::new();
    FONTS
        .get_or_init(|| {
            let mut db = usvg::fontdb::Database::new();
            db.load_system_fonts();
            Arc::new(db)
        })
        .clone()
}

fn convert_failed(why: &str) -> EngineError {
    EngineError::ConversionFailed(Failure::new("svg_convert_failed", why))
}

/// What an `<image href>` names, before touching the disk. Pure.
#[derive(Debug, PartialEq, Eq)]
enum Href {
    /// A path to resolve against the SVG's folder (relative or absolute).
    Path(PathBuf),
    /// Refused outright: a URL, a UNC share, `file://host/`.
    Refused,
}

/// Classify `href` (path rules of Windows when `windows`). Pure.
fn classify_href(href: &str, windows: bool) -> Href {
    let href = href.trim();
    if href.is_empty() {
        return Href::Refused;
    }
    // UNC shares: `\\host\share`, `//host/share` (and the `\\?\UNC\` form).
    if href.starts_with("\\\\") || href.starts_with("//") {
        return Href::Refused;
    }
    if let Some(rest) = strip_prefix_ignore_case(href, "file:") {
        // `file:///C:/x` and `file:///x` are local; `file://host/x` is not.
        let Some(path) = rest.strip_prefix("///") else {
            return Href::Refused;
        };
        let decoded = percent_encoding::percent_decode_str(path)
            .decode_utf8_lossy()
            .into_owned();
        let looks_like_drive = decoded.as_bytes().get(1) == Some(&b':');
        let path = if windows && looks_like_drive {
            decoded
        } else {
            format!("/{decoded}")
        };
        return classify_href(&path, windows);
    }
    // Any other scheme (`http:`, `https:`, `ftp:`, `smb:`...). A Windows
    // drive letter (`C:\`) is a path, not a scheme.
    if let Some((scheme, _)) = href.split_once(':') {
        let is_drive =
            windows && scheme.len() == 1 && scheme.chars().all(|c| c.is_ascii_alphabetic());
        let is_scheme = !scheme.is_empty()
            && scheme
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "+.-".contains(c));
        if is_scheme && !is_drive {
            return Href::Refused;
        }
    }
    Href::Path(PathBuf::from(href))
}

fn strip_prefix_ignore_case<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    let head = s.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix)
        .then(|| &s[prefix.len()..])
}

/// The local file an `<image href>` may load: one that exists inside `dir`
/// (the SVG's own folder, canonical) once symlinks and `..` are resolved.
/// `None` for everything else, which then simply does not draw.
pub(crate) fn local_only(href: &str, dir: &Path) -> Option<PathBuf> {
    let Href::Path(path) = classify_href(href, cfg!(windows)) else {
        tracing::debug!(event = "svg_resource_refused", reason = "not_local");
        return None;
    };
    if dir.as_os_str().is_empty() {
        return None;
    }
    let candidate = if path.is_absolute() {
        path
    } else {
        dir.join(path)
    };
    let real = candidate.canonicalize().ok()?;
    if real.starts_with(dir) && real.is_file() {
        Some(real)
    } else {
        tracing::debug!(event = "svg_resource_refused", reason = "outside_folder");
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
        let p = dir.join(name);
        std::fs::write(&p, bytes).unwrap();
        p
    }

    fn mediabox(pdf: &Path) -> Vec<f32> {
        let doc = lopdf::Document::load(pdf).unwrap();
        assert_eq!(doc.get_pages().len(), 1, "one page per drawing");
        let page = *doc.get_pages().values().next().unwrap();
        let dict = doc.get_dictionary(page).unwrap();
        dict.get(b"MediaBox")
            .unwrap()
            .as_array()
            .unwrap()
            .iter()
            .map(|o| o.as_float().unwrap())
            .collect()
    }

    fn convert_file(path: &Path) -> Result<PathBuf, EngineError> {
        let out = path.with_extension("out.pdf");
        convert_in_process(path, &out).map(|()| out)
    }

    const RECT: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="100">
        <rect x="10" y="10" width="180" height="80" fill="#c33"/></svg>"##;

    /// FR-052a: a sized SVG becomes one vector page of its size at 96 dpi
    /// (200 × 100 px → 150 × 75 pt).
    #[test]
    fn fr_052a_svg_rect_mediabox_075() {
        let dir = tempfile::tempdir().unwrap();
        let svg = write(dir.path(), "r.svg", RECT);
        let pdf = convert_file(&svg).unwrap();
        let mb = mediabox(&pdf);
        assert!((mb[2] - mb[0] - 150.0).abs() < 0.01, "{mb:?}");
        assert!((mb[3] - mb[1] - 75.0).abs() < 0.01, "{mb:?}");
        // Vector, not a raster: no image XObject in the file.
        let bytes = std::fs::read(&pdf).unwrap();
        assert!(!bytes.windows(13).any(|w| w == b"/Subtype /Ima"));
    }

    /// FR-052a: a drawing with only a viewBox takes the viewBox size; with
    /// neither, the size of what it draws.
    #[test]
    fn fr_052a_svg_viewbox_default() {
        let dir = tempfile::tempdir().unwrap();
        let vb = write(
            dir.path(),
            "vb.svg",
            br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 400 300"><circle cx="200" cy="150" r="100"/></svg>"#,
        );
        let mb = mediabox(&convert_file(&vb).unwrap());
        assert!(
            (mb[2] - 300.0).abs() < 0.01 && (mb[3] - 225.0).abs() < 0.01,
            "{mb:?}"
        );
        let bare = write(
            dir.path(),
            "bare.svg",
            br#"<svg xmlns="http://www.w3.org/2000/svg"><circle cx="50" cy="50" r="40"/></svg>"#,
        );
        // usvg sizes such a drawing from its content's bounding box (here
        // 10..90 px): a small page, never an empty or giant one.
        let mb = mediabox(&convert_file(&bare).unwrap());
        assert!(
            (mb[2] - 67.5).abs() < 0.01 && (mb[3] - 67.5).abs() < 0.01,
            "{mb:?}"
        );
    }

    /// FR-052a: an enormous drawing is shrunk onto the largest page.
    #[test]
    fn fr_052a_svg_page_clamped_to_max() {
        assert_eq!(page_dpi(200.0, 100.0), 96.0);
        let dpi = page_dpi(40_000.0, 10_000.0);
        assert!((40_000.0 * 72.0 / dpi - MAX_PAGE_PT).abs() < 0.5);
        let dir = tempfile::tempdir().unwrap();
        let big = write(
            dir.path(),
            "big.svg",
            br#"<svg xmlns="http://www.w3.org/2000/svg" width="40000" height="10000"><rect width="40000" height="10000"/></svg>"#,
        );
        let mb = mediabox(&convert_file(&big).unwrap());
        assert!((mb[2] - MAX_PAGE_PT).abs() < 1.0, "{mb:?}");
        assert!((mb[3] - MAX_PAGE_PT / 4.0).abs() < 1.0, "{mb:?}");
    }

    fn gzip(bytes: &[u8]) -> Vec<u8> {
        use std::io::Write;
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        gz.write_all(bytes).unwrap();
        gz.finish().unwrap()
    }

    /// FR-052a: `.svgz` (gzip) converts like the plain drawing — and the
    /// format is sniffed, so a gzipped file named `.svg` works too.
    #[test]
    fn fr_052a_svgz_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["r.svgz", "gz-named.svg"] {
            let svgz = write(dir.path(), name, &gzip(RECT));
            let mb = mediabox(&convert_file(&svgz).unwrap());
            assert!((mb[2] - 150.0).abs() < 0.01, "{name}: {mb:?}");
        }
    }

    /// FR-052a / NFR (security): a gzip bomb stops at the cap with
    /// `svg_too_large`, without inflating the rest.
    #[test]
    fn fr_052a_svgz_bomb_is_capped() {
        let dir = tempfile::tempdir().unwrap();
        // 65 MiB of spaces compresses to ~65 KiB.
        let mut big = b"<svg xmlns=\"http://www.w3.org/2000/svg\">".to_vec();
        big.resize((SVG_MAX_BYTES + 1024 * 1024) as usize, b' ');
        let bomb = write(dir.path(), "bomb.svgz", &gzip(&big));
        let err = convert_file(&bomb).unwrap_err();
        assert!(
            matches!(&err, EngineError::ConversionFailed(f)
                if f.reason == Some("svg_too_large") && f.params["limit"] == "64 MB"),
            "{err:?}"
        );
    }

    /// A tiny 2 × 2 PNG for `<image>` tests.
    fn png() -> Vec<u8> {
        let mut out = std::io::Cursor::new(Vec::new());
        image::RgbImage::from_pixel(2, 2, image::Rgb([0, 128, 255]))
            .write_to(&mut out, image::ImageFormat::Png)
            .unwrap();
        out.into_inner()
    }

    fn svg_with_image(href: &str) -> Vec<u8> {
        format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="20" height="20"><image width="20" height="20" xlink:href="{href}"/></svg>"#
        )
        .into_bytes()
    }

    fn embeds_an_image(pdf: &Path) -> bool {
        let bytes = std::fs::read(pdf).unwrap();
        bytes.windows(13).any(|w| w == b"/Subtype /Ima")
    }

    /// FR-052a: an `<image>` next to the drawing (relative href) is drawn.
    #[test]
    fn fr_052a_relative_image() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("img")).unwrap();
        write(&dir.path().join("img"), "dot.png", &png());
        let svg = write(dir.path(), "with-image.svg", &svg_with_image("img/dot.png"));
        assert!(embeds_an_image(&convert_file(&svg).unwrap()));
    }

    /// NFR (security): images outside the SVG's folder, on a share or on the
    /// network never load; the drawing still converts without them.
    #[test]
    fn nfr_security_svg_local_only_table() {
        // Pure classification, both path flavours.
        for (href, windows) in [
            (r"\\attacker\share\x.png", true),
            ("//attacker/share/x.png", false),
            ("file://attacker/share/x.png", false),
            ("FILE://attacker/x.png", true),
            ("http://example.com/x.png", false),
            ("https://example.com/x.png", true),
            ("smb://host/x.png", false),
            ("", false),
        ] {
            assert_eq!(classify_href(href, windows), Href::Refused, "{href}");
        }
        assert_eq!(
            classify_href("img/a.png", false),
            Href::Path("img/a.png".into())
        );
        assert_eq!(
            classify_href(r"C:\pics\a.png", true),
            Href::Path(r"C:\pics\a.png".into())
        );
        assert_eq!(
            classify_href("file:///C:/pics/a%20b.png", true),
            Href::Path("C:/pics/a b.png".into())
        );
        assert_eq!(
            classify_href("file:///tmp/a.png", false),
            Href::Path("/tmp/a.png".into())
        );

        // On disk: inside the folder only, symlinks and `..` resolved.
        let root = tempfile::tempdir().unwrap();
        let inside = root.path().join("doc");
        std::fs::create_dir(&inside).unwrap();
        let dir = inside.canonicalize().unwrap();
        write(root.path(), "secret.png", &png());
        write(&inside, "ok.png", &png());
        assert!(local_only("ok.png", &dir).is_some());
        assert!(local_only(&dir.join("ok.png").to_string_lossy(), &dir).is_some());
        assert!(local_only("../secret.png", &dir).is_none(), "../ escapes");
        let outside_abs = root.path().join("secret.png");
        assert!(local_only(&outside_abs.to_string_lossy(), &dir).is_none());
        assert!(local_only("missing.png", &dir).is_none());
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&outside_abs, inside.join("link.png")).unwrap();
            assert!(local_only("link.png", &dir).is_none(), "symlink escapes");
        }

        // End to end: the outside image is simply not drawn.
        let svg = write(&inside, "leak.svg", &svg_with_image("../secret.png"));
        assert!(!embeds_an_image(&convert_file(&svg).unwrap()));
        let svg = write(
            &inside,
            "abs.svg",
            &svg_with_image(&outside_abs.to_string_lossy()),
        );
        assert!(!embeds_an_image(&convert_file(&svg).unwrap()));
    }

    /// NFR (security): an external entity cannot pull a file into the PDF.
    #[test]
    fn fr_052a_xxe_no_leak() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "secret.txt", b"TOP-SECRET-CONTENT");
        let secret = dir.path().join("secret.txt");
        let svg = format!(
            r#"<?xml version="1.0"?>
<!DOCTYPE svg [ <!ENTITY xxe SYSTEM "file://{}"> ]>
<svg xmlns="http://www.w3.org/2000/svg" width="100" height="40"><text x="0" y="20">&xxe;</text></svg>"#,
            secret.display()
        );
        let path = write(dir.path(), "xxe.svg", svg.as_bytes());
        // Either refused as invalid, or converted without the entity: never
        // with the file's contents.
        if let Ok(pdf) = convert_file(&path) {
            let bytes = std::fs::read(pdf).unwrap();
            assert!(!bytes.windows(10).any(|w| w == b"TOP-SECRET"));
        }
    }

    /// FR-052a: a file that is not SVG is `svg_invalid`, not a crash.
    #[test]
    fn fr_052a_malformed_conversion_failed() {
        let dir = tempfile::tempdir().unwrap();
        for (name, bytes) in [
            ("junk.svg", &b"definitely not xml"[..]),
            ("html.svg", &b"<html><body>hi</body></html>"[..]),
            ("broken.svgz", &[0x1f, 0x8b, 0x08, 0x00, 0x01][..]),
        ] {
            let path = write(dir.path(), name, bytes);
            let err = convert_file(&path).unwrap_err();
            assert!(
                matches!(&err, EngineError::ConversionFailed(f) if f.reason == Some("svg_invalid")),
                "{name}: {err:?}"
            );
        }
    }

    /// FR-052a: the route honours the cancel flag.
    #[test]
    fn fr_052a_cancel() {
        let dir = tempfile::tempdir().unwrap();
        let svg = write(dir.path(), "r.svg", RECT);
        let cancel = AtomicBool::new(true);
        let err = convert(&svg, &dir.path().join("o.pdf"), &cancel, None, &|_| {}).unwrap_err();
        assert!(matches!(err, EngineError::Cancelled), "{err:?}");
    }

    /// FR-052a: text is drawn with the system fonts (embedded, vector).
    #[test]
    fn fr_052a_text_svg_produces_a_parseable_pdf() {
        let dir = tempfile::tempdir().unwrap();
        let svg = write(
            dir.path(),
            "t.svg",
            br#"<svg xmlns="http://www.w3.org/2000/svg" width="300" height="60"><text x="10" y="40" font-size="30" font-family="sans-serif">Pluryprint</text></svg>"#,
        );
        assert!(has_text(&std::fs::read(&svg).unwrap()));
        assert!(!has_text(RECT));
        let pdf = convert_file(&svg).unwrap();
        mediabox(&pdf);
    }

    /// The worker's failure line round-trips its reason and params; a line
    /// with a reason of another route is ignored.
    #[test]
    fn worker_failure_line_round_trips() {
        let failure = Failure::new("svg_too_large", "too big").param("limit", "64 MB");
        let stderr = format!("some noise\n{}\n", report_failure(&failure));
        assert_eq!(parse_failure(&stderr), Some(failure));
        assert_eq!(
            parse_failure(r#"pluryprint-failure {"reason":"no_printer","detail":"x"}"#),
            None
        );
        assert_eq!(parse_failure("panicked at ..."), None);
    }

    /// Through the full router: an SVG job yields a one-page cache PDF.
    #[test]
    fn fr_052a_svg_through_convert_to_pdf() {
        let dir = tempfile::tempdir().unwrap();
        let svg = write(dir.path(), "r.svg", RECT);
        let cancel = AtomicBool::new(false);
        let out = super::super::convert_to_pdf(
            &svg,
            &dir.path().join("cache"),
            "job",
            &cancel,
            &|_| {},
            None,
            &super::super::PageSetup::default(),
            None,
        )
        .unwrap()
        .expect("a cache PDF");
        mediabox(&out);
    }
}
