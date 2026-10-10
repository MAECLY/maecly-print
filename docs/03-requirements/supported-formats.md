# Supported Formats

> **TL;DR** — Per-format support matrix. Pluryprint prints **PDF** (encrypted ones too, after asking for the password), raster images (**JPG, PNG, TIFF, BMP, WebP**), **HEIC/HEIF**, **SVG/SVGZ**, **Office** (DOCX, XLSX, PPTX and legacy DOC, XLS, PPT), **OpenDocument** (ODT, ODS, ODP), **TXT**, **RTF**, **HTML** and **Markdown**. **GIF is not supported** and is refused with its own message. PDFs print as they are; images, SVG, Markdown and plain text are converted inside Pluryprint; HTML, Markdown and text are paginated by the operating system's own web view (WebView2 on Windows, WebKit on macOS) in a hidden worker process. Only two kinds of program may have to be installed: **LibreOffice** for Office, OpenDocument and (on Windows) RTF, and on Windows the Microsoft Store **HEIF/HEVC extensions** for HEIC. A missing one pauses the job with a message that names it, and the app offers to install it after asking (FR-058). The file is never silently skipped.

The list of formats is intentionally **bounded**. Print Conductor advertises 90+ formats but supports many of them poorly. We aim to support **fewer formats, very well**, with predictable behavior and clear failure modes.

## The source of truth

The authoritative list is a single constant in `src-tauri/src/engine/mod.rs`:

```rust
pub const SUPPORTED_EXTS: &[&str] = &[
    "pdf", "jpg", "jpeg", "png", "tif", "tiff", "bmp", "webp", "heic", "heif", "svg", "svgz",
    "docx", "doc", "xlsx", "xls", "pptx", "ppt", "odt", "ods", "odp", "rtf", "txt", "html", "htm",
    "md", "markdown",
];
```

Folder expansion, the drop handler, the CLI and the Print Job screen all consult `engine::is_supported`. `engine::convert::route` maps every extension in that list to exactly one converter, and a unit test checks that the two agree (`fr_052_to_057_every_supported_extension_has_a_route`). The UI mirror lives in `src/lib/formats.ts`. It feeds the OS file-picker filters and the Home drop zone, and `HomeScreen.test.tsx` diffs it against the Rust constant.

## Format matrix

| Extension(s) | Converter | External program | Final printer (Win / macOS) | Fidelity |
|---|---|---|---|---|
| `.pdf` | none: passthrough after a structural and an encryption check; a password-protected PDF is decrypted in-process once the user gives the password | — | SumatraPDF / `lp` | Excellent (native) |
| `.jpg` `.jpeg` `.png` `.tif` `.tiff` `.bmp` `.webp` | built-in `image` crate → one-page PDF (transparency flattened onto white) | — | SumatraPDF / `lp` | Excellent (WebP: good) |
| `.heic` `.heif` | OS decoder → one-page PDF | macOS: `sips` (built in). Windows: WIC + Microsoft Store *HEIF Image Extensions* (and *HEVC Video Extensions* for `.heic`) | SumatraPDF / `lp` | Excellent |
| `.svg` `.svgz` | built-in `usvg` + `svg2pdf` → one **vector** page, in a hidden worker process | — | SumatraPDF / `lp` | Excellent (filter effects rasterised) |
| `.docx` `.doc` `.xlsx` `.xls` `.pptx` `.ppt` | LibreOffice headless, hardened private profile | `soffice` | SumatraPDF / `lp` | Good (LibreOffice rendering) |
| `.odt` `.ods` `.odp` | LibreOffice headless, hardened private profile | `soffice` | SumatraPDF / `lp` | Excellent |
| `.rtf` | Windows: LibreOffice. macOS: `textutil` → HTML → the system web view | Windows: `soffice`. macOS: none (`textutil` is part of macOS) | SumatraPDF / `lp` | Good |
| `.txt` | decoded in-process (BOM, UTF-8, else Windows-1252) → wrapping monospaced page → the system web view | — | SumatraPDF / `lp` | Good |
| `.html` `.htm` | the system web view (WebView2 / WKWebView) in a hidden worker, scripts off, no network | — | SumatraPDF / `lp` | As the web view prints it |
| `.md` `.markdown` | `pulldown-cmark` in-process → HTML → the system web view | — | SumatraPDF / `lp` | Good |

