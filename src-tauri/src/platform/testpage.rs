//! Branded MaeclyPrint test page — generated as a PDF so it carries the real
//! logo, brand colors, and typography, and prints with the user's configured
//! default paper/tray for the printer.
//!
//! The page is **laid out on a 210×297 mm design grid** and then uniformly
//! scaled and centered onto the media the user actually configured
//! ([`media_size_mm`]). Emitting a fixed A4 PDF and asking the spooler for
//! `media=Letter` used to leave the driver to scale it, which silently
//! invalidated the corner registration marks — the one element on the page
//! whose whole job is to be dimensionally true.

use printpdf::*;

/// What to render on the page (the printer plus the app's default settings).
pub struct TestPageInfo {
    pub printer: String,
    pub paper: Option<String>,
    pub tray: Option<String>,
}

// --- Brand palette (from src/styles/tokens.css) ---
type Rgb8 = (u8, u8, u8);
const INK: Rgb8 = (26, 24, 20); // #1a1814
const ACCENT: Rgb8 = (194, 65, 12); // #c2410c
const MUTED: Rgb8 = (107, 102, 93); // #6b665d
const BORDER: Rgb8 = (201, 195, 183); // #c9c3b7
const PAPER: Rgb8 = (243, 241, 236); // #f3f1ec
const WHITE: Rgb8 = (255, 255, 255);

/// printpdf's builtin fonts pass raw UTF-8 bytes, so non-ASCII characters
/// render as mojibake. Keep dynamic text (printer/paper/tray names) ASCII-safe.
fn ascii(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_ascii() { c } else { '?' })
        .collect()
}

fn rgb(c: Rgb8) -> Color {
    Color::Rgb(Rgb {
        r: c.0 as f32 / 255.0,
        g: c.1 as f32 / 255.0,
        b: c.2 as f32 / 255.0,
        icc_profile: None,
    })
}

/// The grid every coordinate below is expressed in (A4, portrait, mm).
const DESIGN_W: f32 = 210.0;
const DESIGN_H: f32 = 297.0;

/// Media dimensions in mm for a driver/PWG media name, defaulting to A4.
///
/// `paper` is whatever the driver called the size, so three shapes are
/// accepted, in order: an embedded PWG dimension pair (`iso_a4_210x297mm`,
/// `na_letter_8.5x11in`), a known size keyword (`A4`, `US Legal`), else A4.
pub fn media_size_mm(paper: Option<&str>) -> (f32, f32) {
    let raw = paper.unwrap_or("").trim().to_ascii_lowercase();
    if let Some(dims) = pwg_dimensions(&raw) {
        return dims;
    }
    let key = raw.replace(['_', '-', ' '], "");
    // `legal` before `letter`: `na_legal` must not fall through to Letter.
    if key.contains("legal") {
        (215.9, 355.6)
    } else if key.contains("letter") || key.contains("ansia") {
        (215.9, 279.4)
    } else if key.contains("a3") {
        (297.0, 420.0)
    } else if key.contains("a5") {
        (148.0, 210.0)
    } else if key.contains("executive") {
        (184.15, 266.7)
    } else {
        (DESIGN_W, DESIGN_H)
    }
}

/// Pull `210x297mm` / `8.5x11in` out of a PWG self-describing media name.
fn pwg_dimensions(name: &str) -> Option<(f32, f32)> {
    let (head, unit) = match name.strip_suffix("mm") {
        Some(h) => (h, 1.0),
        None => (name.strip_suffix("in")?, 25.4),
    };
    let tail = head.rsplit('_').next()?;
    let (w, h) = tail.split_once('x')?;
    let w: f32 = w.parse().ok()?;
    let h: f32 = h.parse().ok()?;
    // Reject nonsense (a stray "1x1in" label) rather than emitting a stamp-
    // sized page the layout cannot survive.
    let (w, h) = (w * unit, h * unit);
    (w >= 50.0 && h >= 50.0 && w <= 2000.0 && h <= 2000.0).then_some((w, h))
}

