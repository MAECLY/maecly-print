//! Pure-Rust image → single-page PDF converter.
//!
//! Decoding uses the `image` crate (codecs enabled in `Cargo.toml`: jpeg, png,
//! tiff, bmp, webp); the resulting pixels are embedded into a one-page PDF built
//! by hand with `lopdf`.
//!
//! Why hand-build with `lopdf` instead of `printpdf`'s image API: `printpdf` is
//! pulled in with `default-features = false`, which disables its `images`
//! feature — its image-XObject serialiser is then a `panic!` stub. `lopdf` is
//! already a direct dependency and lets us emit a minimal, correct image
//! XObject (`/DeviceRGB`, 8 bits-per-component) with no extra crates.
//!
//! Known limitations (documented, not bugs):
//! - **EXIF orientation is not applied.** The `image` crate does not auto-rotate
//!   from EXIF, and we do not read it, so a rotated photo prints in its stored
//!   orientation. Matches the crate's own default behaviour.
//! - **Multi-frame TIFF/animations: first frame only.** `decode()` yields the
//!   primary image; extra frames are ignored.
//! - **CMYK JPEGs** and other sub-formats the codec cannot handle surface as
//!   [`EngineError::ConversionFailed`] carrying the decoder's message.

use std::path::Path;

use image::{ImageReader, Limits, RgbImage};
use lopdf::{dictionary, Dictionary, Document, Object, Stream};

use crate::engine::EngineError;

/// The page is sized to the image's pixels at this DPI. The converter is
/// deliberately paper-agnostic: the printer driver does the real fitting to A4
/// / Letter / etc. via the scaling flags (see
/// `docs/04-architecture/print-engine.md`).
const CONVERT_DPI: f32 = 96.0;

/// Points per inch — the PDF user-space unit.
const PT_PER_INCH: f32 = 72.0;

/// Largest page dimension we will emit, in points. 14 400 pt = 200 in is the
/// classic Acrobat page-size ceiling; enormous images are scaled down uniformly
/// to fit rather than producing an out-of-spec page.
const MAX_PAGE_PT: f32 = 14_400.0;

/// Decode `input` and write a single-page PDF wrapping it to `output`.
///
/// `progress` is called at `0.0` (before decode), `0.5` (before embedding) and
/// `1.0` (after save).
pub fn to_pdf(
    input: &Path,
    output: &Path,
    progress: &(dyn Fn(f32) + Send + Sync),
) -> Result<(), EngineError> {
    progress(0.0);

    let rgb = decode_rgb8(input)?;
    let (width, height) = (rgb.width(), rgb.height());
    if width == 0 || height == 0 {
        return Err(EngineError::ConversionFailed(
            "image has zero width or height".to_string(),
        ));
    }

    progress(0.5);

    let doc = build_single_page_pdf(width, height, rgb.into_raw());
    write_pdf(doc, output)?;

    progress(1.0);
    Ok(())
}

/// Decode any supported image to 8-bit RGB, with limits guarding against
/// decompression bombs (the `image` crate does not enforce dimension limits by
/// default).
fn decode_rgb8(input: &Path) -> Result<RgbImage, EngineError> {
    let mut reader = ImageReader::open(input).map_err(|e| {
        EngineError::ConversionFailed(format!("cannot open image {}: {e}", input.display()))
    })?;

    let mut limits = Limits::default();
    limits.max_image_width = Some(30_000);
    limits.max_image_height = Some(30_000);
    limits.max_alloc = Some(1024 * 1024 * 1024); // 1 GiB
    reader.limits(limits);

    // Sniff the real format from magic bytes rather than trusting the
    // extension, so e.g. a JPEG saved as `.png` still decodes.
    let reader = reader.with_guessed_format().map_err(|e| {
        EngineError::ConversionFailed(format!("cannot read image {}: {e}", input.display()))
    })?;

    let decoded = reader
        .decode()
        .map_err(|e| EngineError::ConversionFailed(e.to_string()))?;

    Ok(decoded.to_rgb8())
}

