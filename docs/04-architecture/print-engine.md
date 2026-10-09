# Print Engine

> **TL;DR** — MaeclyPrint uses a **hybrid pipeline**: each input file is converted to PDF by the most appropriate converter for its format, then printed by a single back-end (SumatraPDF on Windows, CUPS `lp` on macOS/Linux). This "convert-everything-to-PDF, then print PDF" strategy is dramatically simpler than per-format printing while still producing native-quality output. **Images are converted in-process by the Rust `image` crate, not by ImageMagick.** HEIC goes through the OS decoder. Office, OpenDocument, TXT and RTF go through LibreOffice headless, HTML through a headless Edge/Chrome/Chromium, and Markdown through Pandoc followed by that browser.

## Implementation status

This document describes the hybrid design. EPUB and SVG are still targets only. Everything else in the matrix is wired:

- **Supported inputs:** `pdf` (passthrough); the raster images `jpg`, `jpeg`, `png`, `tif`, `tiff`, `bmp`, `webp`; `heic`/`heif`; Office `docx`, `doc`, `xlsx`, `xls`, `pptx`, `ppt`; OpenDocument `odt`, `ods`, `odp`; `rtf`, `txt`; `html`, `htm`; `md`, `markdown` (`src-tauri/src/engine/mod.rs::SUPPORTED_EXTS`). The per-format matrix and each converter's exact command line are in [supported-formats.md](../03-requirements/supported-formats.md). **ImageMagick is not a target at all**: the image path is in-process and stays that way.
- **Conversion:** PDFs are validated by their `%PDF-` header and printed in place (no cache file). Images are decoded with the pure-Rust `image` crate and wrapped into a one-page PDF built by hand with `lopdf`, **not** `printpdf`, whose image feature is disabled. HEIC is decoded by `sips` (macOS) or WIC (Windows) into the same one-page PDF. Documents are converted by external programs through `engine::convert::process::run_bounded`. It uses an argument vector (no shell) and a private scratch folder removed on every exit path, and it is bounded by the conversion timeout and the cancel flag, either of which kills the whole child process tree. Documented limitations of the image path: EXIF orientation is not applied, multi-frame TIFFs use the first frame only, and CMYK JPEGs and other sub-formats the codec cannot handle surface as a clear `ConversionFailed`.
- **Printing (M1):** CUPS `lp` on macOS/Linux; **SumatraPDF** on Windows. It is now **bundled**: `scripts/fetch-sumatrapdf.{ps1,sh}` fetches the pinned 3.5.2 release into `src-tauri/resources/sumatrapdf/`, which `tauri.conf.json` ships as a bundle resource, and `engine::deps::bundled_dirs` looks there first. The search order is bundled → configured → `PATH` → well-known install dirs (`%LOCALAPPDATA%`, `%ProgramFiles%`, `%ProgramFiles(x86)%` under `SumatraPDF\`), and where a binary was found is reported to the UI as `found_in`. A genuinely missing binary is still a `DependencyMissing` that pauses the job, with FR-058 remediation text. Real flags are in §3.1 / §3.6.
- **Dependency probing (FR-058):** `engine::deps` probes SumatraPDF, LibreOffice, Chromium/Edge and Pandoc on every `app_info` call (a handful of `is_file` checks). For each dependency it reports whether it is *applicable on this OS at all* and which extensions it blocks while missing. The applicability flag matters: SumatraPDF is irrelevant on macOS, and reporting it as "missing" there would be the loudest possible false alarm. The converters use the same resolver, and a missing one returns `DependencyMissing` with the same remediation text, which pauses the job.
- **Capability gate:** only **duplex** is hard-checked — a duplex request against a non-duplex driver is an error (the failure would otherwise be invisible at the tray); colour on a mono printer is allowed (it prints grey, which is visible). See `engine::print::check_supported`.
- **Orchestration lives in the queue, not the engine.** Conversion/print concurrency (default ≤ 2 conversions, 1 print), retries with backoff, cancellation, and the conversion cache are owned by the dispatcher — see [queue.md](../05-features/queue.md) and [data-model § 4](./data-model.md#4-queuejson--live-print-queue). The converter/printer functions here are pure, blocking, single-file units the dispatcher drives, with a page-range parser (`engine::pagerange`) shared by validation and the backends.
- **The headless CLI does not use the queue at all.** `maeclyprint print` walks the same `engine::convert` → `engine::print::spool` path directly, into a private temp cache deleted on exit, with **no `queue.json` and no retries** — see [ipc-and-cli.md §3.3](./ipc-and-cli.md#33-flag-semantics).

## 1. Why a hybrid engine

Two extreme designs are tempting and both are wrong:

| Design                             | Why it fails                                                                                                  |
| ---------------------------------- | -------------------------------------------------------------------------------------------------------------- |
| **Pure native per format**         | Every format requires a different printing API (Office automation, GDI+ for images, browser print for HTML). Maintenance balloons; bugs multiply per backend. |
| **Pure single-engine (Ghostscript only)** | Ghostscript can rasterize many formats but quality on Office docs is poor and licensing is GPL with edge cases. Doesn't handle DOCX or images natively without conversion anyway. |

The hybrid choice — **convert each format to PDF with the best-of-breed tool, then print PDF with one well-understood engine** — wins because:

- **Quality.** LibreOffice's DOCX → PDF is the most accurate free converter. Chromium prints HTML the way the user sees it. Pandoc gives nicely-typeset Markdown. (Images are the exception to "best-of-breed external tool": a raster image needs no layout engine, so wrapping it in a PDF in-process is both simpler and one fewer dependency the user must install.)
- **Simplicity.** The final printing path has one shape: `print_pdf(file, settings)`. All printer settings (duplex, color, scale, range, copies, paper size) are set once at the printer-adapter layer.
- **Testability.** The "did it print correctly?" question reduces to "did we generate a correct PDF?" — and PDFs are inspectable.
- **Replaceability.** Swapping LibreOffice for a different DOCX converter does not affect the printer code path at all.

## 2. Format → converter → final-printer matrix

| Format           | Converter           | Tool invoked                   | Final printer (Win)  | Final printer (Mac)  |
| ---------------- | ------------------- | ------------------------------ | -------------------- | -------------------- |
| `.pdf`           | (passthrough)       | —                              | SumatraPDF           | CUPS `lp`            |
| `.docx` `.doc`   | LibreOffice         | `soffice --headless --convert-to pdf` | SumatraPDF    | CUPS `lp`            |
| `.xlsx` `.xls`   | LibreOffice         | `soffice --headless --convert-to pdf` | SumatraPDF    | CUPS `lp`            |
| `.pptx` `.ppt`   | LibreOffice         | `soffice --headless --convert-to pdf` | SumatraPDF    | CUPS `lp`            |
| `.odt` `.ods` `.odp` | LibreOffice     | `soffice --headless --convert-to pdf` | SumatraPDF    | CUPS `lp`            |
| `.rtf`           | LibreOffice         | `soffice --headless --convert-to pdf` | SumatraPDF    | CUPS `lp`            |
| `.txt`           | LibreOffice (UTF-8 import) | `soffice --headless --infilter=… --convert-to pdf` | SumatraPDF | CUPS `lp`            |
| `.png` `.jpg` `.jpeg` `.tif` `.tiff` `.bmp` `.webp` | **in-process** (`image` + `lopdf`) | — (no subprocess) | SumatraPDF | CUPS `lp` |
| `.heic` `.heif`  | OS decoder          | `sips -s format png` (macOS) / WIC (Windows) | SumatraPDF | CUPS `lp`      |
| `.html` `.htm`   | Chromium headless   | `<browser> --headless --print-to-pdf=...` | SumatraPDF | CUPS `lp`            |
| `.md` `.markdown` | Pandoc → browser   | `pandoc --to=html5 --standalone`, then the HTML row | SumatraPDF | CUPS `lp`     |
| `.epub`          | *(target)* Pandoc → browser | — | — | — |
| `.svg`           | *(target)* browser  | — | — | — |

Anything not in this table (and the two *target* rows) is **unsupported**. It is refused at drop time with a reason, never skipped silently.

`.gif` is not in the supported list: the `image` crate's `gif` codec is not enabled.

Detection is by **extension**, lowercased. There is no magic-byte fallback: `infer` and `mime_guess` are not dependencies, and a file whose extension lies is rejected by the converter with a `ConversionFailed` naming the decoder's own complaint.

## 3. Concrete invocations

### 3.1 SumatraPDF (Windows, shipped)

```
SumatraPDF.exe ^
  -print-to "HP LaserJet Pro M404dn" ^
  -print-settings "1-5,duplex,monochrome,fit,paper=A4,2x" ^
  -silent ^
  -exit-when-done ^
  "%LOCALAPPDATA%\Maecly\MaeclyPrint\cache\conversions\<job-id>.pdf"
```

The `-print-settings` value is a comma-separated keyword list MaeclyPrint emits in a fixed order (SumatraPDF is itself order-independent; the fixed order keeps the honesty preview stable):

1. **page ranges** — bare, no key (e.g. `1-5,8`); omitted when printing all pages.
2. **duplex** — `simplex` / `duplex` (long edge) / `duplexshort`.
3. **colour** — `color` / `monochrome`.
4. **scaling** — `fit` / `shrink` / `noscale`.
5. **paper** — `paper=A4` / `paper=Letter` / `paper=Legal` / `paper=A3`.
6. **copies** — `<n>x` (e.g. `3x`), only when > 1. SumatraPDF silently skips an unknown `copies=<n>` token.
7. **orientation** — `landscape` / `portrait`; `auto` emits nothing.

Defaults serialise to `simplex,color,fit,paper=A4`. Translation from the wire `JobSettings` lives in `engine/print/sumatra.rs`, and the same builder feeds the UI's honesty preview so what the user sees is exactly what runs. The child is spawned with the console window suppressed; SumatraPDF prints synchronously and reports no spooler job id.

### 3.2 LibreOffice (cross-platform)

```
soffice ^
  -env:UserInstallation=file:///C:/Users/.../AppData/Local/Temp/maeclyprint-convert-<job>-XXXX/profile ^
  --headless --invisible --nologo --nodefault --nolockcheck --norestore ^
  --convert-to pdf ^
  --outdir "%TEMP%\maeclyprint-convert-<job>-XXXX\out" ^
  "%TEMP%\maeclyprint-convert-<job>-XXXX\input.docx"
```

(`engine::convert::office`.) Caveats:

- `soffice` does not allow concurrent invocations against the same user profile, and without its own profile a headless run hands the file to an already-open LibreOffice window. Each conversion therefore gets a throwaway `-env:UserInstallation` profile inside its scratch folder. Conversions can then run in parallel, and while the user has LibreOffice open.
- The input is copied to `input.<ext>` first, so the output name is predictable and no lock file appears next to the user's document. `.txt` adds `--infilter="Text (encoded):UTF8,LF,,,"`.
- `soffice` exits 0 even when it could not load the document, so success means `out/input.pdf` exists and starts with `%PDF-`. Otherwise the error quotes stderr.
- First start with a fresh profile costs a few seconds per file. The default conversion timeout is 60 s, and LibreOffice is killed (with `soffice.bin`) when it runs out. Macros are not executed. A password-protected file produces no PDF and fails with `ConversionFailed`.

### 3.3 Images (in-process, shipped — no external tool)

There is **no ImageMagick invocation**. `engine::convert::image::to_pdf` decodes the file with the `image` crate and hand-builds a one-page PDF with `lopdf`:

- **Why `lopdf` and not `printpdf`'s image API:** `printpdf` is pulled in with `default-features = false`, which disables its `images` feature — its image-XObject serialiser is then a `panic!` stub. `lopdf` is already a direct dependency and emits a minimal, correct image XObject (`/DeviceRGB`, 8 bits per component) with no extra crates.
- **Page sizing:** the page is sized to the image's pixels at **96 DPI** (`CONVERT_DPI`), converted to PDF points at 72 pt/in. The converter is deliberately paper-agnostic — the driver does the real fitting to A4/Letter via the scaling flags (§3.1/§3.6).
- **Ceilings:** the largest emitted page dimension is 14,400 pt (200 in, the classic Acrobat limit); an enormous image is scaled down uniformly rather than producing an out-of-spec page. Decoding is bounded by `image::Limits` (max width/height 30,000 px) as a decompression-bomb guard, because the crate enforces no dimension limits by default.
- **Progress** is reported at `0.0` (before decode), `0.5` (before embedding) and `1.0` (after save).

Known limitations, documented rather than treated as bugs:

- **EXIF orientation is not applied.** The `image` crate does not auto-rotate and we do not read EXIF, so a rotated photo prints in its stored orientation. (This is the one place the old ImageMagick `-auto-orient` claim promised something we do not do.)
- **Multi-frame TIFF / animations: first frame only.**
- **CMYK JPEGs** and other sub-formats the codec cannot handle surface as `ConversionFailed` carrying the decoder's message.
- **Zero-width or zero-height images** are rejected explicitly rather than producing a degenerate page.

### 3.4 Chromium headless

```
msedge.exe ^
  --headless --disable-gpu --no-pdf-header-footer --print-to-pdf-no-header ^
  --user-data-dir="%TEMP%\maeclyprint-convert-<job>-XXXX\browser" ^
  --no-first-run --no-default-browser-check --disable-extensions ^
  --disable-background-networking --disable-sync --mute-audio ^
  --print-to-pdf="%TEMP%\maeclyprint-convert-<job>-XXXX\out.pdf" ^
  "file:///C:/Users/.../page.html"
```

(`engine::convert::html`.) No `--no-sandbox`: the browser's sandbox stays on. The browser is located through `engine::deps` (bundled → configured → `PATH` → install dirs), with `msedge.exe` before `chrome.exe` on Windows and Edge, Chrome, then Chromium on macOS. A private `--user-data-dir` keeps the user's browser and profile out of it.

The step ends when the PDF is **complete**, not when the browser exits. Chrome 156 on macOS writes the PDF in about a second and then never exits. `pdf_is_complete` waits for the `%%EOF` trailer and a size that did not change between two polls, and then the browser tree is killed.

If no browser is found, the job pauses with *"Microsoft Edge or Google Chrome is missing — HTML and Markdown files cannot be printed until one is installed…"* and download links.

### 3.5 Pandoc

```
pandoc --from=markdown --to=html5 --standalone --metadata=pagetitle:notes ^
       --include-in-header=<scratch>\head.html --output=<scratch>\page.html "notes.md"
```

(`engine::convert::markdown`.) This is always Markdown → HTML → the §3.4 browser step. There is no wkhtmltopdf or TeX path, so no TeX install is needed. The injected header carries a print stylesheet and `<base href>` set to the Markdown file's folder, so relative images resolve. Pandoc and the browser share one timeout budget, and a Pandoc failure stops the pipeline before the browser starts.

### 3.6 CUPS `lp` (macOS / Linux, shipped)

```
lp -d "OfficeJet_Pro" \
   -t "report.pdf" \
   -o sides=two-sided-long-edge \
   -o ColorModel=Gray \
   -n 2 \
   -o media=A4 \
   -o fit-to-page \
   -o page-ranges=1-5 \
   "~/Library/Caches/com.Maecly.MaeclyPrint/conversions/<job-id>.pdf"
```

Flag mapping (`engine/print/cups.rs`), in the exact order `lp_args` emits them:

| Setting | Flag |
| --- | --- |
| printer | `-d <name>` |
| title | `-t <title>` |
| duplex none / long / short | `-o sides=one-sided` / `two-sided-long-edge` / `two-sided-short-edge` |
| colour | `-o ColorModel=RGB` *only when the printer lists `RGB`; else omitted* |
| monochrome | `-o ColorModel=Gray` |
| copies > 1 | `-n <copies> -o collate=true\|false` |
| paper | `-o media=<the printer's keyword>` *(`auto` omits)* |
| tray (job's, else the printer's override) | `-o InputSlot=<bin> -o media-source=<bin>` *(none omits)* |
| quality | `-o print-quality=3\|4\|5 -o cupsPrintQuality=Draft\|Normal\|High` |
| orientation portrait / landscape | `-o orientation-requested=3` / `=4` *(`auto` omits)* |
| scaling fit **or** shrink | `-o fit-to-page` *(`none` omits)* |
| page ranges | `-o page-ranges=<r>` (whitespace stripped) |

Three things worth stating plainly:

- **`-d` and `-n`, not `lpr -P` and `-o copies=N`.** The engine invokes `lp`, whose own flags these are. Earlier drafts described the `lpr` spelling; it was never what the code emitted.
- **`sides`, `media` and quality are always emitted** (media unless the job asks for the printer's own paper, `auto`), even for the defaults, so an explicit "single-sided" or "A4" beats a driver default that says otherwise. Everything else is emitted only when it differs from the driver default or names a real choice (a tray, an orientation, several copies), so the command stays readable.
- **`fit` and `shrink` collapse to the same flag.** CUPS exposes exactly one `fit-to-page` toggle and cannot express "shrink oversized pages but never enlarge". So on macOS/Linux the two settings are indistinguishable at the spooler: a small page under `shrink` may still be scaled up. On Windows they are distinct (`fit` vs `shrink` in `-print-settings`). This is a real cross-platform behavioural difference, not a rounding error, and the honesty preview shows the user the actual flag either way.

On success `lp`'s `request id is <dest>-<n>` line yields the CUPS job id used to follow the job and to `cancel` it. `<dest>` may itself contain `-`, so the number is only ever the last segment. `lp` is run in the C locale, but macOS localizes this line from the user's language list anyway (`el id de la petición es Office-42 (1 archivos)`), so the parser does not depend on the English marker: it falls back to the token naming the destination, then to any `<name>-<digits>` token.

The job is then followed until it leaves the queue (`engine::print::job_state`): `ipptool -tv ipp://localhost/jobs/<n> get-job-attributes.test` gives the IPP `job-state` (`pending`, `pending-held`, `processing`, `processing-stopped`, `canceled`, `aborted`, `completed`), and where `ipptool` is missing `lpstat -W not-completed -o <dest>` still tells a waiting job from a finished one. It cannot tell *how* a job finished (CUPS' completed list holds printed, cancelled and aborted jobs alike), so a job that left the queue is `Gone` (done, logged as `left_queue`), never reported as confirmed printed. An `lpstat` that exits non-zero (cupsd restarting or unreachable) is a transient tracking failure, retried and then given up as `unconfirmed`, never read as "no longer listed". On Windows, SumatraPDF reports no id, so the spooler's job list is read before and after spooling and the new job carrying the file's name is followed with `GetJobW` (`JOB_STATUS_*` flags; a job the spooler no longer knows has left the queue). The dispatcher owns the cadence and the budget — see [queue.md](../05-features/queue.md#dispatcher-semantics-what-the-ui-reflects).

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
|    (parallel)      |  Each writes to %LOCALAPPDATA%\Maecly\MaeclyPrint\cache\<job>.pdf
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
| 6. Printer dispatch|  Sequential (one print at a time). Calls SumatraPDF / `lp`
|    (sequential)    |  with the PrintSettings derived from Profile + overrides.
+----------+---------+
           |
           v
+--------------------+
| 7. Status polling  |  Spooler queried for job state; surfaced in UI.
|    + log emit      |  JSONL entry per state transition.
+--------------------+
```

### 4.1 Router selection

The router is **not** a MIME-keyed trait-object factory. `engine::convert::convert_to_pdf` is a plain function that switches on the lowercased extension and returns one of two things:

```rust
// src-tauri/src/engine/convert/mod.rs (shape, not verbatim)
// Ok(None)        -> passthrough: the input is already a printable PDF
// Ok(Some(path))  -> a converted PDF was written to cache_dir/<job-id>.pdf
// Err(Unsupported/ConversionFailed)
```

- **`pdf`** → validated cheaply by its `%PDF-` header and printed in place: `Ok(None)`, no cache file.
- **Raster images** (`jpg`, `jpeg`, `png`, `tif`, `tiff`, `bmp`, `webp`) → `image::to_pdf`, `Ok(Some(path))`.
- **`heic`/`heif`** → `heif::convert` (OS decoder), `Ok(Some(path))`.
- **Office / ODF / `txt` / `rtf`** → `office::convert_with` (LibreOffice).
- **`html`/`htm`** → `html::convert_with` (browser). **`md`/`markdown`** → `markdown::convert_with` (Pandoc, then the browser).
- **Everything else** → `EngineError::UnsupportedFormat`.

`convert::route(ext)` is the table. The external program is resolved before anything is written, so a missing one costs nothing but a paused job. The trait-object design above was never implemented and there is no `JobPlan` type. The router is a match on `Route`, not a plugin registry.

### 4.2 PDF caching

Converted PDFs land in a `conversions/` subdirectory of the OS cache directory — `%LOCALAPPDATA%\Maecly\MaeclyPrint\cache\conversions\<job-id>.pdf` on Windows, `~/Library/Caches/com.Maecly.MaeclyPrint/conversions/<job-id>.pdf` on macOS (`queue::spawn` resolves `paths::cache_dir().join("conversions")`). The cache:

- Is **not** keyed on file content; each job gets its own cache file. This avoids spurious "is the file modified?" decisions.
- Holds nothing for a PDF input: a PDF passes through and is printed in place, so no cache file is produced at all.
- Is swept at app start, which is precisely why the single-instance plugin must be registered **before** any of that work runs — a second launch sweeping the cache would destroy the running instance's in-flight jobs (NFR-014).
- The **CLI does not share it.** `maeclyprint print` converts into a private temp directory deleted when the process exits, so a script and a running app cannot delete each other's work.
- There is **no size-capped eviction** (`cache_max_bytes`) today. Files are removed with their jobs (`clear_done` deletes a job's cache), not by a byte budget.

### 4.3 Size and probe ceilings

Two different limits are easy to confuse:

- **`engine.max_file_size_mb`** (configurable, default and bounds in `ENGINE_LIMITS`) is a *submission* ceiling. A file over it is refused at submit with `InvalidRequest` ("… is larger than the N MB limit"), and a file found inside a folder walk is dropped with `SkipReason::TooLarge` and a human detail string rather than silently vanishing (FR-005/FR-059). A stored `0` here is treated as bricked input — it would reject every file — so it is rejected on write and sanitized back to the default.
- **`MAX_PDF_PROBE_BYTES` = 100 MB** (not configurable) is a *page-count* ceiling and has nothing to do with whether a file prints. PDFs above it are simply not parsed with `lopdf` at submit/inspect time, because that parse would stall the UI; their `pages` stays `null` until after conversion. A 300 MB PDF prints normally, it just shows no page count in the dialog.

Folder expansion has its own independent ceilings, `MAX_FOLDER_FILES = 5,000` entries and `MAX_FOLDER_DEPTH = 16` levels, reported back on the wire (`FolderExpansion.limits`, `files_truncated`, `depth_truncated`) so the UI can phrase the warning without hardcoding them.

## 5. Error handling matrix

Rows marked **(target)** are not implemented yet; the rest are shipped behaviour.

| Error | Detection | Error code | Queue effect |
| --- | --- | --- | --- |
| Print dependency missing (`lp`, `SumatraPDF.exe`) | `engine::deps` probe: bundled → configured → `PATH` → install dirs | `DependencyMissing` (CLI exit **3**) | Job paused, with FR-058 remediation text; never reported on an OS where the dependency is inapplicable |
| Conversion timeout | `conversion_timeout_seconds` elapsed | error code `"conversion_timeout"` | **Terminal — not retried** (see §5.1) |
| Print timeout | `wait-timeout` on the print child | `PrintFailed` (CLI exit **9**) | Retryable |
| Image decode failure (CMYK JPEG, corrupt file, 0×0) | the `image` crate's own error | `ConversionFailed` (CLI exit **8**) | Retryable, carries the decoder's message |
| File path no longer exists | `canonical_input` at submit | `FileNotFound` (CLI exit **6**) | Rejected before enqueue |
| Relative path from IPC/drop | `canonical_input` | `InvalidRequest` (CLI exit **2**) | Rejected; only the CLI resolves relative paths |
| File is a directory | `inspect_files` reports `is_dir` | — | Not an error: the caller asks the FR-004 recurse question and calls `expand_folders` |
| Unsupported format | extension not in `SUPPORTED_EXTS` | `UnsupportedFormat` (CLI exit **7**) | Rejected before enqueue |
| File over `max_file_size_mb` | `stat` at submit / during the folder walk | `InvalidRequest`, or `SkipReason::TooLarge` in an expansion | Rejected / skipped with a reason, never silently dropped |
| Duplex requested, driver cannot | `engine::print::check_supported` | `UnsupportedSetting` → CLI exit **5** (a *printer* problem, not a usage one) | Job fails |
| No printer configured at all | printer resolution at dispatch | `PrintFailed` → CLI exit **5** | GUI pauses the job so the user can pick another; CLI fails immediately |
| Named printer not installed | printer resolution at dispatch | `PrintFailed` → CLI exit **5** | Same split as above |
| Spooler rejected the job | non-zero exit / stderr from `lp`/SumatraPDF | `PrintFailed` (CLI exit **9**) | Retryable; the spooler's own text is surfaced verbatim |
| App was killed mid-job | recovery at next start (`queue::persist::recover`) | error code `"interrupted_by_shutdown"` | Job is `paused` and treated as **resumable**, not failed |
| User cancellation | `cancel_job` / `cancel_all` | — | Job `cancelled` (terminal); a terminal job refuses cancellation with `InvalidState` |
| Converter binary not found (LibreOffice, browser, Pandoc, Windows HEIF codec) | `engine::deps` resolution before the conversion starts; WIC's "no decoder" result | `DependencyMissing` (CLI exit **3**) | Job paused with the FR-058 remediation (what is blocked and where to download it) |
| Converter exit ≠ 0, or exit 0 without a PDF (e.g. a password-protected document) | child exit status, then a `%PDF-` check of the expected output | `ConversionFailed`, quoting the converter's stderr | Retryable, like any conversion failure |
| Converter hangs | the conversion timeout raises the cancel flag, and `run_bounded` kills the process tree | error code `"conversion_timeout"` | Terminal (see §5.1); the scratch folder is removed |
| Encrypted PDF *(target)* | not detected today — an encrypted PDF passes the `%PDF-` header check and fails at the printer | — | — |

Two honest gaps worth naming: there is **no live spooler polling**, so "printer offline" / "out of paper" are not distinguished as their own states — they surface as whatever the spooler says when the job is submitted. And there is no encrypted-PDF pre-check (`qpdf` is not a dependency).

### 5.1 Retry policy

The retry policy lives in the **dispatcher**, not the engine (`queue/dispatcher.rs`), and is the same for every failure that is retryable at all:

- **`MAX_ATTEMPTS = 4`** — the initial attempt plus **three** automatic retries.
- **`RETRY_BACKOFFS = [1 s, 5 s, 25 s]`** — the delay before retry *n*. A job that fails a fourth time is terminal (`Failed`).
- A job waiting out a backoff is held aside and skipped by the scheduler, so it does not occupy a conversion slot.

Earlier drafts said "3 attempts with 1 s/5 s backoff"; both numbers were wrong. Four attempts and three backoffs is what the code does and what its tests assert (`fr_034`-tagged tests advance tokio's virtual clock by exactly 1 s, 5 s and 25 s).

**Conversion timeouts are not retried.** A conversion that exceeds `engine.conversion_timeout_seconds` fails with the `"conversion_timeout"` error code and goes straight to `Failed` — retrying a deterministic timeout would just burn three more of them. Setting the value to `0` disables the timeout entirely; that is the one engine knob where `0` is a meaningful value rather than bricked input (`ZeroPolicy::Disables`), and the write path accepts it for exactly that reason. The **print** hop is bounded separately, by `engine.print_timeout_seconds` (default 300, `0` disables): the spooler child's wait (`wait-timeout`, killed on expiry) and the wait for the OS job to leave the queue share that one budget. A print timeout is terminal too (`"print_timeout"`, no automatic retry): re-spooling a job that wedged the spooler, or one still sitting in the OS queue, would most likely wedge again or print twice.

`retry_job` (UI/IPC) resets `attempts`, so a user can always ask for a fourth-and-fifth chance manually.

## 6. Performance characteristics

> **These figures are design-target estimates, not measurements.** The converters are wired, but nobody has benchmarked them yet. The image row also describes ImageMagick, which is not used at all. Treat the table as a budget to validate against, and replace it with real numbers. For reference, on an Apple-silicon Mac the real-tool tests convert a TXT, an RTF and a DOCX (LibreOffice, a fresh profile each), an HTML page (Chrome) and a Markdown file (Pandoc + Chrome) in parallel in about 5 s in total.

Intended envelope on a Windows 11 machine, Ryzen 7 5800X, NVMe SSD, files in `%TEMP%`:

| Scenario                               | Per-file time | Throughput     |
| -------------------------------------- | ------------- | -------------- |
| 50× small PDFs (passthrough)           | ~50 ms        | ~20 PDFs/sec spooled |
| 50× 5-page DOCX (LibreOffice, parallel ×4) | ~1.8 s avg | ~2.2 docs/sec to PDF |
| 100× JPG photos (ImageMagick parallel ×8) | ~120 ms avg | ~8 imgs/sec to PDF |
| 20× HTML pages (Chromium parallel ×4)  | ~900 ms avg   | ~4 pages/sec to PDF |

These are *converter* throughputs. The printer itself is the long pole for actual ink-on-paper.

## 7. Configuration knobs

The **whole** `[engine]` section is four keys, in `config.toml` (see [data-model.md §1](./data-model.md#1-file-locations) for where that file lives — it is *not* `~/.config/MaeclyPrint/` on Windows or macOS):

```toml
[engine]
max_concurrent_conversions = 2   # 1..=8, applied at the next app start
conversion_timeout_seconds = 60  # 0 disables the timeout
print_timeout_seconds = 300      # 0 disables it
max_file_size_mb = 200           # submission ceiling, in MB
default_profile = "…"            # profile id; omitted → builtin-quick
```

Bounds, defaults and the meaning of a stored `0` for each knob come from a single source of truth, `ENGINE_LIMITS` (`settings/mod.rs`): the defaults, the sanitizer that clamps a hand-edited file, the write-path validator, **and** the `limits` the Settings screen validates against are all the same numbers. The UI therefore cannot drift from the server's bounds.

`max_concurrent_conversions` takes effect at the **next start** — the dispatcher reads it once at boot and changing it mid-session does not re-tune the running one.

Keys named in earlier drafts that **do not exist**: `max_file_size_bytes` (it is `max_file_size_mb`), `cache_dir`, `cache_max_bytes`, the entire `[engine.converters]` table, `[engine.printer.windows].sumatra_path`, and `[engine.printer.macos].lpr_path`. External programs are **located by probing**, not configured: `engine::deps` searches bundled → configured → `PATH` → well-known install dirs and reports where it found each one. There is likewise no `[logging]` section — log rotation and retention are fixed constants (see [data-model.md §5.2](./data-model.md#52-rotation-and-retention)).

## 8. Extensibility

Adding a new format is three changes:

1. Add the converter module under `engine/convert/` as a blocking, single-file function with the same `(input, output, progress) -> Result<(), EngineError>` shape.
2. Add the extension to `SUPPORTED_EXTS` and an arm to `convert_to_pdf`.
3. Add a row to the format table in this document, and to the §5 error matrix if it introduces a new failure mode.

If the converter shells out to a program, also add it to `engine::deps` (spec, executable names, install dirs, `blocks` list, remediation text) so a missing tool is reported as a dependency rather than as a mysterious conversion failure — and bring the subprocess hardening listed in [`security-model.md`](./security-model.md) §3.4 with it.

No printer-side change is needed because the new converter feeds the same PDF pipeline.

## 9. Related documents

- [`system-architecture.md`](./system-architecture.md) — the engine in context.
- [`data-model.md`](./data-model.md) — `PrintSettings` schema referenced above.
- [`ipc-and-cli.md`](./ipc-and-cli.md) — exposing engine controls to the UI and CLI.
- ADR [`0003-hybrid-print-engine-strategy`](../adr/0003-hybrid-print-engine-strategy.md) — why this design.