/// Drawing surface that maps the fixed A4 design grid onto the real media.
///
/// Every public method takes design-grid millimetres; the canvas applies a
/// single uniform scale plus centering offsets, so the composition (and the
/// ratio between text size and box size) is identical on every media, and the
/// page can never be clipped by a smaller sheet.
struct Canvas {
    ops: Vec<Op>,
    scale: f32,
    ox: f32,
    oy: f32,
}

impl Canvas {
    fn new(page_w: f32, page_h: f32) -> Self {
        let scale = (page_w / DESIGN_W).min(page_h / DESIGN_H);
        Self {
            ops: Vec::new(),
            scale,
            ox: (page_w - DESIGN_W * scale) / 2.0,
            oy: (page_h - DESIGN_H * scale) / 2.0,
        }
    }

    fn px(&self, x: f32) -> f32 {
        self.ox + x * self.scale
    }

    fn py(&self, y: f32) -> f32 {
        self.oy + y * self.scale
    }

    fn pts(&self, coords: &[(f32, f32)]) -> Vec<LinePoint> {
        coords
            .iter()
            .map(|&(x, y)| LinePoint {
                p: Point::new(Mm(self.px(x)), Mm(self.py(y))),
                bezier: false,
            })
            .collect()
    }

    fn text(&mut self, x: f32, y: f32, size: f32, font: BuiltinFont, color: Rgb8, s: &str) {
        let pos = Point::new(Mm(self.px(x)), Mm(self.py(y)));
        let size = Pt(size * self.scale);
        self.ops.push(Op::StartTextSection);
        self.ops.push(Op::SetTextCursor { pos });
        self.ops.push(Op::SetFont {
            font: PdfFontHandle::Builtin(font),
            size,
        });
        self.ops.push(Op::SetFillColor { col: rgb(color) });
        self.ops.push(Op::ShowText {
            items: vec![TextItem::Text(s.to_string())],
        });
        self.ops.push(Op::EndTextSection);
    }

    fn rect(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        fill: Option<Rgb8>,
        stroke: Option<(Rgb8, f32)>,
    ) {
        let mode = match (fill.is_some(), stroke.is_some()) {
            (true, true) => PaintMode::FillStroke,
            (false, true) => PaintMode::Stroke,
            _ => PaintMode::Fill,
        };
        if let Some(c) = fill {
            self.ops.push(Op::SetFillColor { col: rgb(c) });
        }
        if let Some((c, t)) = stroke {
            self.ops.push(Op::SetOutlineColor { col: rgb(c) });
            self.ops.push(Op::SetOutlineThickness {
                pt: Pt(t * self.scale),
            });
        }
        let points = self.pts(&[(x, y), (x + w, y), (x + w, y + h), (x, y + h)]);
        self.ops.push(Op::DrawPolygon {
            polygon: Polygon {
                rings: vec![PolygonRing { points }],
                mode,
                winding_order: WindingOrder::NonZero,
            },
        });
    }

    fn line(&mut self, coords: &[(f32, f32)], color: Rgb8, thickness: f32) {
        self.ops.push(Op::SetOutlineColor { col: rgb(color) });
        self.ops.push(Op::SetOutlineThickness {
            pt: Pt(thickness * self.scale),
        });
        let points = self.pts(coords);
        self.ops.push(Op::DrawLine {
            line: Line {
                points,
                is_closed: false,
            },
        });
    }

    /// A line in **real page** millimetres, bypassing the design transform.
    /// Used only by the registration crosshairs, whose whole purpose is to sit
    /// a known distance from the physical sheet edge.
    fn page_line(&mut self, coords: &[(f32, f32)], color: Rgb8, thickness: f32) {
        self.ops.push(Op::SetOutlineColor { col: rgb(color) });
        self.ops.push(Op::SetOutlineThickness { pt: Pt(thickness) });
        self.ops.push(Op::DrawLine {
            line: Line {
                points: coords
                    .iter()
                    .map(|&(x, y)| LinePoint {
                        p: Point::new(Mm(x), Mm(y)),
                        bezier: false,
                    })
                    .collect(),
                is_closed: false,
            },
        });
    }

