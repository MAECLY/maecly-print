# Print Engine

> **TL;DR** — MaeclyPrint uses a **hybrid pipeline**: each input file is converted to PDF by the most appropriate converter for its format (LibreOffice for Office docs, ImageMagick for images, Pandoc for Markdown, Chromium for HTML), then printed by a single back-end (SumatraPDF on Windows, `lpr`/CUPS on macOS). This "convert-everything-to-PDF, then print PDF" strategy is dramatically simpler than per-format printing while still producing native-quality output.

## 1. Why a hybrid engine

Two extreme designs are tempting and both are wrong:

| Design                             | Why it fails                                                                                                  |
| ---------------------------------- | -------------------------------------------------------------------------------------------------------------- |
| **Pure native per format**         | Every format requires a different printing API (Office automation, GDI+ for images, browser print for HTML). Maintenance balloons; bugs multiply per backend. |
| **Pure single-engine (Ghostscript only)** | Ghostscript can rasterize many formats but quality on Office docs is poor and licensing is GPL with edge cases. Doesn't handle DOCX or images natively without conversion anyway. |

The hybrid choice — **convert each format to PDF with the best-of-breed tool, then print PDF with one well-understood engine** — wins because:

- **Quality.** LibreOffice's DOCX → PDF is the most accurate free converter. ImageMagick handles images correctly with EXIF rotation, color profiles, etc. Chromium prints HTML the way the user sees it. Pandoc gives nicely-typeset Markdown.
- **Simplicity.** The final printing path has one shape: `print_pdf(file, settings)`. All printer settings (duplex, color, scale, range, copies, paper size) are set once at the printer-adapter layer.
- **Testability.** The "did it print correctly?" question reduces to "did we generate a correct PDF?" — and PDFs are inspectable.
- **Replaceability.** Swapping LibreOffice for a different DOCX converter does not affect the printer code path at all.

## 2. Format → converter → final-printer matrix

| Format           | Converter           | Tool invoked                   | Final printer (Win)  | Final printer (Mac)  |
| ---------------- | ------------------- | ------------------------------ | -------------------- | -------------------- |
| `.pdf`           | (passthrough)       | —                              | SumatraPDF           | `lpr`                |
| `.docx` `.doc`   | LibreOffice         | `soffice --headless --convert-to pdf` | SumatraPDF    | `lpr`                |
| `.xlsx` `.xls`   | LibreOffice         | `soffice --headless --convert-to pdf` | SumatraPDF    | `lpr`                |
| `.pptx` `.ppt`   | LibreOffice         | `soffice --headless --convert-to pdf` | SumatraPDF    | `lpr`                |
| `.odt` `.ods` `.odp` | LibreOffice     | `soffice --headless --convert-to pdf` | SumatraPDF    | `lpr`                |
| `.rtf`           | LibreOffice         | `soffice --headless --convert-to pdf` | SumatraPDF    | `lpr`                |
| `.txt`           | LibreOffice (or Pandoc) | `soffice --headless --convert-to pdf` | SumatraPDF | `lpr`                |
| `.png` `.jpg` `.jpeg` `.tiff` `.bmp` `.gif` `.webp` `.heic` | ImageMagick (or GDI+ direct) | `magick image.jpg image.pdf` | SumatraPDF | `lpr` |
| `.html` `.htm`   | Chromium headless   | `chrome --headless --print-to-pdf=...` | SumatraPDF   | `lpr`                |
| `.md` `.markdown` | Pandoc             | `pandoc file.md -o file.pdf`    | SumatraPDF          | `lpr`                |
| `.epub`          | Pandoc              | `pandoc -t html in.epub | chrome --headless --print-to-pdf=...` | SumatraPDF | `lpr` |
| `.svg`           | ImageMagick or Chromium | `magick file.svg out.pdf`   | SumatraPDF           | `lpr`                |

