# Supported Formats

> **TL;DR** — Per-format support matrix. **Only PDF and five raster image formats print today**: `.pdf`, `.jpg`/`.jpeg`, `.png`, `.tif`/`.tiff`, `.bmp`, `.webp`. Everything else — Office, ODF, HTML, Markdown, plain text, and **HEIC/HEIF** — is rejected by the router with an explicit reason. This document lists what ships now and what is planned, and never conflates the two.

The list of formats is intentionally **bounded**. Print Conductor advertises 90+ formats but supports many of them poorly. We aim to support **fewer formats, very well**, with predictable behavior and clear failure modes.

## The source of truth

The authoritative list is a single constant in `src-tauri/src/engine/mod.rs`:

```rust
pub const SUPPORTED_EXTS: &[&str] =
    &["pdf", "jpg", "jpeg", "png", "tif", "tiff", "bmp", "webp"];
```

`engine::is_supported` is what folder expansion, the drop handler, the CLI and the Print Job screen all consult. If a row in the matrix below is not in that constant, MaeclyPrint does not print it — no matter what the row says. Adding a format means changing the constant *and* this table in the same commit.

## Ships today

| Extension(s) | MIME | Converter | Final printer (Win) | Final printer (macOS) | Fidelity |
|---|---|---|---|---|---|
| `.pdf` | `application/pdf` | none — passthrough after a `%PDF-` header check | SumatraPDF | `lp` | Excellent (native) |
| `.jpg`, `.jpeg` | `image/jpeg` | built-in `image` crate → one-page PDF | SumatraPDF | `lp` | Excellent |
| `.png` | `image/png` | built-in → PDF | SumatraPDF | `lp` | Excellent |
| `.tif`, `.tiff` | `image/tiff` | built-in → PDF | SumatraPDF | `lp` | Excellent |
| `.bmp` | `image/bmp` | built-in → PDF | SumatraPDF | `lp` | Excellent |
| `.webp` | `image/webp` | built-in → PDF | SumatraPDF | `lp` | Good |

Two corrections to earlier drafts of this table:

- **The image converter is not ImageMagick.** It is the pure-Rust `image` crate compiled with `default-features = false` and exactly five codecs (`jpeg`, `png`, `tiff`, `bmp`, `webp`), rasterising into a PDF built with `printpdf`. No external process, no delegate library, nothing to detect at install time. The set of image formats we support *is* that feature list.
- **macOS spools with `lp`, not `lpr`.** `engine::print::cups` builds an `lp -d <printer> -t <title> …` argument vector.

## Rejected today, planned later

| Extension(s) | Planned converter | Status | What the user sees now |
|---|---|---|---|
| `.heic`, `.heif` | native `libheif`, or shelling out to ImageMagick | **Blocked on a decision** | `"HEIC is not supported yet — convert it to JPEG or PNG first"` |
| `.docx`, `.doc` | LibreOffice headless → PDF | Not wired | Generic unsupported-format refusal |
| `.xlsx`, `.xls` | LibreOffice headless → PDF | Not wired | Generic refusal |
| `.pptx`, `.ppt` | LibreOffice headless → PDF | Not wired | Generic refusal |
| `.odt`, `.ods`, `.odp` | LibreOffice headless → PDF | Not wired | Generic refusal |
| `.rtf`, `.txt` | LibreOffice headless → PDF | Not wired | Generic refusal |
| `.html`, `.htm` | Chromium headless `--print-to-pdf` | Not wired | Generic refusal |
| `.md`, `.markdown` | Pandoc → PDF | Not wired | Generic refusal |

"Not wired" is precise: `engine::deps` can already *locate* LibreOffice, a Chromium-class browser and Pandoc, and reports where it found them — but no converter calls them, so installing one changes nothing. `Spec::blocks` deliberately lists no extensions for those three, and their remediation text says so.

### Why HEIC is called out separately