Everything not listed here is refused at drop time with a reason (see below). `.gif` has a reason of its own.

## How a converter runs

External converters (LibreOffice, `textutil`, `sips`, the render and SVG workers) share one runner, `engine::convert::process` (`run_bounded` / `run_until`). They follow the same rules:

- **No shell.** The program and its arguments are passed as an argument vector. A file called `$(rm -rf ~).docx` is just an odd file name. LibreOffice and `textutil` also work on a private copy (`input.<ext>`), so the original name never reaches their command line.
- **Bounded.** Each child is polled every 100 ms against `engine.conversion_timeout_seconds` and the job's cancel flag. The dispatcher's conversion timer raises that flag too. On either signal, the child **and its helpers** are killed: the process group on Unix, `taskkill /T` on Windows. A converter that runs out of time is `tool_timed_out` and the job fails without retry.
- **Isolated workers.** Two converters run inside Pluryprint's own executable started as a hidden worker, so a hostile file that crashes or wedges the parser takes the worker down, not the app: `pluryprint --worker render-pdf …` (HTML, Markdown, text, RTF on macOS) and `pluryprint --worker svg <in> <out>`. The console CLI (`pluryprint-cli.exe`) hands that work to the app executable next to it. A worker failure is `render_failed` (or `svg_*`) with the worker's stderr as the technical detail.
- **Private scratch folder.** Each conversion gets its own `pluryprint-convert-<job>-*` folder in the OS temp directory. That folder holds the LibreOffice profile, the staged page, the generated HTML and the converter's stderr log. It is removed on every exit path: success, failure, cancel or timeout. Only the final PDF moves to the conversion cache (`<cache>/conversions/<job>.pdf`).
- **The output decides.** LibreOffice exits 0 even when it could not load a document, so success means the expected PDF exists and starts with `%PDF-`. If it does not, the error quotes the tail of the converter's stderr.
- **Where a program is found.** Every external program is resolved the same way before it runs (`engine::deps`): first a copy **bundled next to Pluryprint** (`resources/<subdir>` beside the executable, `Resources/<subdir>` in a macOS `.app`, or the executable's own folder), then **`PATH`**, then the vendor's **well-known install folders** listed per format below. A bundled copy always wins, because it is the version that was tested. There is no setting yet to point at a program by hand.
- **A missing program pauses the job.** The converter is located before anything is written. If it is not found, the result is `DependencyMissing` with the FR-058 remediation text, and the job is **paused**, not failed. Once the program is installed (by hand, or from the Print Job screen's **Install…** button, see [Dependency detection](#dependency-detection)), resuming the job is enough.
- **Pages appear after conversion.** A document's page count is unknown when it is added (the Print Job row shows no count). The conversion task measures the converted PDF, and the job, the queue views and Home show that number.
- **Page ranges.** A range applies to the pages of the converted PDF for PDFs and documents. Images, HEIC and SVG are one page, so the engine drops the range for them (`engine::print::effective_settings`). Before submitting, the range is checked only against PDFs whose page count is known.

## How a rejected file is reported

Nothing is dropped silently (FR-005/FR-059). Folder expansion returns every discarded entry as a `SkippedFile` carrying a `SkipReason` (`UnsupportedFormat`, `TooLarge` or `Unreadable`). Each entry also has a human-readable `detail`, such as `"Unsupported file type (.exe)"`, ready for the job log, and the UI translates the reason. A supported format whose converter is missing is **not** rejected. It is accepted, and the Print Job screen shows the remediation, with an install offer, for the dependencies that the selected files need.

A **GIF** is refused by name (decision D9): the drop-time inspection and the folder walk report `gif_unsupported` ("GIF images are not supported — convert them to PNG or JPEG first"), a submission that still carries one is refused with the IPC reason `gif_not_supported` naming the file, and `pluryprint print` refuses it with the same sentence (exit 7).

## Per-format details

### PDF

No conversion: the final printing engine is called directly. `convert_to_pdf` validates the `%PDF-` header, looks for the `%%EOF` marker in the last 8 KiB and, for files up to 100 MB that parse, requires at least one page. A mislabelled, truncated or empty file is rejected before it reaches the spooler.

```powershell
# Windows: the real argument vector (engine::print::sumatra). A4 is paperkind 9.
SumatraPDF.exe -print-to "PrinterName" -print-settings "1-5,duplex,color,fit,paperkind=9" -silent -exit-when-done file.pdf
```

```bash
# macOS: the real argument vector (engine::print::cups). `lp`, not `lpr`.
lp -d PrinterName -t "job title" -o sides=two-sided-long-edge -o media=A4 -o fit-to-page file.pdf
```

**Encrypted PDFs (FR-059, `engine::convert::pdf_security`).** Before printing, the trailer is checked for `/Encrypt` without parsing a plain file. An encrypted file is then opened by `lopdf`, which tries the empty user password on its own:

| What the file is | What happens |
|---|---|
| Owner password only, printing allowed | Printed in place, no question asked. |
| Needs the user's password | The job **pauses** with `password_required`, and its Queue row offers **Enter password** instead of Resume. The dialog sends the password with `job_supply_password` (optionally for the batch's other protected PDFs), clears the field at once and follows the job. A wrong password brings the job back paused with `password_incorrect`. **Not now** leaves it waiting; **Cancel this job** cancels it. The CLI takes the password from `--password-file <path>` or `--password-stdin`, never as an argument. |
| Printing not allowed by its permissions | Refused, whatever the password (`printing_not_allowed`, decision D12). The permission is honoured, never stripped. SumatraPDF 3.5.2 was seen printing such a file anyway, so this check in Pluryprint is the guard. |
| A security handler `lopdf` cannot read (public key, unknown revision) | Refused as `unsupported_encryption`. |

With the password, `lopdf` decrypts the file **in-process** into the job's cache PDF, which is deleted with the job, and the copy is checked to read back with every page. The password never reaches a command line, an environment block or a log. It is held in a buffer that is zeroed when dropped and is not saved anywhere: a new job asks again. RC4, AES-128 and AES-256 files are covered by tests (`tests/fixtures/pdf_enc`).

**Other edge cases:**
- Corrupt or mislabelled PDFs: rejected with `ConversionFailed("not a valid PDF…")` before spooling (no `%PDF-` header, no `%%EOF` marker, or no pages). In the CLI that file fails with exit 8 and the rest of the batch still prints.
- Very large PDFs: no chunked spooling; we rely on the OS spooler.

### Images (JPG, PNG, TIFF, BMP, WebP)

These are converted in-process. `engine::convert::image::to_pdf` decodes the image with the pure-Rust `image` crate (built with exactly the `jpeg`, `png`, `tiff`, `bmp` and `webp` codecs). An image with an alpha channel is composited onto white, as on paper, so a transparent logo does not print as a black box. The pixels are then embedded in a one-page PDF, built with `lopdf`, at `cache_dir/<job_id>.pdf`. Progress is reported as `0.0..=1.0`. EXIF orientation is not applied, and only the first frame of a multi-frame TIFF is used.

### GIF (not supported)

GIF is **refused** (decision D9): the `image` crate's `gif` codec is not built in, and the refusal names GIF and suggests converting to PNG or JPEG (see [How a rejected file is reported](#how-a-rejected-file-is-reported)).

### HEIC / HEIF

The `image` crate has no HEIF decoder. Bundling `libheif` would bring a native build and HEVC patent questions into the app. Both target OSes already ship a decoder, so the app uses it (`engine::convert::heif`):

- **macOS:** `sips -s format png <in> --out <scratch>/heif.png` (`/usr/bin/sips`, part of the base system), followed by the image path above.
- **Windows:** Windows Imaging Component (`IWICImagingFactory` → frame 0 → `IWICFormatConverter` to 24-bit RGB) feeds the same one-page PDF builder. WIC can decode HEIF only when the Microsoft Store **HEIF Image Extensions** (and **HEVC Video Extensions** for `.heic`) are installed. Both are dependencies of their own in the report: the app asks WIC for a HEIF decoder and Media Foundation for an HEVC decoder (`MFTEnumEx`, Store MFTs included), and a probe that fails is read as "installed" so it never raises a false alarm. Without the HEIF extension the job pauses with `heif-codec`; with HEIF but no HEVC decoder it pauses with `hevc-codec`. A decode that fails while both are present is a damaged file (`heif_decode_failed`). The decode runs in-process and cannot be killed, so it runs on its own thread; cancelling the job or reaching the conversion timeout abandons that thread and ends the conversion at once (`ConversionTimedOut`, terminal).

### SVG / SVGZ

`engine::convert::svg` (FR-052a) parses the drawing with `usvg` and writes it with `svg2pdf` (both MIT OR Apache-2.0): shapes, gradients and text stay **vector**, only filter effects are rasterised. The page is the drawing's own size at 96 dpi, clamped to 200 in, and fitting it to the paper is left to the printer's scaling flags, as for an image.

- **Isolation.** A hostile drawing can make a parser panic, and the release build aborts on panic, so the conversion runs in `pluryprint --worker svg <in> <out>`, bounded by the conversion timeout and the cancel flag. The worker's failure comes back as `svg_invalid`, `svg_too_large` or `svg_convert_failed`.
- **Nothing is fetched.** `usvg` has no network code, ignores `<script>`, `<foreignObject>` and animation, and its XML parser has no external entities (no XXE). An `<image>` loads only a file inside the SVG's own folder, symlinks resolved: never a UNC share (which would hand the user's NTLM credentials to that host over SMB), a `file://host/` URL, a `../` outside the folder or a URL.
- **SVGZ** is inflated by Pluryprint with a 64 MB cap. Text uses the system fonts, loaded once and only when the parsed drawing has text or embeds an image; no font is ever loaded from a URL or a path in the drawing.
- **Resource caps.** The worker runs under a 1 GiB memory cap (a Job Object on Windows; `RLIMIT_DATA` and a memory watchdog on macOS/Linux) and the conversion timeout. A drawing that needs more memory fails with `svg_too_large`, one that needs more time with `tool_timed_out`; both are terminal. Before that, a drawing that would expand past the work budget (nested `<use>`, markers, patterns, or masks, clip paths and filters re-built for every element that uses them), a raster `<image>` over 64 Mpx, filter effects over 64 Mpx in all, more than 256 MB of images or a PDF over 512 MB is refused with `svg_too_large` at once. An SVG used as an `<image>` goes through the same caps.

