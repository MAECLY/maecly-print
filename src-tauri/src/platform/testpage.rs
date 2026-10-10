//! Branded Pluryprint test page — generated as a PDF so it carries the real
//! logo, brand colors, and typography, and prints with the user's configured
//! default paper for the printer — plus its tray where the platform's spool path
//! can actually select one ([`TRAY_IS_APPLIED`]).
//!
//! The page is **laid out on a 210×297 mm design grid** and then uniformly
//! scaled and centered onto the media the user actually configured
//! ([`media_size_mm`]). Emitting a fixed A4 PDF and asking the spooler for
//! `media=Letter` used to leave the driver to scale it, which silently
//! invalidated the corner registration marks — the one element on the page
//! whose whole job is to be dimensionally true.
//!
//! The sheet is printed in the app's language (FR-090/FR-092): its sentences
//! are the `testPage` section of the UI catalogs (`src/i18n/locales/<lang>/
//! common.json`), compiled in, with English as the fallback. The builtin PDF
//! fonts are declared WinAnsi-encoded, which covers Spanish (`á`, `ñ`, `¿`,
//! `«»`), so no font has to be embedded. printpdf 0.9 writes a builtin font's
//! text as raw UTF-8 bytes, though, which printed `PÃ¡gina`; so text is
//! shown with a `Tj` of the Windows-1252 bytes ([`Canvas::text`]), and a
//! character outside WinAnsi becomes `?` ([`win_ansi`]).

use printpdf::*;

/// The catalogs, compiled in so the page needs no frontend at run time.
const EN_COMMON: &str = include_str!("../../../src/i18n/locales/en/common.json");
const ES_COMMON: &str = include_str!("../../../src/i18n/locales/es/common.json");

/// The page's sentences in one language (catalog section `testPage`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Strings {
    /// PDF title, and the job title the spooler shows.
    pub title: String,
    tagline: String,
    headline: String,
    confirm: String,
    props: [String; 3],
    defaults: String,
    printer: String,
    paper: String,
    tray: String,
    printer_default: String,
    color: String,
    band: String,
    band_note: String,
}

impl Strings {
    /// The sentences for the app locale `locale` (`es-ES` → Spanish); any
    /// locale without a catalog, or a key missing from one, is English.
    pub fn for_locale(locale: &str) -> Self {
        let lang = locale
            .split(['-', '_'])
            .next()
            .unwrap_or("")
            .to_ascii_lowercase();
        let parse = |raw: &str| serde_json::from_str::<serde_json::Value>(raw).ok();
        let catalog = match lang.as_str() {
            "es" => parse(ES_COMMON),
            _ => None,
        };
        let english = parse(EN_COMMON);
        let get = |key: &str| -> String {
            let pick = |doc: &Option<serde_json::Value>| {
                doc.as_ref()?
                    .get("testPage")?
                    .get(key)?
                    .as_str()
                    .map(str::to_string)
            };
            pick(&catalog)
                .or_else(|| pick(&english))
                .unwrap_or_default()
        };
        Self {
            title: get("title"),
            tagline: get("tagline"),
            headline: get("headline"),
            confirm: get("confirm"),
            props: [get("prop1"), get("prop2"), get("prop3")],
            defaults: get("defaults"),
            printer: get("printer"),
            paper: get("paper"),
            tray: get("tray"),
            printer_default: get("printerDefault"),
            color: get("color"),
            band: get("band"),
            band_note: get("bandNote"),
        }
    }
}

/// What to render on the page (the printer plus the app's default settings).
pub struct TestPageInfo {
    /// The app's UI locale (`config.toml` `app.locale`): the page's language.
    pub locale: String,
    /// Queue name of the printer the page is being printed on.
    pub printer: String,
    /// The configured paper size, or `None` for the printer's default media.
    pub paper: Option<String>,
    /// The configured tray override for this printer, or `None` for "printer
    /// default". Whether it is *printed* depends on [`TRAY_IS_APPLIED`]: the
    /// sheet only claims a tray on platforms whose test-page path really sends
    /// one.
    pub tray: Option<String>,
}

/// Does this platform's test-page path actually select the configured input
/// tray on the device?
///
/// - **Unix (CUPS):** yes — `platform::cups::print_test_page` passes
///   `InputSlot=<tray>` to `lp`.
/// - **Windows (SumatraPDF):** no — `platform::windows::print_test_page` builds
///   its [`crate::queue::job::JobSettings`] from the paper override alone and
///   leaves `tray` unset, so nothing reaches `-print-settings bin=`. The backend
///   itself *can* express a bin now (`engine::print::sumatra::bin_token`); until
///   the Windows path forwards the override, the page must not say a tray was
///   used, because none was.
///
/// The sheet is a physical artifact: once it is out of the printer, a wrong line
/// on it can never be corrected. So it states only what the code on this
/// platform genuinely does — the "PRINTED WITH YOUR PLURYPRINT DEFAULTS" header
/// is a claim about this print, not a listing of preferences.
pub const TRAY_IS_APPLIED: bool = cfg!(unix);