HEIC was listed as MVP in every earlier version of this document. It never worked, and the engine now refuses it with a **specific** message rather than the generic one, because "unsupported format" reads to a user as "MaeclyPrint doesn't know what a `.heic` is" — which is not the problem.

The problem is that the `image` crate ships **no HEIF decoder at any feature combination**. Supporting HEIC therefore means either a native dependency (`libheif`, with its own build and licensing consequences on both platforms) or shelling out to ImageMagick — reintroducing exactly the external-process dependency the pure-Rust codec choice in ADR-0003 was meant to avoid. That is an architecture decision, not a bug fix, and it has not been taken.

The refusal is one constant, `engine::convert::HEIF_UNSUPPORTED_REASON`, shared by the converter and by the file-inspection surface, so the engine and the drop toast tell the same story. It is pinned by `fr_052_heic_is_refused_with_an_explicit_reason` and `fr_052_inspect_names_heic_instead_of_hiding_it`.

## How a rejected file is reported

Nothing is dropped silently (FR-005/FR-059). Folder expansion returns every discarded entry as a `SkippedFile` carrying a `SkipReason` — `UnsupportedFormat`, `TooLarge` or `Unreadable` — plus a human-readable `detail` (`"Unsupported file type (.docx)"`) ready for the job log and the UI.

## Per-format details

> Sections for formats in the *Rejected today* table describe the **planned** pipeline. Only PDF and Images below describe running code.

### PDF

The simplest case: no conversion, direct invocation of the final printing engine. `convert_to_pdf` validates the `%PDF-` header (cheap: five bytes) and returns `Ok(None)`, meaning "print the original in place". A mislabelled or truncated file is rejected before it reaches the spooler.

```powershell
# Windows — the real argument vector (engine::print::sumatra)
SumatraPDF.exe -print-to "PrinterName" -print-settings "1-5,duplex,color,fit,paper=A4" -silent -exit-when-done file.pdf
```

```bash
# macOS — the real argument vector (engine::print::cups). `lp`, not `lpr`.
lp -d PrinterName -t "job title" -o sides=two-sided-long-edge -o media=A4 -o fit-to-page file.pdf
```

**Edge cases:**
- Corrupt or mislabelled PDFs: rejected by the header check with `ConversionFailed("not a valid PDF")` before spooling. **Implemented.**
- Very large PDFs: no chunked spooling; we rely on the OS spooler. **Implemented** (by doing nothing).
- Encrypted PDFs: *planned* — prompt for a password (CLI: `--password <pwd>`). There is no `--password` flag today; an encrypted PDF fails at the spooler.
- Permission-restricted PDFs (no printing flag): *planned* — explicit error suggesting `qpdf --decrypt`.

### Images (JPG, PNG, TIFF, BMP, WebP)

Two stages, both in-process:

1. `engine::convert::image::to_pdf` decodes with the `image` crate and rasterises the result into a one-page PDF written to `cache_dir/<job_id>.pdf`. Progress is reported as `0.0..=1.0`.
2. SumatraPDF (Windows) or `lp` (macOS) prints that PDF.

Cancellation is checked at every stage boundary. Decoding a large image is a single blocking call that cannot itself be interrupted, so a cancel lands either before or after decode, never inside it.