    /// The MaeclyPrint glyph (twin pages + accent chevron), ported from
    /// `src/components/Brand.tsx` (24×24 viewBox), scaled to `size` mm of the
    /// design grid with its top at `oy_top`.
    fn logo(&mut self, ox: f32, oy_top: f32, size: f32) {
        let s = size / 24.0;
        let map = |gx: f32, gy: f32| (ox + gx * s, oy_top - gy * s);
        for &(gx, gy, gw, gh) in &[(2.5f32, 3.5f32, 13.0f32, 17.0f32), (8.5, 3.5, 13.0, 17.0)] {
            let (x0, y0) = map(gx, gy + gh); // bottom-left in PDF space
            self.rect(x0, y0, gw * s, gh * s, Some(WHITE), Some((INK, 1.0)));
        }
        let chevron: Vec<(f32, f32)> = [(11.0, 8.0), (15.0, 13.0), (19.0, 8.0)]
            .iter()
            .map(|&(gx, gy)| map(gx, gy))
            .collect();
        self.line(&chevron, ACCENT, 1.4);
        let underline: Vec<(f32, f32)> = [(11.0, 16.0), (19.0, 16.0)]
            .iter()
            .map(|&(gx, gy)| map(gx, gy))
            .collect();
        self.line(&underline, INK, 1.0);
    }
}

/// The footer credit line, composed from the crate's canonical repository URL
/// (`Cargo.toml`) rather than a literal — the same source `report_issue` and
/// the updater's repo-slug parser already use, so the address printed on paper
/// can never drift from the one the app links to.
fn footer_line() -> String {
    let repo = env!("CARGO_PKG_REPOSITORY")
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_end_matches('/')
        .trim_end_matches(".git")
        .trim_end_matches('/');
    format!("MaeclyPrint   |   {repo}   |   GPLv3")
}