/// Build a one-page PDF whose only content is the given RGB image, scaled to
/// fill the page. `rgb` is row-major, top-to-bottom, 3 bytes per pixel.
fn build_single_page_pdf(width: u32, height: u32, rgb: Vec<u8>) -> Document {
    let (page_w_pt, page_h_pt) = page_size_pt(width, height);

    let mut doc = Document::with_version("1.5");

    // The image itself as a DeviceRGB, 8-bpc XObject. `compress()` applies
    // FlateDecode only when it actually shrinks the stream, so tiny fixtures
    // stay uncompressed (still valid) while real photos get deflated.
    let mut image_stream = Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Image",
            "Width" => Object::Integer(i64::from(width)),
            "Height" => Object::Integer(i64::from(height)),
            "ColorSpace" => "DeviceRGB",
            "BitsPerComponent" => Object::Integer(8),
        },
        rgb,
    );
    let _ = image_stream.compress();
    let image_id = doc.add_object(image_stream);

    // Content stream: map the image's 1×1 unit space onto the full page. The
    // first sample row lands at the top, so the image prints upright.
    let content = format!("q {page_w_pt:.4} 0 0 {page_h_pt:.4} 0 0 cm /Im0 Do Q");
    let content_id = doc.add_object(Stream::new(Dictionary::new(), content.into_bytes()));

    // Reserve the Pages id first so the Page can name it as its Parent.
    let pages_id = doc.new_object_id();
    let page_id = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => Object::Reference(pages_id),
        "MediaBox" => Object::Array(vec![
            Object::Real(0.0),
            Object::Real(0.0),
            Object::Real(page_w_pt),
            Object::Real(page_h_pt),
        ]),
        "Contents" => Object::Reference(content_id),
        "Resources" => dictionary! {
            "XObject" => dictionary! { "Im0" => Object::Reference(image_id) },
        },
    });
    doc.set_object(
        pages_id,
        dictionary! {
            "Type" => "Pages",
            "Kids" => Object::Array(vec![Object::Reference(page_id)]),
            "Count" => Object::Integer(1),
        },
    );

    let catalog_id = doc.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => Object::Reference(pages_id),
    });
    doc.trailer.set("Root", Object::Reference(catalog_id));

    doc
}

/// Page size in points: image pixels at [`CONVERT_DPI`], scaled down uniformly
/// if the longer side would exceed [`MAX_PAGE_PT`].
fn page_size_pt(width: u32, height: u32) -> (f32, f32) {
    let mut w = width as f32 * PT_PER_INCH / CONVERT_DPI;
    let mut h = height as f32 * PT_PER_INCH / CONVERT_DPI;
    let longest = w.max(h);
    if longest > MAX_PAGE_PT {
        let scale = MAX_PAGE_PT / longest;
        w *= scale;
        h *= scale;
    }
    (w, h)
}

fn write_pdf(mut doc: Document, output: &Path) -> Result<(), EngineError> {
    let mut file = std::fs::File::create(output).map_err(|e| {
        EngineError::ConversionFailed(format!("cannot create {}: {e}", output.display()))
    })?;
    doc.save_to(&mut file).map_err(|e| {
        EngineError::ConversionFailed(format!("cannot write PDF {}: {e}", output.display()))
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[test]
    fn writes_a_parseable_single_page_pdf_and_reports_progress() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("gradient.png");
        let mut img = RgbImage::new(6, 9); // portrait: height > width
        for (x, y, px) in img.enumerate_pixels_mut() {
            *px = image::Rgb([(x * 40) as u8, (y * 24) as u8, 200]);
        }
        img.save_with_format(&src, image::ImageFormat::Png).unwrap();

        let calls = Mutex::new(Vec::new());
        let progress = |p: f32| calls.lock().unwrap().push(p);
        let out = dir.path().join("out.pdf");
        to_pdf(&src, &out, &progress).unwrap();

        let doc = lopdf::Document::load(&out).unwrap();
        assert_eq!(doc.get_pages().len(), 1);

        let seen = calls.lock().unwrap().clone();
        assert_eq!(seen.first().copied(), Some(0.0));
        assert_eq!(seen.last().copied(), Some(1.0));
    }

    #[test]
    fn page_size_preserves_aspect_ratio() {
        // 192×96 px at 96 DPI → 144×72 pt (a 2:1 landscape page).
        let (w, h) = page_size_pt(192, 96);
        assert!((w - 144.0).abs() < 0.01);
        assert!((h - 72.0).abs() < 0.01);
    }

    #[test]
    fn page_size_clamps_enormous_images() {
        let (w, h) = page_size_pt(1_000_000, 500_000);
        assert!(w <= MAX_PAGE_PT + 0.01);
        assert!(h <= MAX_PAGE_PT + 0.01);
        // Aspect ratio (2:1) is preserved through the clamp.
        assert!((w / h - 2.0).abs() < 0.01);
    }
}
