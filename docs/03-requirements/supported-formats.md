# Supported Formats

> **TL;DR** — Per-format support matrix. MaeclyPrint prints **PDF**, raster images (**JPG, PNG, TIFF, BMP, WebP**), **HEIC/HEIF**, **Office** (DOCX, XLSX, PPTX and legacy DOC, XLS, PPT), **OpenDocument** (ODT, ODS, ODP), **TXT**, **RTF**, **HTML** and **Markdown**. PDFs print as they are and images are converted inside the app. Everything else is converted by a program already on the machine: the OS image decoder (HEIC), LibreOffice (Office, ODF, TXT, RTF), Edge/Chrome/Chromium (HTML), and Pandoc followed by the browser (Markdown). If a converter is missing, its job pauses with a message that names it and explains how to install it (FR-058). The file is never silently skipped.

The list of formats is intentionally **bounded**. Print Conductor advertises 90+ formats but supports many of them poorly. We aim to support **fewer formats, very well**, with predictable behavior and clear failure modes.

## The source of truth

The authoritative list is a single constant in `src-tauri/src/engine/mod.rs`:

```rust
pub const SUPPORTED_EXTS: &[&str] = &[
    "pdf", "jpg", "jpeg", "png", "tif", "tiff", "bmp", "webp", "heic", "heif", "docx", "doc",
    "xlsx", "xls", "pptx", "ppt", "odt", "ods", "odp", "rtf", "txt", "html", "htm", "md",
    "markdown",
];
```

Folder expansion, the drop handler, the CLI and the Print Job screen all consult `engine::is_supported`. `engine::convert::route` maps every extension in that list to exactly one converter, and a unit test checks that the two agree (`fr_052_to_057_every_supported_extension_has_a_route`). The UI mirror lives in `src/lib/formats.ts`. It feeds the OS file-picker filters and the Home drop zone, and `HomeScreen.test.tsx` diffs it against the Rust constant.

## Format matrix

| Extension(s) | Converter | External program | Final printer (Win / macOS) | Fidelity |
|---|---|---|---|---|
| `.pdf` | none: passthrough after a `%PDF-` header check | — | SumatraPDF / `lp` | Excellent (native) |
| `.jpg` `.jpeg` `.png` `.tif` `.tiff` `.bmp` `.webp` | built-in `image` crate → one-page PDF | — | SumatraPDF / `lp` | Excellent (WebP: good) |
| `.heic` `.heif` | OS decoder → one-page PDF | macOS: `sips` (built in). Windows: WIC + Microsoft Store *HEIF Image Extensions* | SumatraPDF / `lp` | Excellent |
| `.docx` `.doc` `.xlsx` `.xls` `.pptx` `.ppt` | LibreOffice headless | `soffice` | SumatraPDF / `lp` | Good (LibreOffice rendering) |
| `.odt` `.ods` `.odp` | LibreOffice headless | `soffice` | SumatraPDF / `lp` | Excellent |
| `.txt` `.rtf` | LibreOffice headless (TXT is read as UTF-8) | `soffice` | SumatraPDF / `lp` | Good |
| `.html` `.htm` | Chromium-class browser, `--print-to-pdf` | Edge, Chrome or Chromium | SumatraPDF / `lp` | As the browser prints it |
| `.md` `.markdown` | Pandoc → standalone HTML → the browser step | `pandoc` + Edge/Chrome/Chromium | SumatraPDF / `lp` | Good |

Everything not listed here is refused at drop time with a reason (see below).

## How a converter runs

All external converters share one runner, `engine::convert::process::run_bounded`. They follow the same rules:

- **No shell.** The program and its arguments are passed as an argument vector. A file called `$(rm -rf ~).docx` is just an odd file name. LibreOffice also works on a private copy (`input.<ext>`), so the original name never reaches its command line.
- **Bounded.** Each child is polled every 100 ms against `engine.conversion_timeout_seconds` and the job's cancel flag. The dispatcher's conversion timer raises that flag too. On either signal, the child **and its helpers** are killed: the process group on Unix, `taskkill /T` on Windows.
- **Private scratch folder.** Each conversion gets its own `maeclyprint-convert-<job>-*` folder in the OS temp directory. That folder holds the LibreOffice profile, the browser profile, the generated HTML and the converter's stderr log. It is removed on every exit path: success, failure, cancel or timeout. Only the final PDF moves to the conversion cache (`<cache>/conversions/<job>.pdf`).
- **The output decides.** LibreOffice exits 0 even when it could not load a document, so success means the expected PDF exists and starts with `%PDF-`. If it does not, the error quotes the tail of the converter's stderr.
- **A missing program pauses the job.** The converter is located before anything is written. If it is not found, the result is `DependencyMissing` with the FR-058 remediation text, and the job is **paused**, not failed. Once the user has installed the program, resuming the job is enough.
- **Pages appear after conversion.** A document's page count is unknown when it is added (the Print Job row shows no count). The conversion task measures the converted PDF, and the job, the queue views and Home show that number.
- **Page ranges.** A range applies to the pages of the converted PDF for PDFs and documents. Images and HEIC are one page, so the engine drops the range for them (`engine::print::effective_settings`). Before submitting, the range is checked only against PDFs whose page count is known.

## How a rejected file is reported

Nothing is dropped silently (FR-005/FR-059). Folder expansion returns every discarded entry as a `SkippedFile` carrying a `SkipReason` (`UnsupportedFormat`, `TooLarge` or `Unreadable`). Each entry also has a human-readable `detail`, such as `"Unsupported file type (.exe)"`, ready for the job log and the UI. A supported format whose converter is missing is **not** rejected. It is accepted, and the Print Job screen shows the remediation for the dependencies that the selected files need.

## Per-format details

### PDF

No conversion: the final printing engine is called directly. `convert_to_pdf` validates the `%PDF-` header (cheap: five bytes) and returns `Ok(None)`, meaning "print the original in place". A mislabelled or truncated file is rejected before it reaches the spooler.

```powershell
# Windows: the real argument vector (engine::print::sumatra)
SumatraPDF.exe -print-to "PrinterName" -print-settings "1-5,duplex,color,fit,paper=A4" -silent -exit-when-done file.pdf
```

```bash
# macOS: the real argument vector (engine::print::cups). `lp`, not `lpr`.
lp -d PrinterName -t "job title" -o sides=two-sided-long-edge -o media=A4 -o fit-to-page file.pdf
```

**Edge cases:**
- Corrupt or mislabelled PDFs: rejected by the header check with `ConversionFailed("not a valid PDF")` before spooling.
- Very large PDFs: no chunked spooling; we rely on the OS spooler.
- Encrypted PDFs: *planned*. The app would prompt for a password (CLI: `--password <pwd>`). There is no `--password` flag today, so an encrypted PDF fails at the spooler.
- Permission-restricted PDFs (no printing flag): *planned*. The app would show an explicit error that suggests `qpdf --decrypt`.

### Images (JPG, PNG, TIFF, BMP, WebP)

These are converted in-process. `engine::convert::image::to_pdf` decodes the image with the pure-Rust `image` crate (built with exactly the `jpeg`, `png`, `tiff`, `bmp` and `webp` codecs). It then embeds the pixels in a one-page PDF, built with `lopdf`, at `cache_dir/<job_id>.pdf`. Progress is reported as `0.0..=1.0`. EXIF orientation is not applied, and only the first frame of a multi-frame TIFF is used.

### HEIC / HEIF

The `image` crate has no HEIF decoder. Bundling `libheif` would bring a native build and HEVC patent questions into the app. Both target OSes already ship a decoder, so the app uses it (`engine::convert::heif`):

- **macOS:** `sips -s format png <in> --out <scratch>/heif.png` (`/usr/bin/sips`, part of the base system), followed by the image path above.
- **Windows:** Windows Imaging Component (`IWICImagingFactory` → frame 0 → `IWICFormatConverter` to 24-bit RGB) feeds the same one-page PDF builder. WIC can decode HEIF only when the Microsoft Store **HEIF Image Extensions** (and **HEVC Video Extensions** for `.heic`) are installed. When they are missing, WIC reports that no decoder exists, and the job pauses with a `DependencyMissing` message that names both extensions.
- **Linux:** no system decoder is assumed. The job pauses and asks the user to convert to JPEG or PNG.

### Office (DOCX, XLSX, PPTX, DOC, XLS, PPT), OpenDocument (ODT, ODS, ODP), TXT and RTF

