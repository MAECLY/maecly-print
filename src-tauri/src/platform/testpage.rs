//! Branded MaeclyPrint test page — generated as a PDF so it carries the real
//! logo, brand colors, and typography, and prints with the user's configured
//! default paper/tray for the printer.

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

fn pts(coords: &[(f32, f32)]) -> Vec<LinePoint> {
    coords
        .iter()
        .map(|&(x, y)| LinePoint {
            p: Point::new(Mm(x), Mm(y)),
            bezier: false,
        })
        .collect()
}

fn text(ops: &mut Vec<Op>, x: f32, y: f32, size: f32, font: BuiltinFont, color: Rgb8, s: &str) {
    ops.push(Op::StartTextSection);
    ops.push(Op::SetTextCursor {
        pos: Point::new(Mm(x), Mm(y)),
    });
    ops.push(Op::SetFont {
        font: PdfFontHandle::Builtin(font),
        size: Pt(size),
    });
    ops.push(Op::SetFillColor { col: rgb(color) });
    ops.push(Op::ShowText {
        items: vec![TextItem::Text(s.to_string())],
    });
    ops.push(Op::EndTextSection);
}

fn rect(
    ops: &mut Vec<Op>,
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
        ops.push(Op::SetFillColor { col: rgb(c) });
    }
    if let Some((c, t)) = stroke {
        ops.push(Op::SetOutlineColor { col: rgb(c) });
        ops.push(Op::SetOutlineThickness { pt: Pt(t) });
    }
    ops.push(Op::DrawPolygon {
        polygon: Polygon {
            rings: vec![PolygonRing {
                points: pts(&[(x, y), (x + w, y), (x + w, y + h), (x, y + h)]),
            }],
            mode,
            winding_order: WindingOrder::NonZero,
        },
    });
}

fn line(ops: &mut Vec<Op>, coords: &[(f32, f32)], color: Rgb8, thickness: f32) {
    ops.push(Op::SetOutlineColor { col: rgb(color) });
    ops.push(Op::SetOutlineThickness { pt: Pt(thickness) });
    ops.push(Op::DrawLine {
        line: Line {
            points: pts(coords),
            is_closed: false,
        },
    });
}

/// The MaeclyPrint glyph (twin pages + accent chevron), ported from
/// `src/components/Brand.tsx` (24×24 viewBox), scaled to `size` mm with its top
/// at `oy_top`.
fn logo(ops: &mut Vec<Op>, ox: f32, oy_top: f32, size: f32) {
    let s = size / 24.0;
    let map = |gx: f32, gy: f32| (ox + gx * s, oy_top - gy * s);
    for &(gx, gy, gw, gh) in &[(2.5f32, 3.5f32, 13.0f32, 17.0f32), (8.5, 3.5, 13.0, 17.0)] {
        let (x0, y0) = map(gx, gy + gh); // bottom-left in PDF space
        rect(ops, x0, y0, gw * s, gh * s, Some(WHITE), Some((INK, 1.0)));
    }
    let chevron: Vec<(f32, f32)> = [(11.0, 8.0), (15.0, 13.0), (19.0, 8.0)]
        .iter()
        .map(|&(gx, gy)| map(gx, gy))
        .collect();
    line(ops, &chevron, ACCENT, 1.4);
    let underline: Vec<(f32, f32)> = [(11.0, 16.0), (19.0, 16.0)]
        .iter()
        .map(|&(gx, gy)| map(gx, gy))
        .collect();
    line(ops, &underline, INK, 1.0);
}

/// Render the branded test page to PDF bytes (A4).
pub fn render(info: &TestPageInfo) -> Vec<u8> {
    let mut ops: Vec<Op> = Vec::new();

    // Header: logo + two-color wordmark + tagline.
    logo(&mut ops, 20.0, 285.0, 14.0);
    ops.push(Op::StartTextSection);
    ops.push(Op::SetTextCursor {
        pos: Point::new(Mm(38.0), Mm(276.0)),
    });
    ops.push(Op::SetFont {
        font: PdfFontHandle::Builtin(BuiltinFont::HelveticaBold),
        size: Pt(24.0),
    });
    ops.push(Op::SetFillColor { col: rgb(INK) });
    ops.push(Op::ShowText {
        items: vec![TextItem::Text("Maecly".to_string())],
    });
    ops.push(Op::SetFillColor { col: rgb(ACCENT) });
    ops.push(Op::ShowText {
        items: vec![TextItem::Text("Print".to_string())],
    });
    ops.push(Op::EndTextSection);
    text(
        &mut ops,
        38.0,
        269.0,
        10.5,
        BuiltinFont::Helvetica,
        MUTED,
        "Open source batch printing for Windows and Mac.",
    );
    rect(&mut ops, 20.0, 262.0, 170.0, 0.6, Some(ACCENT), None);

    // Headline + confirmation.
    text(
        &mut ops,
        20.0,
        246.0,
        26.0,
        BuiltinFont::HelveticaBold,
        INK,
        "Printer Test Page",
    );
    text(
        &mut ops,
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
        rect(&mut ops, 20.0, y, 2.4, 2.4, Some(ACCENT), None);
        text(
            &mut ops,
            25.0,
            y - 0.3,
            11.0,
            BuiltinFont::HelveticaBold,
            INK,
            prop,
        );
    }

    // Settings box — printed with the app's defaults for this printer.
    rect(
        &mut ops,
        20.0,
        168.0,
        170.0,
        28.0,
        Some(PAPER),
        Some((BORDER, 0.6)),
    );
    text(
        &mut ops,
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
        text(&mut ops, 25.0, y, 10.0, BuiltinFont::Courier, MUTED, label);
        text(
            &mut ops,
            48.0,
            y,
            10.0,
            BuiltinFont::CourierBold,
            INK,
            value,
        );
    }

    // Color & alignment test strip.
    text(
        &mut ops,
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
    for (i, c) in swatches.iter().enumerate() {
        let x = 20.0 + i as f32 * 19.5;
        rect(
            &mut ops,
            x,
            132.0,
            18.0,
            14.0,
            Some(*c),
            Some((BORDER, 0.4)),
        );
    }

    // Marketing band (fills the lower third).
    rect(&mut ops, 20.0, 86.0, 170.0, 26.0, Some(PAPER), None);
    text(
        &mut ops,
        26.0,
        100.0,
        15.0,
        BuiltinFont::HelveticaBold,
        INK,
        "Select 5, 50, or 500 files. Right-click. They print.",
    );
    text(
        &mut ops,
        26.0,
        92.0,
        10.0,
        BuiltinFont::Helvetica,
        MUTED,
        "The commercial tools that do this cost $79-$149. MaeclyPrint is open source and free.",
    );

    // Registration crosshairs at the corners (alignment check).
    for &(cx, cy) in &[(12.0, 285.0), (198.0, 285.0), (12.0, 12.0), (198.0, 12.0)] {
        line(&mut ops, &[(cx - 3.0, cy), (cx + 3.0, cy)], INK, 0.4);
        line(&mut ops, &[(cx, cy - 3.0), (cx, cy + 3.0)], INK, 0.4);
    }

    // Footer.
    rect(&mut ops, 20.0, 18.0, 170.0, 0.4, Some(ACCENT), None);
    text(
        &mut ops,
        20.0,
        13.0,
        8.5,
        BuiltinFont::Courier,
        MUTED,
        "MaeclyPrint   |   github.com/MigelAngelEC/maecly-print   |   GPLv3",
    );

    let page = PdfPage::new(Mm(210.0), Mm(297.0), ops);
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
}