### Office (DOCX, XLSX, PPTX, DOC, XLS, PPT), OpenDocument (ODT, ODS, ODP), and RTF on Windows

`engine::convert::office` copies the file into the scratch folder as `input.<ext>` and runs:

```bash
soffice -env:UserInstallation=file:///<scratch>/profile --headless --invisible --nologo \
        --nodefault --nolockcheck --norestore \
        --convert-to pdf --outdir <scratch>/out <scratch>/input.<ext>
```

- **Private profile per conversion.** LibreOffice allows one process per user profile. Without `-env:UserInstallation`, a headless convert hands the file to an open LibreOffice window, or blocks on its lock. A throwaway profile isolates every conversion, including parallel ones. The cost is LibreOffice's first-start time, a few seconds per file.
- **Hardened profile.** Before LibreOffice starts, the profile is seeded with `registrymodifications.xcu` (`office::HARDENING_XCU`): links from untrusted documents are blocked (`BlockUntrustedRefererLinks`), Writer and Calc never update links on load, macros and active content (OLE, DDE) are off, and LibreOffice's own HTTP(S) goes to a dead proxy on loopback. A document someone sent the user therefore cannot make the conversion fetch a picture at `http://…` (telling the sender when it was printed) or at `\\host\share\…` (handing that host the user's NTLM credentials). `fr_053_real_libreoffice_fetches_no_linked_resource` checks it against a real LibreOffice.
- **Fidelity.** Complex Word layouts and Office 365–specific features can render differently than in Microsoft Word. A spreadsheet prints every sheet that has content.