Anything not in this table is treated as **unsupported** and shown grouped at the bottom of the print dialog with a "Skip" badge. Users can still drag in files of unknown type and we do magic-byte detection (via `infer`) to recover.

## 3. Concrete invocations

### 3.1 SumatraPDF (Win)

```
SumatraPDF.exe ^
  -print-to "HP LaserJet Pro M404dn" ^
  -print-settings "duplex,color,fit,1-5,2x" ^
  -silent ^
  "C:\Users\...\AppData\Local\MaeclyPrint\cache\<job-id>.pdf"
```

`-print-settings` accepts a comma-separated list of:

- `duplexlong` / `duplexshort` / `simplex`
- `color` / `monochrome`
- `fit` / `shrink` / `noscale`
- `<from>-<to>` page range (e.g. `1-5`, `2,4,6-9`)
- `<n>x` copies (e.g. `3x`)
- `paper=A4` / `paper=letter` / `paper=legal`
- `bin=<tray-name>`
- `landscape` / `portrait`

We translate from our internal `PrintSettings` struct to a `-print-settings` string in `engine/printer/windows.rs`.

### 3.2 LibreOffice (cross-platform)

```
soffice ^
  --headless ^
  --convert-to pdf ^
  --outdir "C:\Users\...\AppData\Local\MaeclyPrint\cache" ^
  "C:\Users\...\Documents\report.docx"
```

Caveats:

- `soffice` does not allow concurrent invocations against the same user profile. We pass `-env:UserInstallation=file:///...` to give each conversion its own profile dir, so we can run them in parallel.
- Conversion can take 2–10 s for non-trivial documents; we set a default timeout of **60 s per file**, configurable.
- Macros and external links are disabled (`--norestore --nologo --nofirststartwizard`). Password-protected files surface a non-zero exit and we report "PasswordProtected" in the queue.

### 3.3 ImageMagick

```
magick "image.jpg" -auto-orient -density 300 -quality 95 "image.pdf"
```

`-auto-orient` honors EXIF rotation. `-density 300` controls the rasterization DPI for vector inputs (SVG). For multi-page TIFFs the input filename is `multi.tiff[0-9]` to limit ranges if needed; otherwise all pages map to PDF pages.

### 3.4 Chromium headless

```
chrome ^
  --headless=new ^
  --disable-gpu ^
  --no-sandbox ^
  --print-to-pdf="output.pdf" ^
  --print-to-pdf-no-header ^
  "file:///C:/Users/.../page.html"
```

We locate Chromium in this order:

1. Bundled `MaeclyPrint\redist\chromium\chrome.exe` (when the user opted into the full bundle, M2+).
2. Edge: `%ProgramFiles(x86)%\Microsoft\Edge\Application\msedge.exe`.
3. Chrome: `%ProgramFiles%\Google\Chrome\Application\chrome.exe`.
4. PATH lookup (`which`).

If none is present and HTML files are queued, the user sees a clear message in the dialog and a one-click "Install Microsoft Edge (recommended)" link.

### 3.5 Pandoc

```
pandoc "notes.md" --pdf-engine=wkhtmltopdf -o "notes.pdf"
```

If `wkhtmltopdf` is unavailable we fall back to converting Markdown → HTML via pandoc, then HTML → PDF via the chosen Chromium binary. This avoids requiring a TeX install.

### 3.6 `lpr` (Mac)

```
lpr -P "OfficeJet_Pro" \
    -o sides=two-sided-long-edge \
    -o ColorModel=Gray \
    -o media=A4 \
    -o page-ranges=1-5 \
    -# 2 \
    "/Users/.../cache/<job-id>.pdf"
```

CUPS is well-documented and stable. Settings translate cleanly from `PrintSettings`.

## 4. The full pipeline

