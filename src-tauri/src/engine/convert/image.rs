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
//! - **Multi-frame TIFF: first frame only.** `decode()` yields the primary
//!   image; extra frames are ignored. (GIF is not a supported format at all,
//!   decision D9.)
//! - **Transparency is flattened onto white**, as on paper: an image with an
//!   alpha channel (PNG, WebP, TIFF) is composited before it is embedded.
//! - **CMYK JPEGs** and other sub-formats the codec cannot handle surface as
//!   [`EngineError::ConversionFailed`] carrying the decoder's message.

use std::path::Path;

use image::{ImageReader, Limits, Rgb, RgbImage, RgbaImage};
use lopdf::{dictionary, Dictionary, Document, Object, Stream};

use crate::engine::{EngineError, Failure};

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
        return Err(EngineError::ConversionFailed(Failure::new(
            "image_damaged",
            "image has zero width or height",
        )));
    }

    progress(0.5);

    rgb_to_pdf(width, height, rgb.into_raw(), output)?;

    progress(1.0);
    Ok(())
}

/// Write already-decoded 8-bit RGB pixels (row-major, top-to-bottom) as a
/// one-page PDF. Shared with decoders that do not go through the `image`
/// crate (the Windows HEIF path, see [`super::heif`]).
pub fn rgb_to_pdf(width: u32, height: u32, rgb: Vec<u8>, output: &Path) -> Result<(), EngineError> {
    if width == 0 || height == 0 || rgb.len() != width as usize * height as usize * 3 {
        return Err(EngineError::ConversionFailed(Failure::new(
            "image_damaged",
            "decoded image has an inconsistent size",
        )));
    }
    write_pdf(build_single_page_pdf(width, height, rgb), output)
}

/// Decode any supported image to 8-bit RGB, with limits guarding against
/// decompression bombs (the `image` crate does not enforce dimension limits by
/// default).
fn decode_rgb8(input: &Path) -> Result<RgbImage, EngineError> {
    let mut reader = ImageReader::open(input).map_err(|e| cannot_read(input, "open", e))?;

    let mut limits = Limits::default();
    limits.max_image_width = Some(30_000);
    limits.max_image_height = Some(30_000);
    limits.max_alloc = Some(1024 * 1024 * 1024); // 1 GiB
    reader.limits(limits);

    // Sniff the real format from magic bytes rather than trusting the
    // extension, so e.g. a JPEG saved as `.png` still decodes.
    let reader = reader
        .with_guessed_format()
        .map_err(|e| cannot_read(input, "read", e))?;

    let decoded = reader
        .decode()
        .map_err(|e| EngineError::ConversionFailed(Failure::new("image_damaged", e.to_string())))?;

    // Paper is white: an image with an alpha channel is composited onto white
    // before it is embedded. `to_rgb8()` alone drops alpha, so transparent
    // pixels stored as `[0, 0, 0, 0]` (most logos) printed as a black box.
    Ok(if decoded.color().has_alpha() {
        flatten_on_white(decoded.to_rgba8())
    } else {
        decoded.to_rgb8()
    })
}

/// Composite straight-alpha RGBA pixels onto an opaque white background
/// (`out = c·a + 255·(1 − a)`, rounded). Pure.
fn flatten_on_white(rgba: RgbaImage) -> RgbImage {
    let (width, height) = rgba.dimensions();
    RgbImage::from_fn(width, height, |x, y| {
        let [r, g, b, a] = rgba.get_pixel(x, y).0;
        let a = u16::from(a);
        let over = |c: u8| ((u16::from(c) * a + 255 * (255 - a) + 127) / 255) as u8;
        Rgb([over(r), over(g), over(b)])
    })
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
    let cannot_write = |e: String| {
        EngineError::ConversionFailed(Failure::new(
            "cannot_write_output",
            format!("cannot write PDF {}: {e}", output.display()),
        ))
    };
    let mut file = std::fs::File::create(output).map_err(|e| cannot_write(e.to_string()))?;
    doc.save_to(&mut file)
        .map_err(|e| cannot_write(e.to_string()))?;
    Ok(())
}