Besides a bundled copy and `PATH`, LibreOffice is looked for in `/Applications/LibreOffice.app/Contents/MacOS/` on macOS and in `%ProgramFiles%\LibreOffice\program\` (then the x86 Program Files) on Windows.

### HTML

`engine::convert::html` (FR-055) never hands the user's file to a renderer. It writes a **staged copy** into the scratch folder whose head starts with:

- a Content-Security-Policy that forbids scripts, plugins, frames and connections;
- a `<base href>` at the document's own folder, so relative stylesheets and images resolve as they do next to the file;
- the job's paper as CSS `@page { size: … }` (landscape swaps the sides). The paper is also set natively on the print settings, so it does not depend on the page's CSS. Paper `auto` keeps the web view's default.

The staged page is printed by the operating system's own web view in `pluryprint --worker render-pdf` (`crate::render`, decision D15). Nothing has to be installed: Pluryprint needs WebView2 to run at all, and WebKit is part of macOS.

- **Windows:** a hidden window with a private WebView2 profile (its `Local State` is stamped first, so WebView2 skips Chromium's blank-password logon probe), scripts, dialogs and dev tools off, and `PrintToPdf` on the job's paper with no header or footer. The page's folder is served from a private `https` origin rather than `file:`, so Chromium refuses every `file:` sub-resource, a protocol-relative `//host/x` reference becomes `https:` and is refused by the request filter, and any navigation other than the page is cancelled. `file://host/share/…` is therefore never opened over SMB or WebDAV.
- **macOS:** a bare AppKit process with no Dock icon, a `WKWebView` with content JavaScript off, a non-persistent data store and a content rule list that blocks everything but host-less `file:` and `data:`. WebKit may read the document's folder only. `NSPrintOperation` writes the paginated PDF; a size watchdog stops a page that keeps producing pages.
- **No network, no script.** No request leaves for the network or a file share, on either OS. A printed file cannot report back to whoever sent it (IP address, time of printing, a tracking pixel). Local files next to the page still load, so a saved page prints the way it looks offline. Only local files reach this converter; remote `http(s)://` inputs are not accepted. `@media print` styles are respected. A page that draws its content with script prints what it has without script.

