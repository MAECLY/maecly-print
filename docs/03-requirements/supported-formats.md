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
| `.pdf` | none: passthrough after a structural check | — | SumatraPDF / `lp` | Excellent (native) |
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
- **Where a program is found.** Every external program is resolved the same way before it runs (`engine::deps`): first a copy **bundled next to MaeclyPrint** (`resources/<subdir>` beside the executable, `Resources/<subdir>` in a macOS `.app`, or the executable's own folder), then **`PATH`**, then the vendor's **well-known install folders** listed per format below. A bundled copy always wins, because it is the version that was tested. There is no setting yet to point at a program by hand.
- **A missing program pauses the job.** The converter is located before anything is written. If it is not found, the result is `DependencyMissing` with the FR-058 remediation text, and the job is **paused**, not failed. Once the user has installed the program, resuming the job is enough.
- **Pages appear after conversion.** A document's page count is unknown when it is added (the Print Job row shows no count). The conversion task measures the converted PDF, and the job, the queue views and Home show that number.
- **Page ranges.** A range applies to the pages of the converted PDF for PDFs and documents. Images and HEIC are one page, so the engine drops the range for them (`engine::print::effective_settings`). Before submitting, the range is checked only against PDFs whose page count is known.

## How a rejected file is reported

Nothing is dropped silently (FR-005/FR-059). Folder expansion returns every discarded entry as a `SkippedFile` carrying a `SkipReason` (`UnsupportedFormat`, `TooLarge` or `Unreadable`). Each entry also has a human-readable `detail`, such as `"Unsupported file type (.exe)"`, ready for the job log and the UI. A supported format whose converter is missing is **not** rejected. It is accepted, and the Print Job screen shows the remediation for the dependencies that the selected files need.

## Per-format details

### PDF

No conversion: the final printing engine is called directly. `convert_to_pdf` validates the `%PDF-` header, looks for the `%%EOF` marker in the last 8 KiB and, for files up to 100 MB that parse, requires at least one page. Then it returns `Ok(None)`, meaning "print the original in place". A mislabelled, truncated or empty file is rejected before it reaches the spooler.

```powershell
# Windows: the real argument vector (engine::print::sumatra)
SumatraPDF.exe -print-to "PrinterName" -print-settings "1-5,duplex,color,fit,paper=A4" -silent -exit-when-done file.pdf
```

```bash
# macOS: the real argument vector (engine::print::cups). `lp`, not `lpr`.
lp -d PrinterName -t "job title" -o sides=two-sided-long-edge -o media=A4 -o fit-to-page file.pdf
```

**Edge cases:**
- Corrupt or mislabelled PDFs: rejected with `ConversionFailed("not a valid PDF…")` before spooling (no `%PDF-` header, no `%%EOF` marker, or no pages). In the CLI that file fails with exit 8 and the rest of the batch still prints.
- Very large PDFs: no chunked spooling; we rely on the OS spooler.
- Encrypted PDFs: *planned*. The app would prompt for a password (CLI: `--password <pwd>`). There is no `--password` flag today, so an encrypted PDF fails at the spooler.
- Permission-restricted PDFs (no printing flag): *planned*. The app would show an explicit error that suggests `qpdf --decrypt`.

### Images (JPG, PNG, TIFF, BMP, WebP)

These are converted in-process. `engine::convert::image::to_pdf` decodes the image with the pure-Rust `image` crate (built with exactly the `jpeg`, `png`, `tiff`, `bmp` and `webp` codecs). It then embeds the pixels in a one-page PDF, built with `lopdf`, at `cache_dir/<job_id>.pdf`. Progress is reported as `0.0..=1.0`. EXIF orientation is not applied, and only the first frame of a multi-frame TIFF is used.

### HEIC / HEIF

The `image` crate has no HEIF decoder. Bundling `libheif` would bring a native build and HEVC patent questions into the app. Both target OSes already ship a decoder, so the app uses it (`engine::convert::heif`):

- **macOS:** `sips -s format png <in> --out <scratch>/heif.png` (`/usr/bin/sips`, part of the base system), followed by the image path above.
- **Windows:** Windows Imaging Component (`IWICImagingFactory` → frame 0 → `IWICFormatConverter` to 24-bit RGB) feeds the same one-page PDF builder. WIC can decode HEIF only when the Microsoft Store **HEIF Image Extensions** (and **HEVC Video Extensions** for `.heic`) are installed. Without the HEIF extension, WIC reports that no decoder exists, or on Windows 11 that its built-in HEIF decoder stub cannot initialise (`WINCODEC_ERR_COMPONENTINITIALIZEFAILURE`). Either way the job pauses with a `DependencyMissing` message that names both extensions. With the HEIF extension but no HEVC decoder, WIC does find a decoder and then fails on the pixels with some other error. In that case MaeclyPrint asks Media Foundation whether any HEVC video decoder is registered (`MFTEnumEx`, Store MFTs included). If none is, the job pauses with a `DependencyMissing` message that names **HEVC Video Extensions**, instead of retrying a generic conversion failure. A decode that fails while an HEVC decoder exists is a damaged file (`ConversionFailed`). The decode runs in-process and cannot be killed, so it runs on its own thread. Cancelling the job or reaching the conversion timeout abandons that thread and ends the conversion at once, like the converters that run as processes.
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

Besides a bundled copy and `PATH`, LibreOffice is looked for in `/Applications/LibreOffice.app/Contents/MacOS/` on macOS and in `%ProgramFiles%\LibreOffice\program\` (then the x86 Program Files) on Windows.

### HTML

`engine::convert::html` prints the file from its own location, so relative CSS and images resolve the way they do in a browser:

```bash
<browser> --headless --disable-gpu --no-pdf-header-footer --print-to-pdf-no-header \
          --user-data-dir=<scratch>/browser --no-first-run --no-default-browser-check \
          --disable-extensions --disable-background-networking --disable-sync --mute-audio \
          --proxy-server=127.0.0.1:9 --proxy-bypass-list='<-loopback>' \
          --host-resolver-rules='MAP * ~NOTFOUND' \
          --force-webrtc-ip-handling-policy=disable_non_proxied_udp --disable-quic \
          --disable-remote-fonts --disable-features=NetworkPrediction \
          --print-to-pdf=<scratch>/out.pdf file:///path/to/page.html
```

- **Browser.** On Windows, Edge comes first (it is always installed), then Chrome, in their Program Files folders. On macOS the order is Edge, Chrome, then Chromium in `/Applications`. On Linux it is `google-chrome`, `chromium`, `chromium-browser`, then `microsoft-edge`. A bundled copy and `PATH` are searched before those folders, as for every converter.
- **Private browser profile.** The user's open browser and its profile are never touched.
- **The finished PDF ends the step.** Some Chrome builds (seen with Chrome 156 on macOS) write the PDF in about a second and then never exit. The step therefore ends once the PDF has its `%%EOF` trailer and its size has stopped changing between polls. The browser tree is then killed.
- **The job's paper.** The browser lays pages out on US Letter unless the page sets a size, so an A4 job used to get Letter pages (clipped on the right at actual size). When the job names a paper (`A4`, `Letter`, `Legal`, `A3`, `A5`, `A6`, `B4`, `B5`, `Tabloid`, `Executive`, `4x6`-style inch sizes or CUPS `w<pt>h<pt>` keywords), the browser loads a copy of the page from the scratch folder that starts with `<style>@page { size: <w> <h>; }</style>` (landscape swaps the two) and, if the page has none, a `<base href>` to its own folder so relative links still resolve. The user's file is never changed, and a size the page sets itself still wins. Paper `auto` (printer default) and sizes MaeclyPrint cannot measure keep the browser default. A UTF-16 page is printed as is.
- **No network.** A printed file must not report back to whoever sent it (IP address, when it was opened, a tracking pixel). Every `http(s)`/`ws` request goes to a proxy on a closed local port, and that includes `localhost`, so local services cannot be reached either. Host names do not resolve, WebRTC cannot send UDP around the proxy, and web fonts are not downloaded. Local files such as stylesheets and images next to the page still load, so a saved page prints the way it looks offline. Only local files reach this converter; remote `http(s)://` inputs are not accepted. `@media print` styles are respected.
- **Scripts run** as they would in the browser. Chrome's only per-run switch to disable them (`--blink-settings=scriptEnabled=false`) makes headless `--print-to-pdf` hang without output (Chrome 154), and a page that draws its content with script would otherwise print blank. Without a network a script has nowhere to send what it sees, and Chrome's file-origin rules stop a `file://` page from reading other local files.
- **Known gap (Windows):** an HTML file that references a UNC path (`<img src="\\host\share\x.png">` or `file://host/…`) makes Windows contact that host over SMB. The proxy does not cover `file://`, and a page's own markup is printed as written. Markdown is protected (see below); for HTML, only print files you trust.

### Markdown

`engine::convert::markdown` uses Pandoc to produce HTML, then the HTML step above. Pandoc's own PDF output would need a LaTeX distribution, a multi-gigabyte install:

```bash
pandoc --from=markdown-raw_html-raw_attribute --to=html5 --standalone --sandbox \
       --lua-filter=<scratch>/local-images.lua --metadata=pagetitle:<name> \
       --include-in-header=<scratch>/head.html --output=<scratch>/page.html <input.md>
```

A Markdown file is text, and printing it must not make it active content:

- **Raw HTML is printed as text.** `<script>`, `<iframe>`, `<img src=…>` and `{=html}` blocks are not passed through to the page.
- **Only local images load.** The Lua filter keeps relative paths, drive paths, `file:///…` and `data:` images. A remote, protocol-relative or UNC image (`http://…`, `//host/…`, `\\host\…`, `file://host/…`) is replaced by its alt text. On Windows a UNC image would otherwise hand the user's NTLM credentials to that host over SMB.
- **`--sandbox`** limits Pandoc to the files named on its command line, with no network.
- **A Content-Security-Policy** in the injected header forbids scripts, plugins, frames and connections. The browser step then runs with no network, as above.

The injected header carries `<base href="file:///<folder of the .md>/">`, so relative image links resolve against the Markdown file's folder. It also carries a small print stylesheet (margins, readable type, code blocks, tables) and the job's paper size as an `@page` rule, under the same rules as HTML above. The Pandoc and browser steps share one timeout budget. Besides a bundled copy and `PATH`, Pandoc is looked for in `/opt/homebrew/bin` and `/usr/local/bin` on macOS, and in `%LOCALAPPDATA%\Pandoc` and `%ProgramFiles%\Pandoc` on Windows. TeX math is passed through as text.

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