`engine::convert::office` copies the file into the scratch folder as `input.<ext>` and runs:

```bash
soffice -env:UserInstallation=file:///<scratch>/profile --headless --invisible --nologo \
        --nodefault --nolockcheck --norestore \
        [--infilter="Text (encoded):UTF8,LF,,,"   # .txt only] \
        --convert-to pdf --outdir <scratch>/out <scratch>/input.<ext>
```

- **Private profile per conversion.** LibreOffice allows one process per user profile. Without `-env:UserInstallation`, a headless convert hands the file to an open LibreOffice window, or blocks on its lock. A throwaway profile isolates every conversion, including parallel ones. The cost is LibreOffice's first-start time, a few seconds per file.
- **UTF-8 text.** Without the explicit import filter, LibreOffice guesses the encoding of a `.txt` from the system locale and mangles accented text.
- **Macros are not executed.** LibreOffice headless does not run document macros.
- **Fidelity.** Complex Word layouts and Office 365–specific features can render differently than in Microsoft Word. A spreadsheet prints every sheet that has content.

LibreOffice is located at `/Applications/LibreOffice.app/Contents/MacOS/soffice` on macOS, at `%ProgramFiles%\LibreOffice\program\soffice.exe` (or the x86 path) on Windows, or on `PATH`.

### HTML

`engine::convert::html` prints the file from its own location, so relative CSS and images resolve the way they do in a browser:

```bash
<browser> --headless --disable-gpu --no-pdf-header-footer --print-to-pdf-no-header \
          --user-data-dir=<scratch>/browser --no-first-run --no-default-browser-check \
          --disable-extensions --disable-background-networking --disable-sync --mute-audio \
          --print-to-pdf=<scratch>/out.pdf file:///path/to/page.html
```

- **Browser.** On Windows, Edge comes first (it is always installed), then Chrome. On macOS the order is Edge, Chrome, then Chromium in `/Applications`. On Linux it is `google-chrome`, `chromium`, `chromium-browser`, then `microsoft-edge` on `PATH`.
- **Private browser profile.** The user's open browser and its profile are never touched.
- **The finished PDF ends the step.** Some Chrome builds (seen with Chrome 156 on macOS) write the PDF in about a second and then never exit. The step therefore ends once the PDF has its `%%EOF` trailer and its size has stopped changing between polls. The browser tree is then killed.
- **The job's paper.** The browser lays pages out on US Letter unless the page sets a size, so an A4 job used to get Letter pages (clipped on the right at actual size). When the job names a paper (`A4`, `Letter`, `Legal`, `A3`, `A5`, `A6`, `B4`, `B5`, `Tabloid`, `Executive`, `4x6`-style inch sizes or CUPS `w<pt>h<pt>` keywords), the browser loads a copy of the page from the scratch folder that starts with `<style>@page { size: <w> <h>; }</style>` (landscape swaps the two) and, if the page has none, a `<base href>` to its own folder so relative links still resolve. The user's file is never changed, and a size the page sets itself still wins. Paper `auto` (printer default) and sizes MaeclyPrint cannot measure keep the browser default. A UTF-16 page is printed as is.
- **Scripts run** as they would in the browser. Only local files reach this converter; remote `http(s)://` inputs are not accepted. `@media print` styles are respected.

### Markdown

`engine::convert::markdown` uses Pandoc to produce HTML, then the HTML step above. Pandoc's own PDF output would need a LaTeX distribution, a multi-gigabyte install:

```bash
pandoc --from=markdown --to=html5 --standalone --metadata=pagetitle:<name> \
       --include-in-header=<scratch>/head.html --output=<scratch>/page.html <input.md>
```

The injected header carries `<base href="file:///<folder of the .md>/">`, so relative image links resolve against the Markdown file's folder. It also carries a small print stylesheet (margins, readable type, code blocks, tables) and the job's paper size as an `@page` rule, under the same rules as HTML above. The Pandoc and browser steps share one timeout budget. Pandoc is located on `PATH`, in `/opt/homebrew/bin` or `/usr/local/bin` on macOS, and in `%LOCALAPPDATA%\Pandoc` or `%ProgramFiles%\Pandoc` on Windows. TeX math is passed through as text.

## Out of scope (deferred to v2+ or never)