// --- Brand palette (from src/styles/tokens.css) ---
type Rgb8 = (u8, u8, u8);
const INK: Rgb8 = (26, 24, 20); // #1a1814
const ACCENT: Rgb8 = (194, 65, 12); // #c2410c
const MUTED: Rgb8 = (107, 102, 93); // #6b665d
const BORDER: Rgb8 = (201, 195, 183); // #c9c3b7
const PAPER: Rgb8 = (243, 241, 236); // #f3f1ec
const WHITE: Rgb8 = (255, 255, 255);

/// The characters WinAnsi (Windows-1252) adds in 0x80–0x9F; with printable
/// ASCII and Latin-1 (U+00A0–U+00FF) they are everything printpdf can encode
/// for a builtin font.
const WIN_ANSI_EXTRA: &str = "€‚ƒ„…†‡ˆ‰Š‹ŒŽ‘’“”•–—˜™š›œžŸ";

/// A `Tj` showing `s` in the builtin fonts' WinAnsi encoding (the bytes
/// printpdf's own `ShowText` gets wrong for anything past ASCII). `s` should
/// already be [`win_ansi`]; anything unmappable would become `?` anyway.
fn show_win_ansi(s: &str) -> Op {
    let text = win_ansi(s);
    let (bytes, _, _) = encoding_rs::WINDOWS_1252.encode(&text);
    Op::Unknown {
        key: "Tj".to_string(),
        value: vec![DictItem::String {
            data: bytes.into_owned(),
            literal: false,
        }],
    }
}

/// The save options: printpdf's defaults, except that our own `Tj`
/// ([`show_win_ansi`]) is written instead of skipped as an unknown operator.
/// Every operator on this page is built here, none comes from input.
fn save_options() -> PdfSaveOptions {
    PdfSaveOptions {
        secure: false,
        ..PdfSaveOptions::default()
    }
}

/// The builtin fonts can print WinAnsi (Windows-1252) only, so a character
/// outside it (CJK, emoji in a printer name) becomes `?` instead of garbage.
fn win_ansi(s: &str) -> String {
    s.chars()
        .map(|c| {
            let ok = (' '..='~').contains(&c)
                || ('\u{A0}'..='\u{FF}').contains(&c)
                || WIN_ANSI_EXTRA.contains(c);
            if ok {
                c
            } else {
                '?'
            }
        })
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
        self.ops.push(show_win_ansi(s));
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

    /// The Pluryprint glyph (twin pages + accent chevron), ported from
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
    format!("Pluryprint   |   {repo}   |   AGPL-3.0")
}

/// What the sheet's `Tray` row may honestly say: the configured bin name only
/// where the spool path really selects it ([`TRAY_IS_APPLIED`]), and otherwise
/// "printer default" (`printer_default`, in the page's language) — which is
/// exactly what the device did with the sheet.
fn tray_row_value(configured: Option<&str>, printer_default: &str) -> String {
    match configured.filter(|t| TRAY_IS_APPLIED && !t.trim().is_empty()) {
        Some(tray) => tray.trim().to_string(),
        None => printer_default.to_string(),
    }
}

