# Supported Formats

> **TL;DR** — Per-format support matrix: which file types MaeclyPrint can print, the converter and final-print engine used, target release version, and known limitations.

The list of formats is intentionally **bounded**. Print Conductor advertises 90+ formats but supports many of them poorly. We aim to support **fewer formats, very well**, with predictable behavior and clear failure modes.

## Master matrix

| Extension(s) | MIME | Converter | Final printer (Win) | Final printer (macOS) | Target | Fidelity |
|---|---|---|---|---|---|---|
| `.pdf` | `application/pdf` | none | SumatraPDF | `lpr` | **MVP** | Excellent (native) |
| `.jpg`, `.jpeg` | `image/jpeg` | ImageMagick → PDF *(or GDI+ direct)* | SumatraPDF | `lpr` | **MVP** | Excellent |
| `.png` | `image/png` | ImageMagick → PDF | SumatraPDF | `lpr` | **MVP** | Excellent |
| `.tiff`, `.tif` | `image/tiff` | ImageMagick → PDF | SumatraPDF | `lpr` | MVP | Excellent |
| `.bmp` | `image/bmp` | ImageMagick → PDF | SumatraPDF | `lpr` | MVP | Excellent |
| `.webp` | `image/webp` | ImageMagick → PDF | SumatraPDF | `lpr` | MVP | Good |
| `.heic`, `.heif` | `image/heic` | ImageMagick (with HEIC delegate) → PDF | SumatraPDF | `lpr` | MVP | Good |
| `.docx` | `application/vnd.openxmlformats-officedocument.wordprocessingml.document` | LibreOffice headless → PDF | SumatraPDF | `lpr` | **v0.2** | High (90%+) |
| `.xlsx` | `application/vnd.openxmlformats-officedocument.spreadsheetml.sheet` | LibreOffice headless → PDF | SumatraPDF | `lpr` | **v0.2** | High |
| `.pptx` | `application/vnd.openxmlformats-officedocument.presentationml.presentation` | LibreOffice headless → PDF | SumatraPDF | `lpr` | **v0.2** | High |
| `.doc` | `application/msword` | LibreOffice headless → PDF | SumatraPDF | `lpr` | v0.2 | Medium-high |
| `.xls` | `application/vnd.ms-excel` | LibreOffice headless → PDF | SumatraPDF | `lpr` | v0.2 | Medium-high |
| `.ppt` | `application/vnd.ms-powerpoint` | LibreOffice headless → PDF | SumatraPDF | `lpr` | v0.2 | Medium-high |
| `.odt` | `application/vnd.oasis.opendocument.text` | LibreOffice headless → PDF | SumatraPDF | `lpr` | **v0.2** | Excellent (native) |
| `.ods` | `application/vnd.oasis.opendocument.spreadsheet` | LibreOffice headless → PDF | SumatraPDF | `lpr` | v0.2 | Excellent |
| `.odp` | `application/vnd.oasis.opendocument.presentation` | LibreOffice headless → PDF | SumatraPDF | `lpr` | v0.2 | Excellent |
| `.rtf` | `application/rtf` | LibreOffice headless → PDF | SumatraPDF | `lpr` | v0.2 | High |
| `.txt` | `text/plain` | LibreOffice headless → PDF | SumatraPDF | `lpr` | **v0.2** | High |
| `.html`, `.htm` | `text/html` | Chromium headless `--print-to-pdf` | SumatraPDF | `lpr` | **v0.2** | High |
| `.md`, `.markdown` | `text/markdown` | Pandoc → PDF (via wkhtmltopdf or LaTeX) | SumatraPDF | `lpr` | **v0.2** | High |

**Legend.** *MVP* = ships in v0.1.0. *v0.2* = ships in v0.2.0 (M3 milestone).

## Per-format details

### PDF

The simplest case: no conversion, direct invocation of the final printing engine.

```powershell
# Windows
SumatraPDF.exe -print-to "PrinterName" -print-settings "duplex,color,fit,1-5" -silent file.pdf
```

```bash
# macOS (M5+)
lpr -P PrinterName -o sides=two-sided-long-edge -o media=A4 file.pdf
```

**Edge cases handled:**
- Encrypted PDFs: prompt for password (CLI: `--password <pwd>`).
- Permission-restricted PDFs (no printing flag): explicit error, suggest `qpdf --decrypt` workaround in error message.
- Corrupt PDFs: SumatraPDF exit code captured, job marked failed with diagnostic.
- Very large PDFs (>500 MB): chunked spooling not needed; relies on OS spooler.

### Images (JPG, PNG, TIFF, BMP, WebP, HEIC)

Two-stage approach:

1. ImageMagick converts the image to a PDF page sized to fit the configured paper:
   ```bash
   magick convert input.jpg -resize 2480x3508 -gravity center -extent 2480x3508 -density 300 -compress jpeg page.pdf
   ```