| Format | Why deferred |
|---|---|
| AutoCAD DWG/DXF | Requires LibreCAD or proprietary ODA SDK; small audience for batch printing |
| Visio (.vsdx) | LibreOffice support is fragile; rarely batch-printed |
| EPUB | Niche use case; better with dedicated reader |
| Email (.eml, .msg, .pst) | Email rendering is non-trivial; users typically use Outlook |
| PostScript (.ps) | Edge case; Ghostscript dependency would inflate installer |
| GIF, SVG | Not enabled; GIF would need the `gif` codec, SVG the browser path |
| Zebra ZPL barcodes | Highly specialized; would be a v2 plugin if demand emerges |
| Plaintext code with syntax highlighting (Pretty Print) | Could be a v2 feature; PrintFile covered this niche |

## Adding a new format

Each converter is a module under `src-tauri/src/engine/convert/`. To add a format:

1. Add a converter module, or extend an existing routing table (`IMAGE_EXTS`, `OFFICE_EXTS`, `HTML_EXTS`, `MARKDOWN_EXTS`, `HEIF_EXTS`).
2. Route it in `convert::route` / `convert_to_pdf`. If it shells out, use `process::run_bounded` and a `WorkDir`.
3. **Add the extension to `engine::SUPPORTED_EXTS`** and to `src/lib/formats.ts`. The route test and the Home test fail if either is missing.
4. If it needs a new external program, add a `Dependency` variant in `engine/deps.rs`. List the extensions it gates in that spec's `blocks` and write the remediation text.
5. Add tests: argument-vector unit tests, fake-binary tests (`external_tests::fakes`), and a real-tool test that generates its fixture in-test and skips when the tool is absent.
6. Update this document and the FR row.

A future plugin API (v2) may allow third-party converters as separate crates loaded at runtime.

## Dependency detection

`engine::deps` resolves external programs:

| Dependency | Executable names | Blocks (extensions) while missing |
|---|---|---|
| SumatraPDF | `SumatraPDF.exe` | On Windows, **every** supported format, because it is the final printer. Never reported off Windows, where CUPS `lp` prints. |
| LibreOffice | `soffice` / `soffice.exe` | `docx doc xlsx xls pptx ppt odt ods odp rtf txt` |
| Chromium-class browser | `msedge.exe`, `chrome.exe` (Win) · `Microsoft Edge`, `Google Chrome`, `Chromium` (macOS) | `html htm md markdown` |
| Pandoc | `pandoc` / `pandoc.exe` | `md markdown` |

HEIC needs no probe on macOS (`sips` ships with the OS). On Windows, the codec is detected when the image is decoded, as described above.

The search order is the same for every dependency: **bundled** next to the executable, then a **configured** path (no setting exists yet), then **`PATH`**, then **well-known install directories**. `resolve` is a pure function, so the Windows search order can be unit-tested from a macOS machine. `FoundIn` records which step found the program, so diagnostics can show which copy was picked.

Each missing dependency's remediation names it, says what cannot print, and links to where to get it. For example: *"LibreOffice is missing — Office, OpenDocument, TXT and RTF files cannot be printed until it is installed. Download it free from https://www.libreoffice.org/download"*. The converter returns the same text as `DependencyMissing`, so the paused job and the Print Job screen give the same explanation.

## Tests

- `engine::convert::process::tests`: zero and non-zero exits, timeout kill (including a background helper in the same process group), cancel kill, a child that never exits after its output is finished, a missing program reported as `DependencyMissing`, and no shell interpolation.
- `engine::convert::external_tests::fakes` (Unix): `/bin/sh` fake `soffice`, `chrome`, `pandoc` and `sips` binaries. They pin the argument vectors that are actually spawned, the "exit 0 with no output" failure, the timeout kill and the scratch-folder cleanup.
- `engine::convert::external_tests::fr_05x_real_*`: real conversions of fixtures generated in-test. A 300-line UTF-8 `.txt` must span several pages, and the queue's page probe must read that count. The other fixtures are an `.rtf`, a `.docx` that LibreOffice creates from a `.txt`, an `.html` with a relative stylesheet, a `.md`, and on macOS a `.heic` that `sips` creates from a PNG. Each test **skips with a note** when its tool is not installed, so a CI runner without LibreOffice stays green while a developer machine runs the real conversion.