`tests/render_worker.rs` drives the built binary: pagination, A4 and landscape Letter, relative images, a hostile page that must not reach a local listener, Markdown, UTF-16 text, RTF and two workers in parallel. `PLURYPRINT_RENDER_TRACE=1` writes each stage of the worker to the job's `render.log`.

**Browser fallback (hidden, D15).** `[engine] html_renderer = "browser"` in `config.toml`, or `PLURYPRINT_HTML_RENDERER=browser`, sends the staged page to headless Microsoft Edge, Google Chrome or Chromium instead, the route used before round 3 (`--headless --no-pdf-header-footer --user-data-dir=<scratch>/browser … --print-to-pdf`, a dead proxy and no name resolution, the CSP above, and a seeded profile). The Settings screen does not show this switch, and it is kept for one version only. Because a browser loads the page from `file:`, it cannot keep a `file://host/…` sub-resource off a file share; that is one more reason it is only a fallback.

### Markdown

`engine::convert::markdown` (FR-056) turns the file into HTML in-process with `pulldown-cmark`, then hands it to the HTML renderer above. No Pandoc, no browser, no LaTeX. GitHub-flavoured extensions are on: tables, footnotes, strikethrough, task lists and smart punctuation. A small print stylesheet makes it look like a document.

A Markdown file is text, and printing it must not make it active content:

- **Raw HTML is dropped.** A `<script>`, an `<iframe>` or an `<img src=…>` written in the Markdown never reaches the page; the text between inline tags stays.
- **Only local images load.** A relative path, a drive path, a host-less `file:///` URL or a `data:` URI is kept. A remote, protocol-relative or UNC image (`http://…`, `//host/…`, `\\host\…`, `file://host/…`) is replaced by its alt text, and links to shares lose their target.
- The page then gets the same staging (CSP, `<base href>` at the Markdown file's folder, the job's paper) and the same renderer, scripts off and no network.

### Plain text

`engine::convert::text` (FR-057) decodes the file without LibreOffice: a byte-order mark first (UTF-8, UTF-16 LE/BE), then strict UTF-8, else Windows-1252, which is what a Western Windows machine writes when it does not write UTF-8. The text is HTML-escaped into a `<pre>` page in a monospaced font that wraps long lines, laid out on the job's paper by the HTML renderer. A binary file is refused as `text_unreadable`.

### RTF

On **macOS** RTF needs no LibreOffice: `/usr/bin/textutil -convert html` (part of macOS) turns a private copy into HTML, keeping fonts, styles, colours, lists and tables, and that HTML goes through the HTML renderer with the work folder as its base, so a picture `textutil` wrote next to the page resolves and nothing outside it does. On **Windows** RTF goes through LibreOffice, as above.

## Out of scope (deferred to v2+ or never)

| Format | Why |
|---|---|
| GIF | Refused by decision D9, with its own message (see above) |
| AutoCAD DWG/DXF | Requires LibreCAD or proprietary ODA SDK; small audience for batch printing |
| Visio (.vsdx) | LibreOffice support is fragile; rarely batch-printed |
| EPUB | Niche use case; better with dedicated reader |
| Email (.eml, .msg, .pst) | Email rendering is non-trivial; users typically use Outlook |
| PostScript (.ps) | Edge case; Ghostscript dependency would inflate installer |
| Zebra ZPL barcodes | Highly specialized; would be a v2 plugin if demand emerges |
| Plaintext code with syntax highlighting (Pretty Print) | Could be a v2 feature; PrintFile covered this niche |

## Adding a new format

Each converter is a module under `src-tauri/src/engine/convert/`. To add a format:

1. Add a converter module, or extend an existing routing table (`IMAGE_EXTS`, `OFFICE_EXTS`, `HTML_EXTS`, `MARKDOWN_EXTS`, `TEXT_EXTS`, `SVG_EXTS`, `HEIF_EXTS`).
2. Route it in `convert::route` / `convert_to_pdf`. If it shells out, use `process::run_bounded` and a `WorkDir`. A parser that could panic on hostile input belongs in a worker (`crate::worker`), like SVG.
3. **Add the extension to `engine::SUPPORTED_EXTS`** and to `src/lib/formats.ts`. The route test and the Home test fail if either is missing.
4. If it needs a new external program, add a `Dependency` variant in `engine/deps.rs`. List the extensions it gates in that spec's `blocks`, write the remediation text (and its EN/ES `common:deps.<id>` sentence), and add install recipes if there is a consented way to install it.
5. Add tests: argument-vector unit tests, fake-binary tests (`external_tests::fakes`), and a real-tool test that generates its fixture in-test and skips when the tool is absent.
6. Update this document and the FR row.

A future plugin API (v2) may allow third-party converters as separate crates loaded at runtime.

## Dependency detection

`engine::deps` resolves external programs and codecs:

| Dependency | Executable names / probe | Blocks (extensions) while missing | Install offered (after consent) |
|---|---|---|---|
| SumatraPDF | `SumatraPDF.exe` (bundled by the installer) | On Windows, **every** supported format, because it is the final printer. Never reported off Windows, where CUPS `lp` prints. | `winget install SumatraPDF.SumatraPDF`, or the download page |
| LibreOffice | `soffice` / `soffice.exe` | Windows: `docx doc xlsx xls pptx ppt odt ods odp rtf`. macOS: the same without `rtf` | Windows: `winget` (its MSI asks for administrator approval, which the dialog says). macOS: `brew install --cask libreoffice`. Always the download page |
| HEIF Image Extensions (Windows) | asked of WIC | `heic heif` | `winget` from the `msstore` source (free), the Store page, or the web page |
| HEVC Video Extensions (Windows) | asked of Media Foundation | `heic` | the Store page only: it may be paid, and winget cannot buy |
| Edge / Chrome / Chromium | `msedge.exe`, `chrome.exe` (Win) · `Microsoft Edge`, `Google Chrome`, `Chromium` (macOS) | nothing: only the hidden `html_renderer = "browser"` fallback uses it | the Edge download page |
| Pandoc | — | nothing: no longer used, kept so an old `queue.json` still loads | — |

HEIC needs no probe on macOS (`sips` ships with the OS), and neither does RTF (`textutil`).

The search order is the same for every program: **bundled** next to the executable, then a **configured** path (no setting exists yet), then **`PATH`**, then **well-known install directories**. `resolve` is a pure function, so the Windows search order can be unit-tested from a macOS machine. `FoundIn` records which step found the program, so diagnostics can show which copy was picked.

Each missing dependency's remediation names it, says what cannot print, and links to where to get it. For example: *"LibreOffice is missing — Office, OpenDocument and RTF files cannot be printed until it is installed. Download it free from https://www.libreoffice.org/download"*. The UI shows that sentence in the user's language (`common:deps.<id>.remediation`), and the paused job and the Print Job screen give the same explanation.

**Guided install (decision D13).** The report lists how each missing dependency can be installed, best first. The Print Job screen's **Install…** opens a consent dialog with every way, the exact command or address, whether Windows will ask for administrator approval, the price and the size; a way whose tool (winget, Homebrew) is not present is disabled and the download page preselected. Nothing runs until the user presses **Install** (`install_dependency` refuses without `consent: true`). winget and brew run with an argument vector, no shell and no console window, one install at a time, bounded by 30 minutes, with their output streamed into the dialog. The dependency is probed again afterwards and the fresh report replaces the panel's row. Store and web options open with the OS handler.

## Tests

- `engine::convert::process::tests`: zero and non-zero exits, timeout kill (including a background helper in the same process group), cancel kill, a child that never exits after its output is finished, a missing program reported as `DependencyMissing`, and no shell interpolation.
- `engine::convert::external_tests::fakes` (Unix): `/bin/sh` fake `soffice`, browser, render worker and `sips` binaries. They pin the argument vectors that are actually spawned, the "exit 0 with no output" failure, the timeout kill, the browser profile seeding and the scratch-folder cleanup, and that HTML and Markdown go to the worker, not to a browser or Pandoc.
- `engine::convert::external_tests::fr_05x_real_*`: real conversions of fixtures generated in-test (an `.rtf`, a `.docx` that LibreOffice creates, an HTML page through the browser fallback, and on macOS a `.heic` that `sips` creates from a PNG), plus a LibreOffice document and an HTML page that must not reach a local listener. Each test **skips with a note** when its tool is not installed, so a CI runner without LibreOffice stays green while a developer machine runs the real conversion.
- `tests/render_worker.rs` and `tests/svg_worker.rs`: the real workers of the built binary.
- `engine::convert::pdf_security` tests and the queue's `needs_password` tests: every encryption kind, owner-only and no-print files, and the pause / wrong password / refusal paths.
