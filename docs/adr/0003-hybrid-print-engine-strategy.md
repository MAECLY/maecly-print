# ADR-0003: Hybrid print-engine strategy

## Status

Accepted

## Date

2026-05-03

## Context

MaeclyPrint must print files of many formats: PDFs, Office documents (DOCX, XLSX, PPTX, ODT, ODS, ODP, RTF), images (PNG, JPG, TIFF, BMP, WebP, HEIC, SVG), HTML pages, Markdown, and EPUB. Each format has its own native printing path:

- PDFs can be sent to a printer directly via SumatraPDF (Win) or `lpr` (Mac/CUPS).
- Office files have native print APIs only when the original application is installed (Word, LibreOffice).
- Images can be drawn via GDI+ on Windows or Quartz on Mac — direct, but each platform is different.
- HTML can be printed by a headless browser.
- Markdown has no native printer; it has to be rendered first.

Three architectures are possible:

1. **Multi-engine, route-by-format**: each format type is printed by its own native API/tool. Many code paths, many platform-specific bugs.
2. **Single-engine, universal rasterizer**: convert everything to a single intermediate (e.g., PostScript or PDF) using a single tool (e.g., Ghostscript). Simple, but quality on complex Office docs is poor.
3. **Hybrid: best-of-breed converter per format → PDF → single PDF printer**: each format uses the best tool for *that conversion* (LibreOffice for Office, ImageMagick for images, Chromium for HTML, Pandoc for Markdown), then the resulting PDF is printed by a single, well-understood backend (SumatraPDF on Win, `lpr` on Mac).

We need to choose one.

## Decision

We will adopt the **hybrid strategy (option 3)**. Specifically:

- Every input file is **converted to PDF** by the best-of-breed converter for its format.
- The resulting PDF is then **printed by a single backend per OS**: SumatraPDF on Windows, `lpr` on macOS.
- Already-PDF inputs skip conversion (passthrough).
- Conversion is parallelizable; printing is sequential by default.

The format-to-converter map (full table in [`../04-architecture/print-engine.md`](../04-architecture/print-engine.md)):

| Format             | Converter            | Final printer (Win) | Final printer (Mac) |
| ------------------ | -------------------- | ------------------- | ------------------- |
| `.pdf`             | (passthrough)        | SumatraPDF          | `lpr`               |
| `.docx` / `.xlsx` / `.pptx` / `.odt` / `.ods` / `.odp` / `.rtf` | LibreOffice headless | SumatraPDF | `lpr` |
| Images             | ImageMagick          | SumatraPDF          | `lpr`               |
| `.html`            | Chromium headless    | SumatraPDF          | `lpr`               |
| `.md`              | Pandoc               | SumatraPDF          | `lpr`               |
| `.epub`            | Pandoc → Chromium    | SumatraPDF          | `lpr`               |

The Rust engine layer exposes a `Converter` trait (any format) and a `Printer` trait (per OS). The router selects a converter; the printer is fixed per OS.

## Alternatives Considered

| Option                                 | Why rejected                                                                                              |
| -------------------------------------- | --------------------------------------------------------------------------------------------------------- |
| **Multi-engine, native-per-format**    | Massive code surface: separate code paths for each format on each OS. Bug count multiplies. Office automation requires Word/LibreOffice anyway, so we don't even save complexity. |
| **Pure Ghostscript pipeline**         | Ghostscript can rasterize PDFs/PostScript well, but it does not handle DOCX or modern Office files at all — we'd need LibreOffice anyway. So Ghostscript only replaces SumatraPDF/lpr at the final stage, where it adds nothing (and is slower). |
| **Bundle a custom PDF renderer (e.g., MuPDF, Poppler) instead of SumatraPDF** | MuPDF (AGPL) and Poppler (GPLv2/GPLv3) are powerful but require us to *build* a printing pipeline from scratch (page-iteration, GDI+ blitting, settings translation). SumatraPDF already does this, well, with a stable CLI. |
| **Use the Print Spooler's built-in conversion** (XPS-everywhere) | Windows can convert many formats via XPS Document Writer. Quality is uneven, settings translation is opaque, and it doesn't exist on macOS — defeating the cross-platform goal. |
| **Pure native printing (no PDF intermediate)** | Each printer settings translation has to be re-done per format and per OS. Too many bugs. |

## Consequences

### Positive

- **Single printing code path per OS.** The "set duplex, set color, set range, send to printer" logic exists once per OS (in the printer adapter) instead of once per format-platform pair.
- **Quality is high.** Each converter is the best free tool for its format. LibreOffice's DOCX→PDF is widely the best free converter; ImageMagick handles EXIF/color profiles correctly; Chromium prints HTML the way the user sees it.
- **Testability.** "Did the print succeed?" reduces to "did we generate a correct PDF?" PDFs are inspectable, diff-able, and stable as test artifacts.
- **Parallelism.** Conversions are independent and can run in a `rayon` pool. Printing remains sequential to keep status meaningful.
- **Replaceable converters.** Swap LibreOffice for OnlyOffice with one trait impl change. No changes to the printer adapter.
- **PDF cache is a useful side effect.** Users can re-print a job from cache without re-converting.

### Negative

- **External tool dependency.** We need LibreOffice, ImageMagick, Pandoc, and a Chromium-class browser available on the system. We mitigate by:
  - Auto-detecting installed copies first.
  - Bundling SumatraPDF (small, GPLv3) in our installer.
  - Offering an optional "Full bundle" download with all converters included.
  - Showing clear error messages and one-click install links when a converter is missing.
- **Conversion latency.** Each format adds 0.1-2 s before the first page hits the spooler. Acceptable for batch printing; users expect printing to take "a little while".
- **Disk usage.** Converted PDFs live in cache (default 1 GB cap, evicted oldest-first). Slightly more disk traffic than direct printing.
- **PDF as a lossy format for some content.** Form fields, multimedia, and 3D content in PDFs are flattened. Acceptable for printing (which is itself lossy).

### Risks / Open Questions

- **LibreOffice headless reliability.** Concurrent invocations against the same user profile collide; we work around it by passing `-env:UserInstallation=...` per job. We will keep an eye on edge cases (extremely large XLSX files, complex PPTX with embedded media).
- **Chromium availability.** Edge or Chrome is present on >95% of Windows installs, but not 100%. We surface a clean "install Edge" prompt when needed.
- **Color management.** Converters and printers may disagree about color profiles. M2 will add an explicit color-management mode (sRGB / printer-native) per profile.
- **Form-fillable PDFs.** Currently we flatten on print, which loses fillable form data. That matches every other "print PDF" tool but may surprise some users.
- **EPUB → Chromium chain feels fragile.** If demand for EPUB grows, we may add a dedicated EPUB → PDF converter (e.g., Calibre's `ebook-convert`).

## Related

- ADR-0001 (stack) — Rust core orchestrates the converters as subprocesses.
- ADR-0002 (license) — SumatraPDF's GPLv3 license is one reason we picked GPLv3.
- [`../04-architecture/print-engine.md`](../04-architecture/print-engine.md) — full engine reference.