```
+--------------------+
| User submits N     |
| files + Profile    |
+----------+---------+
           |
           v
+--------------------+
| 1. Format detect   |  mime_guess by extension; infer fallback by magic bytes.
|    per file        |  Output: Vec<JobPlan>
+----------+---------+
           |
           v
+--------------------+
| 2. Router selects  |  Each job gets a Converter trait object.
|    Converter       |  Passthrough for PDF, etc.
+----------+---------+
           |
           v
+--------------------+
| 3. Plan validation |  Sanity-check: file exists, readable, < max size,
|    + dedupe        |  not encrypted (when detectable).
+----------+---------+
           |
           v
+--------------------+
| 4. Conversion pool |  Up to N parallel converters via rayon.
|    (parallel)      |  Each writes to %LOCALAPPDATA%\MaeclyPrint\cache\<job>.pdf
+----------+---------+
           |
           v
+--------------------+
| 5. Per-job result  |  Ok(pdf_path) or Err(ConvertError). Errors do not
|    captured        |  abort the queue; they are logged and reported.
+----------+---------+
           |
           v
+--------------------+
| 6. Printer dispatch|  Sequential by default. Adapter calls SumatraPDF/lpr
|    (sequential)    |  with the PrintSettings derived from Profile + overrides.
+----------+---------+
           |
           v
+--------------------+
| 7. Status polling  |  Spooler queried for job state; surfaced in UI.
|    + log emit      |  JSONL entry per state transition.
+--------------------+
```

### 4.1 Router selection algorithm

```rust
pub fn select_converter(mime: &str, ext: &str) -> Box<dyn Converter> {
    match (mime, ext) {
        ("application/pdf", _) => Box::new(Passthrough),
        (m, _) if m.starts_with("image/") => Box::new(ImageMagickConv::default()),
        ("application/vnd.openxmlformats-officedocument.wordprocessingml.document", _)
        | ("application/msword", _)
        | ("application/vnd.ms-excel", _)
        | ("application/vnd.openxmlformats-officedocument.spreadsheetml.sheet", _)
        | ("application/vnd.ms-powerpoint", _)
        | ("application/vnd.openxmlformats-officedocument.presentationml.presentation", _)
        | ("application/vnd.oasis.opendocument.text", _)
        | ("application/vnd.oasis.opendocument.spreadsheet", _)
        | ("application/vnd.oasis.opendocument.presentation", _)
        | ("application/rtf", _) => Box::new(LibreOfficeConv::new()),
        ("text/html", _) => Box::new(ChromiumConv::detect()),
        ("text/markdown", _) | (_, "md") | (_, "markdown") => Box::new(PandocConv::default()),
        ("application/epub+zip", _) => Box::new(EpubChainConv::new()),  // pandoc -> chromium
        _ => Box::new(Unsupported),
    }
}
```

Selection happens during plan-build; the trait object is owned by the `JobPlan`.

### 4.2 PDF caching

Converted PDFs land in `%LOCALAPPDATA%\MaeclyPrint\cache\<job-id>.pdf`. On macOS: `~/Library/Caches/MaeclyPrint/<job-id>.pdf`. The cache:

- Is **not** keyed on file content; each job gets its own cache file. This avoids spurious "is the file modified?" decisions.
- Is GC'd after the parent job completes (success or terminal failure) by a background task with a 1-minute grace period.
- Has a configurable hard cap of 1 GB; when exceeded the oldest unreferenced PDFs are evicted.

## 5. Error handling matrix