/// Render the branded test page to PDF bytes, sized to the configured media.
pub fn render(info: &TestPageInfo) -> Vec<u8> {
    let (page_w, page_h) = media_size_mm(info.paper.as_deref());
    let mut c = Canvas::new(page_w, page_h);

    // Header: logo + two-color wordmark + tagline.
    c.logo(20.0, 285.0, 14.0);
    let wordmark_pos = Point::new(Mm(c.px(38.0)), Mm(c.py(276.0)));
    let wordmark_size = Pt(24.0 * c.scale);
    c.ops.push(Op::StartTextSection);
    c.ops.push(Op::SetTextCursor { pos: wordmark_pos });
    c.ops.push(Op::SetFont {
        font: PdfFontHandle::Builtin(BuiltinFont::HelveticaBold),
        size: wordmark_size,
    });
    c.ops.push(Op::SetFillColor { col: rgb(INK) });
    c.ops.push(Op::ShowText {
        items: vec![TextItem::Text("Maecly".to_string())],
    });
    c.ops.push(Op::SetFillColor { col: rgb(ACCENT) });
    c.ops.push(Op::ShowText {
        items: vec![TextItem::Text("Print".to_string())],
    });
    c.ops.push(Op::EndTextSection);
    c.text(
        38.0,
        269.0,
        10.5,
        BuiltinFont::Helvetica,
        MUTED,
        "Open source batch printing for Windows and Mac.",
    );
    c.rect(20.0, 262.0, 170.0, 0.6, Some(ACCENT), None);

    // Headline + confirmation.
    c.text(
        20.0,
        246.0,
        26.0,
        BuiltinFont::HelveticaBold,
        INK,
        "Printer Test Page",
    );
    c.text(
        20.0,
        237.0,
        11.0,
        BuiltinFont::Helvetica,
        MUTED,
        "If you can read this clearly, your printer is working with MaeclyPrint.",
    );

    // Value-prop bullets.
    for (i, prop) in [
        "No 15-file limit",
        "No Adobe required",
        "No $149 license fee",
    ]
    .iter()
    .enumerate()
    {
        let y = 224.0 - i as f32 * 8.0;
        c.rect(20.0, y, 2.4, 2.4, Some(ACCENT), None);
        c.text(25.0, y - 0.3, 11.0, BuiltinFont::HelveticaBold, INK, prop);
    }

    // Settings box — printed with the app's defaults for this printer.
    c.rect(20.0, 168.0, 170.0, 28.0, Some(PAPER), Some((BORDER, 0.6)));
    c.text(
        25.0,
        189.0,
        9.0,
        BuiltinFont::HelveticaBold,
        MUTED,
        "PRINTED WITH YOUR MAECLYPRINT DEFAULTS",
    );
    let printer = ascii(&info.printer);
    let paper = ascii(info.paper.as_deref().unwrap_or("printer default"));
    let tray = ascii(info.tray.as_deref().unwrap_or("printer default"));
    for (i, (label, value)) in [
        ("Printer", printer.as_str()),
        ("Paper", paper.as_str()),
        ("Tray", tray.as_str()),
    ]
    .iter()
    .enumerate()
    {
        let y = 182.0 - i as f32 * 5.0;
        c.text(25.0, y, 10.0, BuiltinFont::Courier, MUTED, label);
        c.text(48.0, y, 10.0, BuiltinFont::CourierBold, INK, value);
    }

    // Color & alignment test strip.
    c.text(
        20.0,
        152.0,
        9.0,
        BuiltinFont::HelveticaBold,
        MUTED,
        "COLOR & ALIGNMENT",
    );
    let swatches: [Rgb8; 8] = [
        ACCENT,
        (0, 158, 224),
        (236, 0, 140),
        (255, 242, 0),
        (0, 0, 0),
        (64, 64, 64),
        (128, 128, 128),
        (191, 191, 191),
    ];
    for (i, col) in swatches.iter().enumerate() {
        let x = 20.0 + i as f32 * 19.5;
        c.rect(x, 132.0, 18.0, 14.0, Some(*col), Some((BORDER, 0.4)));
    }

    // Marketing band (fills the lower third).
    c.rect(20.0, 86.0, 170.0, 26.0, Some(PAPER), None);
    c.text(
        26.0,
        100.0,
        15.0,
        BuiltinFont::HelveticaBold,
        INK,
        "Select 5, 50, or 500 files. Right-click. They print.",
    );
    c.text(
        26.0,
        92.0,
        10.0,
        BuiltinFont::Helvetica,
        MUTED,
        "The commercial tools that do this cost $79-$149. MaeclyPrint is open source and free.",
    );

    // Registration crosshairs 12 mm from every PHYSICAL edge — they measure the
    // sheet, not the design grid, so they stay true on non-A4 media.
    for &(cx, cy) in &[
        (12.0, page_h - 12.0),
        (page_w - 12.0, page_h - 12.0),
        (12.0, 12.0),
        (page_w - 12.0, 12.0),
    ] {
        c.page_line(&[(cx - 3.0, cy), (cx + 3.0, cy)], INK, 0.4);
        c.page_line(&[(cx, cy - 3.0), (cx, cy + 3.0)], INK, 0.4);
    }

    // Footer.
    c.rect(20.0, 18.0, 170.0, 0.4, Some(ACCENT), None);
    c.text(20.0, 13.0, 8.5, BuiltinFont::Courier, MUTED, &footer_line());

    let page = PdfPage::new(Mm(page_w), Mm(page_h), c.ops);
    let mut doc = PdfDocument::new("MaeclyPrint Test Page");
    doc.with_pages(vec![page]);
    let mut warnings = Vec::new();
    doc.save(&PdfSaveOptions::default(), &mut warnings)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_a_valid_pdf() {
        let pdf = render(&TestPageInfo {
            printer: "EPSON L15160 Series".into(),
            paper: Some("A4".into()),
            tray: Some("Tray 1".into()),
        });
        assert!(pdf.starts_with(b"%PDF"), "output should be a PDF");
        assert!(pdf.len() > 1500, "page should have content");
    }

    #[test]
    fn ascii_replaces_non_ascii() {
        assert_eq!(ascii("Café · São"), "Caf? ? S?o");
    }

    /// The address printed on paper must be the project's canonical repository,
    /// not a hand-typed literal that can drift from `Cargo.toml`.
    #[test]
    fn fr_footer_uses_the_canonical_repository_url() {
        let footer = footer_line();
        let canonical = env!("CARGO_PKG_REPOSITORY")
            .trim_start_matches("https://")
            .trim_end_matches('/');
        assert!(
            footer.contains(canonical),
            "footer should carry {canonical}: {footer}"
        );
        // The literal it replaced pointed at a personal fork.
        assert!(!footer.contains("MigelAngelEC"), "{footer}");
        assert!(!footer.contains("https://"), "{footer}");
        assert!(footer.starts_with("MaeclyPrint   |   "), "{footer}");
        assert!(footer.ends_with("   |   GPLv3"), "{footer}");
    }

    #[test]
    fn fr_media_size_reads_keywords_and_pwg_names() {
        assert_eq!(media_size_mm(Some("A4")), (210.0, 297.0));
        assert_eq!(media_size_mm(Some("iso_a4_210x297mm")), (210.0, 297.0));
        assert_eq!(media_size_mm(Some("A3")), (297.0, 420.0));
        assert_eq!(media_size_mm(Some("US Legal")), (215.9, 355.6));
        assert_eq!(media_size_mm(Some("Letter")), (215.9, 279.4));
        // PWG inches convert to mm.
        let (w, h) = media_size_mm(Some("na_letter_8.5x11in"));
        assert!(
            (w - 215.9).abs() < 0.1 && (h - 279.4).abs() < 0.1,
            "{w}x{h}"
        );
        // Unknown / absent media degrade to A4, never to zero.
        assert_eq!(media_size_mm(Some("Photo 4x6")), (210.0, 297.0));
        assert_eq!(media_size_mm(None), (210.0, 297.0));
        // A nonsense dimension pair is rejected instead of shrinking the page.
        assert_eq!(media_size_mm(Some("custom_1x1mm")), (210.0, 297.0));
    }

    /// Regression: the page used to be hard-coded to A4 while the spooler was
    /// asked for `media=Letter`, so the driver rescaled it behind our back.
    #[test]
    fn fr_regression_page_geometry_follows_the_configured_paper() {
        for (paper, expect_mm) in [
            ("A4", (210.0f32, 297.0f32)),
            ("Letter", (215.9, 279.4)),
            ("A3", (297.0, 420.0)),
        ] {
            let (w, h) = media_size_mm(Some(paper));
            assert_eq!((w, h), expect_mm);
            let pdf = render(&TestPageInfo {
                printer: "Front desk".into(),
                paper: Some(paper.into()),
                tray: None,
            });
            assert!(pdf.starts_with(b"%PDF"), "{paper} should render a PDF");
            assert!(pdf.len() > 1500, "{paper} page should have content");
        }
    }

    /// The design must never overflow the sheet: the uniform scale keeps the
    /// whole 210×297 grid inside the media, whatever its aspect ratio.
    #[test]
    fn nfr_canvas_never_places_content_outside_the_page() {
        for (w, h) in [(210.0f32, 297.0f32), (215.9, 279.4), (297.0, 420.0)] {
            let c = Canvas::new(w, h);
            for &(x, y) in &[(0.0f32, 0.0f32), (DESIGN_W, DESIGN_H)] {
                assert!(c.px(x) >= -0.001 && c.px(x) <= w + 0.001, "x {}", c.px(x));
                assert!(c.py(y) >= -0.001 && c.py(y) <= h + 0.001, "y {}", c.py(y));
            }
        }
    }
}