/// The image at `input` could not be opened or read (`verb`), naming the file
/// (never its folder) for the UI.
fn cannot_read(input: &Path, verb: &str, e: std::io::Error) -> EngineError {
    let name = input
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    EngineError::ConversionFailed(
        Failure::new(
            "cannot_read_input",
            format!("cannot {verb} image {}: {e}", input.display()),
        )
        .param("file", name),
    )
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

    /// The `/Im0` samples of the one-page PDF at `pdf`, decompressed.
    fn image_samples(pdf: &Path) -> Vec<u8> {
        let doc = lopdf::Document::load(pdf).unwrap();
        let page = *doc.get_pages().values().next().unwrap();
        let resources = doc
            .get_dictionary(page)
            .unwrap()
            .get(b"Resources")
            .unwrap()
            .as_dict()
            .unwrap();
        let id = resources
            .get(b"XObject")
            .unwrap()
            .as_dict()
            .unwrap()
            .get(b"Im0")
            .unwrap()
            .as_reference()
            .unwrap();
        let stream = doc.get_object(id).unwrap().as_stream().unwrap();
        stream
            .decompressed_content()
            .unwrap_or_else(|_| stream.content.clone())
    }

    /// FR-052 (F1): transparent pixels print white, not black. `to_rgb8()`
    /// dropped the alpha channel, so a logo whose transparent pixels are
    /// stored as `[0, 0, 0, 0]` came out on a black box.
    #[test]
    fn fr_052_transparent_rgba_png_flattens_on_white() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("logo.png");
        let mut img = image::RgbaImage::new(3, 1);
        img.put_pixel(0, 0, image::Rgba([0, 0, 0, 0])); // fully transparent
        img.put_pixel(1, 0, image::Rgba([255, 0, 0, 255])); // opaque red
        img.put_pixel(2, 0, image::Rgba([0, 0, 0, 128])); // half-transparent black
        img.save_with_format(&src, image::ImageFormat::Png).unwrap();

        let out = dir.path().join("out.pdf");
        to_pdf(&src, &out, &|_| {}).unwrap();
        let samples = image_samples(&out);
        assert_eq!(&samples[0..3], &[255, 255, 255], "transparent → white");
        assert_eq!(&samples[3..6], &[255, 0, 0], "opaque pixels unchanged");
        assert_eq!(&samples[6..9], &[127, 127, 127], "50 % black over white");
    }

    /// FR-052: the same for an image with alpha in another codec (WebP
    /// lossless keeps RGBA) and for a 16-bit RGBA PNG.
    #[test]
    fn fr_052_alpha_in_webp_and_16_bit_png_flattens_on_white() {
        let dir = tempfile::tempdir().unwrap();
        let webp = dir.path().join("a.webp");
        let mut img = image::RgbaImage::new(2, 2);
        for px in img.pixels_mut() {
            *px = image::Rgba([0, 0, 0, 0]);
        }
        img.save_with_format(&webp, image::ImageFormat::WebP)
            .unwrap();
        let out = dir.path().join("w.pdf");
        to_pdf(&webp, &out, &|_| {}).unwrap();
        assert!(image_samples(&out).iter().all(|&b| b == 255));

        let png16 = dir.path().join("b.png");
        let img16: image::ImageBuffer<image::Rgba<u16>, Vec<u16>> =
            image::ImageBuffer::from_pixel(2, 2, image::Rgba([0, 0, 0, 0]));
        image::DynamicImage::ImageRgba16(img16)
            .save_with_format(&png16, image::ImageFormat::Png)
            .unwrap();
        let out = dir.path().join("p.pdf");
        to_pdf(&png16, &out, &|_| {}).unwrap();
        assert!(image_samples(&out).iter().all(|&b| b == 255));
    }

    /// The flattening itself, pixel by pixel. Pure.
    #[test]
    fn fr_052_flatten_on_white_is_alpha_compositing() {
        let mut rgba = image::RgbaImage::new(4, 1);
        rgba.put_pixel(0, 0, image::Rgba([10, 20, 30, 255]));
        rgba.put_pixel(1, 0, image::Rgba([10, 20, 30, 0]));
        rgba.put_pixel(2, 0, image::Rgba([0, 0, 255, 51])); // 20 % blue
        rgba.put_pixel(3, 0, image::Rgba([255, 255, 255, 128]));
        let rgb = flatten_on_white(rgba);
        assert_eq!(rgb.get_pixel(0, 0).0, [10, 20, 30]);
        assert_eq!(rgb.get_pixel(1, 0).0, [255, 255, 255]);
        assert_eq!(rgb.get_pixel(2, 0).0, [204, 204, 255]);
        assert_eq!(rgb.get_pixel(3, 0).0, [255, 255, 255]);
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