| Error                              | Detection                          | User-visible message                                  | Queue effect              |
| ---------------------------------- | ---------------------------------- | ----------------------------------------------------- | ------------------------- |
| Converter binary not found         | `which()` at startup; cached       | "LibreOffice is required for .docx files. Install it." | Job skipped, others continue |
| Converter timeout (default 60 s)   | `tokio::time::timeout` around child | "Conversion took too long. Try a different printer or split the document." | Job marked failed, retryable |
| Converter exit code ≠ 0            | child wait status                   | Show stderr tail in expandable details                  | Job failed                |
| Encrypted PDF                      | qpdf check before printing         | "This PDF is password-protected. Provide the password." | Job paused awaiting input |
| Password-protected Office doc      | `soffice` exit code 5 + stderr     | "This document is password-protected. Provide the password." | Job paused awaiting input |
| File path no longer exists         | stat at plan-build                 | "File not found: <path>"                              | Job skipped               |
| File is a directory                | stat at plan-build                 | "Skipped folder: <path>"                              | Job skipped               |
| Unsupported format                 | router returns `Unsupported`       | "MaeclyPrint cannot print .xyz files."                | Job skipped               |
| File > max size (default 200 MB)   | stat at plan-build                 | "File is larger than the limit. Adjust in Settings."  | Job skipped               |
| Disk full during conversion        | I/O error from converter           | "Not enough disk space to print."                     | Job failed; queue paused  |
| Printer offline                    | spooler reports `PRINTER_STATUS_OFFLINE` | "Printer is offline. Check the connection."   | Job paused, retry on online |
| Printer out of paper               | spooler `PRINTER_STATUS_PAPER_OUT` | "Out of paper."                                       | Job paused                |
| Printer driver missing             | `EnumPrinters` returns nothing for name | "Printer not installed."                          | Job skipped               |
| `SumatraPDF.exe` missing           | `which()` cached                   | "MaeclyPrint requires SumatraPDF. Reinstall."         | All print jobs fail       |
| User cancellation                  | UI cancel button → IPC             | —                                                     | Active job killed; pending discarded |

### 5.1 Retry policy

- **Transient printer errors** (offline, paper out, jammed): mark as `Paused`, poll spooler every 5 s for up to 10 minutes, then fail.
- **Converter timeouts**: retry once with a longer timeout (×2) only if `auto_retry_timeouts = true` in profile.
- **Network errors during update download**: handled in the updater module, not here.

## 6. Performance characteristics

Measured on a Windows 11 machine, Ryzen 7 5800X, NVMe SSD, files in `%TEMP%`:

| Scenario                               | Per-file time | Throughput     |
| -------------------------------------- | ------------- | -------------- |
| 50× small PDFs (passthrough)           | ~50 ms        | ~20 PDFs/sec spooled |
| 50× 5-page DOCX (LibreOffice, parallel ×4) | ~1.8 s avg | ~2.2 docs/sec to PDF |
| 100× JPG photos (ImageMagick parallel ×8) | ~120 ms avg | ~8 imgs/sec to PDF |
| 20× HTML pages (Chromium parallel ×4)  | ~900 ms avg   | ~4 pages/sec to PDF |

These are *converter* throughputs. The printer itself is the long pole for actual ink-on-paper.

## 7. Configuration knobs

These are all in `~/.config/MaeclyPrint/config.toml`:

```toml
[engine]
max_concurrent_conversions = 4
conversion_timeout_seconds = 60
max_file_size_bytes = 209_715_200      # 200 MiB
cache_dir = "auto"                      # or absolute path
cache_max_bytes = 1_073_741_824         # 1 GiB

[engine.converters]
libreoffice_path = "auto"               # auto-detect or absolute path
imagemagick_path = "auto"
pandoc_path = "auto"
chromium_path = "auto"

[engine.printer.windows]
sumatra_path = "auto"                   # bundled in install dir by default

[engine.printer.macos]
lpr_path = "/usr/bin/lpr"
```

## 8. Extensibility

Adding a new format is three changes:

1. Implement `Converter` in `engine/converters/`.
2. Add the MIME / extension to `select_converter`.
3. Add a row to the format table in this document.

No printer-side change is needed because the new converter feeds the same PDF pipeline.

## 9. Related documents

- [`system-architecture.md`](./system-architecture.md) — the engine in context.
- [`data-model.md`](./data-model.md) — `PrintSettings` schema referenced above.
- [`ipc-and-cli.md`](./ipc-and-cli.md) — exposing engine controls to the UI and CLI.
- ADR [`0003-hybrid-print-engine-strategy`](../adr/0003-hybrid-print-engine-strategy.md) — why this design.
