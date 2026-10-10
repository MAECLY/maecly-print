# ADR-0012: The system web view renders HTML, Markdown and text, in a worker process

## Status

Accepted

## Date

2026-10-10

## Context

[ADR-0003](./0003-hybrid-print-engine-strategy.md) chose a hybrid engine: each format is converted to PDF by the best tool for it, then one backend per OS prints the PDF. That part stands. Its converter table, though, named a headless Chromium-class browser for HTML and Pandoc for Markdown, and the first implementation added LibreOffice for plain text. By round 3 (decisions D10 and D15) that choice had three costs:

- **Two or three programs to install.** Pandoc is rarely present, and Edge/Chrome is not guaranteed on macOS. A Markdown file needed both. Every one of them is a paused job and a remediation message for the user.
- **A security gap that could not be closed from outside the browser.** A browser loads the page from `file:`. On Windows a `file://host/share/x.png` or `//host/x` sub-resource makes Windows open `\\host\share` over SMB or WebDAV and hand that host the user's NTLM credentials. Proxy switches cover `http(s)`, not `file:`.
- **Account lockouts.** Headless Chromium runs a blank-password logon probe on Windows that records a failed logon per run; ten in ten minutes lock the account under the Windows 11 default policy. A seeded profile avoids it, but every new browser build is a new chance to regress.

Meanwhile Pluryprint already depends on a web engine on every machine it runs on: WebView2 on Windows (Tauri needs it to start) and WebKit on macOS. Separately, some converters parse untrusted input with libraries that can panic, and the release build aborts on panic, so a hostile file could take the whole app down (D10).

## Decision

We will print HTML, Markdown and plain text through the **operating system's own web view**, in a **hidden worker process** of the Pluryprint executable, and run the other converters that parse untrusted input in-process only when they cannot crash the app:

- `pluryprint --worker render-pdf <request>` prints a staged HTML page to PDF: WebView2 `PrintToPdf` in a hidden window on Windows, `WKWebView` with `NSPrintOperation` in a bare AppKit process on macOS. The page's paper is set natively.
- The engine never hands the worker the user's file. It stages a copy with a Content-Security-Policy that forbids scripts, plugins, frames and connections, a `<base href>` at the document's folder and the job's paper. The worker turns scripts off with the web view's own switch, refuses every request that is not the page's folder or a `data:` URI, and cancels every navigation away from the page. On Windows the folder is served from a private `https` origin, so no `file:` sub-resource and no file share is ever opened.
- **Markdown** is converted to HTML in-process with `pulldown-cmark` (raw HTML dropped, only local images kept). **Text** is decoded in-process (BOM, UTF-8, else Windows-1252) into a `<pre>` page. **RTF on macOS** goes through `textutil`, part of the OS. All three then use the worker. Office, OpenDocument and RTF on Windows stay with LibreOffice, whose private profile is hardened against linked resources.
- **SVG** is drawn in-process by `usvg` + `svg2pdf`, but in its own worker (`pluryprint --worker svg`), for the panic reason above.
- Every worker is bounded by the conversion timeout and the job's cancel flag like any converter, and a failure keeps the worker's stderr as the technical detail.
- The headless browser stays as a **hidden fallback** for one version: `[engine] html_renderer = "browser"` in `config.toml`, or `PLURYPRINT_HTML_RENDERER=browser`. The Settings screen does not show it.

## Alternatives Considered

| Option | Why rejected |
|---|---|
| Keep Edge/Chrome and Pandoc | Two optional installs for two formats, the `file:` share leak, and the lockout probe to keep suppressed. |
| Render in the app's own WebView2/WKWebView window | A page that hangs or crashes the web view would take the app's UI with it; the worker contains it. |
| Bundle a Chromium build | Over 100 MB against the 15 MB installer budget (NFR-006), and a browser to keep patched. |
| Typeset Markdown with Pandoc's PDF output | Needs a LaTeX distribution, several gigabytes. |
| Run `usvg` on a thread in the app | A panic aborts the release build; only a process boundary contains it. |

## Consequences

### Positive

- HTML, Markdown, text, SVG and RTF on macOS print with nothing to install; the only optional programs left are LibreOffice and, on Windows, the HEIF/HEVC Store codecs.
- No request from a printed page reaches the network or a file share, on either OS, including the SMB case a browser could not close.
- A hostile page or drawing can kill a worker, not Pluryprint.

### Negative

- Two rendering engines to know instead of one: WebView2 and WebKit paginate slightly differently, so the same HTML can break pages in different places on Windows and macOS.
- Scripts never run, so a page that draws its content with JavaScript prints what it has without it.
- The console CLI has to find the app executable next to it to start the worker.

### Risks / Open Questions

- WebView2 and WebKit updates arrive with the OS, not with Pluryprint, so a rendering change can appear without a Pluryprint release. `tests/render_worker.rs` exercises the real worker on each platform.
- The browser fallback is removed after one version; anyone who needed it has to report why before then.

## Related

- ADR-0003 — the hybrid strategy this keeps, and whose HTML/Markdown converter choice this replaces.
- [`../04-architecture/print-engine.md`](../04-architecture/print-engine.md) §3.4–3.5 and [`../03-requirements/supported-formats.md`](../03-requirements/supported-formats.md) — the implementation.