**Why an in-process converter rather than ImageMagick?** No external dependency to detect, install, warn about or version-pin; no process spawn per file; and the supported-format list becomes a compile-time fact (the crate's feature list) instead of a runtime probe that can disagree with the documentation. The cost is HEIC, and the odd exotic sub-format — a CMYK JPEG, for instance, comes back as `ConversionFailed` rather than printing wrong.

**Why not GDI+ direct on Windows?** GDI+ is faster but produces inconsistent results across printer drivers. The PDF intermediate normalises behaviour. There is no `--engine=gdi` flag and none is planned.

### Office (DOCX, XLSX, PPTX) and ODF (ODT, ODS, ODP) — planned

> Not implemented. `engine::deps` can find LibreOffice; nothing calls it.

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

### TXT and RTF — planned

> Not implemented; `.txt` and `.rtf` are refused today.

LibreOffice handles both reliably. For very simple TXT we COULD use a faster `pandoc -f markdown_strict -o pdf` path, but the consistency benefit of one engine wins for v0.2.

### HTML — planned

> Not implemented. `engine::deps` can find a Chromium-class browser; nothing calls it.

Chromium headless renders the HTML to a PDF identical (within Chromium fidelity) to "Print to PDF" from a browser:

```bash
chrome --headless --disable-gpu --no-pdf-header-footer --print-to-pdf=output.pdf "file:///path/to/file.html"
```

**Edge cases:**
- HTML with relative resource URLs (CSS, images): they are resolved relative to the file path.
- HTML with JavaScript: scripts run in headless mode (default). Use `--no-javascript` for security-sensitive contexts.
- Remote HTML (`http://`/`https://`): NOT supported in MVP. The user must download first. (This is by design to avoid surprise network egress.)
- Pages with `@media print` styles: respected.

### Markdown — planned

> Not implemented. `engine::deps` can find Pandoc; nothing calls it.

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

Each converter is a module under `src-tauri/src/engine/convert/`. Adding a format is:

1. Add a converter module (e.g. `src-tauri/src/engine/convert/office.rs`).
2. Route the extension to it in `convert/mod.rs::convert_to_pdf`.
3. **Add the extension to `engine::SUPPORTED_EXTS`.** Skipping this is the classic mistake: the converter works, but `is_supported` still refuses the file at drop time, so nothing ever reaches it.
4. If the converter shells out, add a `Dependency` variant in `engine/deps.rs` and list the extensions it gates in that spec's `blocks`, so a missing program produces a remediation message instead of a spawn failure mid-job.
5. Add tests. Note there is no fixtures tree yet (see [testing-strategy.md](../08-development/testing-strategy.md)); a converter that needs binary inputs brings the fixture convention with it.
6. Update this document — both tables, and move the format's detail section out of *planned*.
7. Document any new install-time dependency in [installer-windows.md](../07-build-and-release/installer-windows.md).

A future plugin API (v2) MAY allow third-party converters as separate crates loaded at runtime.

## Dependency detection

`engine::deps` resolves external programs. Four are known:

| Dependency | Executable names | Blocks (extensions) |
|---|---|---|
| SumatraPDF | `SumatraPDF.exe` | The Windows print hop for **every** supported format. Never reported as missing off Windows — macOS and Linux print through CUPS `lp`, and "SumatraPDF is missing" on a Mac would be the loudest possible false alarm. |
| LibreOffice | `soffice` | **nothing today** — no converter uses it |
| Chromium | `msedge.exe` / `chrome.exe` (Windows) | **nothing today** |
| Pandoc | `pandoc` | **nothing today** |

Neither ImageMagick nor qpdf is a dependency: image conversion is in-process, and no PDF preprocessing step exists.

The search order is the same for all four, and `resolve` is a pure function that takes the candidate directories and an `exists` predicate, so a Windows search order is unit-testable from a macOS machine:

1. **Bundled** — next to our own executable, in the layout `bundle.resources` produces. A copy we ship always wins: it is the version we tested against.
2. **Configured** — an explicit user-supplied path. No such setting exists in `config.toml` yet, so callers pass `None`; the slot exists because precedence has to live in the resolver, and adding the setting later must not reshuffle the other three steps.
3. **`PATH`.**
4. **Well-known install directories** for the OS.

`FoundIn` records which step produced the hit, so "it picked up the wrong copy" is answerable from diagnostics without a debugger.

**A report never claims a format works.** Because LibreOffice, Chromium and Pandoc block nothing, their remediation text says installing them changes nothing yet — rather than implying an Office document would print.