2. SumatraPDF prints the resulting PDF.

**Why not GDI+ direct?** GDI+ is faster on Windows but produces inconsistent results across printer drivers. The PDF intermediate normalizes behavior. We document GDI+ as an opt-in fast path (`--engine=gdi`) for advanced users.

**HEIC** requires ImageMagick's HEIC delegate (`libheif`); the installer detects and warns if missing.

### Office (DOCX, XLSX, PPTX) and ODF (ODT, ODS, ODP)

LibreOffice headless converts to PDF, SumatraPDF prints:

```bash
soffice --headless --convert-to pdf --outdir %TEMP%\maeclyprint input.docx
SumatraPDF.exe -print-to "PrinterName" %TEMP%\maeclyprint\input.pdf
```

**Fidelity caveats:**
- Complex Word styles (custom themes, Office 365–specific features) may render differently than in MS Word.
- Excel sheets with many tabs: by default each tab becomes a PDF page; the user MAY restrict to active sheet via per-format options.
- PowerPoint: animations are not preserved (irrelevant for print). Speaker notes can be included via `--print-notes` flag.
- Password-protected Office files: prompt for password.
- Files with macros: macros are NOT executed (LibreOffice headless ignores them by design — this is a security feature, not a bug).

**Performance.** First-run LibreOffice startup is ~3-5 seconds. We mitigate by keeping a `soffice` process warm via `--accept` UNO socket if user enables "Office speedup" in Settings.

### TXT and RTF

LibreOffice handles both reliably. For very simple TXT we COULD use a faster `pandoc -f markdown_strict -o pdf` path, but the consistency benefit of one engine wins for v0.2.

### HTML

Chromium headless renders the HTML to a PDF identical (within Chromium fidelity) to "Print to PDF" from a browser:

```bash
chrome --headless --disable-gpu --no-pdf-header-footer --print-to-pdf=output.pdf "file:///path/to/file.html"
```

**Edge cases:**
- HTML with relative resource URLs (CSS, images): they are resolved relative to the file path.
- HTML with JavaScript: scripts run in headless mode (default). Use `--no-javascript` for security-sensitive contexts.
- Remote HTML (`http://`/`https://`): NOT supported in MVP. The user must download first. (This is by design to avoid surprise network egress.)
- Pages with `@media print` styles: respected.

### Markdown

Two-stage: Pandoc converts MD → PDF using either wkhtmltopdf (lighter) or LaTeX (richer). Default is wkhtmltopdf-style for portability:

```bash
pandoc input.md -o input.pdf --pdf-engine=wkhtmltopdf
```

**Caveats:**
- Pandoc is required as an external dependency.
- LaTeX-style math (`$$ ... $$`) requires LaTeX installed; otherwise renders as raw text with a warning.
- Embedded images with relative paths are resolved relative to the .md file.

## Out of scope (deferred to v2+ or never)

| Format | Why deferred |
|---|---|
| AutoCAD DWG/DXF | Requires LibreCAD or proprietary ODA SDK; small audience for batch printing |
| Visio (.vsdx) | LibreOffice support is fragile; rarely batch-printed |
| EPUB | Niche use case; better with dedicated reader |
| Email (.eml, .msg, .pst) | Email rendering is non-trivial; users typically use Outlook |
| PostScript (.ps) | Edge case; Ghostscript dependency would inflate installer |
| Zebra ZPL barcodes | Highly specialized; would be a v2 plugin if demand emerges |
| Plaintext code with syntax highlighting (Pretty Print) | Could be a v2 feature; PrintFile covered this niche |

## Adding a new format

The architecture isolates each converter to a module under `src-tauri/src/engine/`. Adding a format is:

1. Add a converter module (e.g., `src-tauri/src/engine/epub.rs`).
2. Register it in the format router (`router.rs`).
3. Add tests with a fixture file under `tests/fixtures/`.
4. Add a row to this matrix.
5. Document any new dependency in [installer-windows.md](../07-build-and-release/installer-windows.md).

A future plugin API (v2) MAY allow third-party converters as separate crates loaded at runtime.

## Dependency detection

At first launch the application probes for optional binaries:

| Dependency | Probe command | Fallback if missing |
|---|---|---|
| LibreOffice | `soffice --version` | Office formats disabled, banner in UI |
| Chromium / Edge | `chrome --version` / `msedge --version` | HTML format disabled |
| Pandoc | `pandoc --version` | Markdown format disabled |
| ImageMagick | `magick --version` | Images fall back to GDI+ direct (Windows only) |
| qpdf | `qpdf --version` | PDF preprocessing features disabled |

The probe is cached for 24 hours and re-run on Settings → Refresh Dependencies.