/// Render the branded test page to PDF bytes, sized to the configured media.
pub fn render(info: &TestPageInfo) -> Vec<u8> {
    let s = Strings::for_locale(&info.locale);
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
        items: vec![TextItem::Text("Plury".to_string())],
    });
    c.ops.push(Op::SetFillColor { col: rgb(ACCENT) });
    c.ops.push(Op::ShowText {
        items: vec![TextItem::Text("print".to_string())],
    });
    c.ops.push(Op::EndTextSection);
    c.text(
        38.0,
        269.0,
        10.5,
        BuiltinFont::Helvetica,
        MUTED,
        &win_ansi(&s.tagline),
    );
    c.rect(20.0, 262.0, 170.0, 0.6, Some(ACCENT), None);

    // Headline + confirmation.
    c.text(
        20.0,
        246.0,
        26.0,
        BuiltinFont::HelveticaBold,
        INK,
        &win_ansi(&s.headline),
    );
    c.text(
        20.0,
        237.0,
        11.0,
        BuiltinFont::Helvetica,
        MUTED,
        &win_ansi(&s.confirm),
    );

    // Value-prop bullets.
    for (i, prop) in s.props.iter().enumerate() {
        let y = 224.0 - i as f32 * 8.0;
        c.rect(20.0, y, 2.4, 2.4, Some(ACCENT), None);
        c.text(
            25.0,
            y - 0.3,
            11.0,
            BuiltinFont::HelveticaBold,
            INK,
            &win_ansi(prop),
        );
    }

    // Settings box — printed with the app's defaults for this printer.
    c.rect(20.0, 168.0, 170.0, 28.0, Some(PAPER), Some((BORDER, 0.6)));
    c.text(
        25.0,
        189.0,
        9.0,
        BuiltinFont::HelveticaBold,
        MUTED,
        &win_ansi(&s.defaults),
    );
    let printer = win_ansi(&info.printer);
    let paper = win_ansi(info.paper.as_deref().unwrap_or(&s.printer_default));
    let tray = win_ansi(&tray_row_value(info.tray.as_deref(), &s.printer_default));
    for (i, (label, value)) in [
        (win_ansi(&s.printer), printer.as_str()),
        (win_ansi(&s.paper), paper.as_str()),
        (win_ansi(&s.tray), tray.as_str()),
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
        &win_ansi(&s.color),
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
        &win_ansi(&s.band),
    );
    c.text(
        26.0,
        92.0,
        10.0,
        BuiltinFont::Helvetica,
        MUTED,
        &win_ansi(&s.band_note),
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
    let mut doc = PdfDocument::new(&s.title);
    doc.with_pages(vec![page]);
    let mut warnings = Vec::new();
    doc.save(&save_options(), &mut warnings)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_a_valid_pdf() {
        let pdf = render(&TestPageInfo {
            locale: "en-US".into(),
            printer: "EPSON L15160 Series".into(),
            paper: Some("A4".into()),
            tray: Some("Tray 1".into()),
        });
        assert!(pdf.starts_with(b"%PDF"), "output should be a PDF");
        assert!(pdf.len() > 1500, "page should have content");
    }

    /// The sheet prints "PRINTED WITH YOUR PLURYPRINT DEFAULTS" above the
    /// settings box, so every row under it has to be true of *this* print. The
    /// Windows path does not send a tray at all, and the page used to claim
    /// "Tray: Tray 2" anyway — a lie on paper, which nothing can retract.
    #[test]
    fn fr_tray_row_only_claims_a_tray_the_platform_really_applies() {
        assert_eq!(TRAY_IS_APPLIED, cfg!(unix));

        let claimed = tray_row_value(Some("  Tray 2 "), "printer default");
        if TRAY_IS_APPLIED {
            assert_eq!(claimed, "Tray 2", "CUPS sends InputSlot, so it may say so");
        } else {
            assert_eq!(
                claimed, "printer default",
                "no bin was sent, so the sheet must not name one"
            );
        }

        // No override configured, or a blank one: "printer default" everywhere.
        assert_eq!(tray_row_value(None, "printer default"), "printer default");
        assert_eq!(
            tray_row_value(Some("   "), "printer default"),
            "printer default"
        );
    }

    /// WinAnsi is what the builtin fonts can print: Spanish and the
    /// typographic quotes stay, anything else becomes `?`.
    #[test]
    fn win_ansi_keeps_spanish_and_replaces_the_rest() {
        assert_eq!(
            win_ansi("Página «prueba» ¿sí? — €"),
            "Página «prueba» ¿sí? — €"
        );
        assert_eq!(win_ansi("打印机 ☃"), "??? ?");
        assert_eq!(win_ansi("a\tb"), "a?b");
    }

    /// FR-090/FR-092: the sheet speaks the app's language, falls back to
    /// English, and every sentence exists in both catalogs.
    #[test]
    fn fr_092_test_page_is_printed_in_the_app_language() {
        let en = Strings::for_locale("en-US");
        let es = Strings::for_locale("es-ES");
        assert_eq!(en.headline, "Printer Test Page");
        assert_eq!(es.headline, "Página de prueba");
        assert_eq!(es.title, "Página de prueba de Pluryprint");
        assert_eq!(Strings::for_locale("fr-FR"), en, "no catalog: English");
        for s in [&en, &es] {
            for text in [
                &s.title,
                &s.tagline,
                &s.headline,
                &s.confirm,
                &s.props[0],
                &s.props[1],
                &s.props[2],
                &s.defaults,
                &s.printer,
                &s.paper,
                &s.tray,
                &s.printer_default,
                &s.color,
                &s.band,
                &s.band_note,
            ] {
                assert!(!text.is_empty());
                // Every sentence is printable as it is.
                assert_eq!(&win_ansi(text), text, "not WinAnsi: {text}");
            }
        }
        assert_ne!(en.confirm, es.confirm);
        // Spanish renders to a valid page, with its accents encoded.
        let pdf = render(&TestPageInfo {
            locale: "es-ES".into(),
            printer: "Recepción".into(),
            paper: Some("A4".into()),
            tray: None,
        });
        assert!(pdf.starts_with(b"%PDF"));
        // The accent is WinAnsi's single byte 0xE1, not UTF-8's C3 A1 (which
        // printed as "PÃ¡gina").
        let doc = lopdf::Document::load_mem(&pdf).unwrap();
        let page = *doc.get_pages().values().next().unwrap();
        let content =
            lopdf::content::Content::decode(&doc.get_page_content(page).unwrap()).unwrap();
        let shown: Vec<Vec<u8>> = content
            .operations
            .iter()
            .filter(|op| op.operator == "Tj")
            .filter_map(|op| op.operands.first()?.as_str().ok().map(<[u8]>::to_vec))
            .collect();
        assert!(
            shown.iter().any(|b| b.as_slice() == b"P\xE1gina de prueba"),
            "{shown:?}"
        );
        assert!(shown.iter().any(|b| b.as_slice() == b"Recepci\xF3n"));
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
        assert!(footer.starts_with("Pluryprint   |   "), "{footer}");
        assert!(footer.ends_with("   |   AGPL-3.0"), "{footer}");
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
                locale: "en".into(),
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
