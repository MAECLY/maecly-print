# ADR-0012: The system web view renders HTML, Markdown and text, in a worker process

## Status

Accepted. Amended in round 6: the browser fallback is removed (see "Round 6 amendment").

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
- ~~The browser fallback is removed after one version.~~ Done in round 6 (below).

## Round 6 amendment: the network layer is the boundary, and the fallback is gone

**Context.** Security reviews in rounds 4 to 6 (notes 5-7, and the round-6 finding about character references inside an SVG `<style>`) kept finding parser differentials in the text pre-pass that protected the browser fallback: whatever `html_scan` decided about comments, encodings, `<style>` text or `<base>`, some page made the browser's own parser decide differently, and a share reference reached SMB. A text scan in front of a browser cannot be the security boundary.

**Decision.**

- The headless Edge/Chrome fallback (`[engine] html_renderer = "browser"`, `PLURYPRINT_HTML_RENDERER`) is **removed**, with its share-reference neutraliser and stylesheet walk. A stored `"browser"` is read and ignored; the app removes the key at the next launch and tells the user once (EN/ES) that pages now print with the system web view.
- The boundary is the **web view's network layer**, on the URL the web view itself resolved: on Windows a `WebResourceRequested` filter on `*` that answers every request (document folder from the private origin, `data:`/`blob:`, a logged 403 for the rest), cancelled navigations, frame navigations and new windows, and served files checked on their open handle; on macOS a `WKContentRuleList` that admits only the document folder's own `file:` URL, `data:` and `blob:`, plus a navigation delegate. Read access alone was shown to be insufficient on macOS (a picture in the folder's parent loaded).
- `html_scan` stays only as defence in depth (where the policy and `<base>` go) and decodes leniently: an unknown or empty charset no longer refuses the page.

**Consequences.** No headless browser is ever started; Edge/Chrome/Chromium is not used at all. A page with a broken charset declaration prints. `page_not_checkable` now means only that the policy could not be placed first (a doctype that never closes).

## Round 7 amendment: WebKit reads no file itself

**Context.** The round-6 macOS boundary was a `WKContentRuleList` whose only folder rule was a `file:` URL prefix, plus read access to the folder. A review found that a symbolic link inside the document's folder (`doc/link.png -> ../secret.png`; Archive Utility keeps links from zip files) has a URL under the prefix, and WebKit followed it on disk to any file the user can read, `/net/<host>/…` included (autofs then mounts an NFS export, reaching the network).

**Decision.** The macOS worker serves the page the way the Windows worker does. It registers a `WKURLSchemeHandler` for `pluryprint-doc` and loads the staged page from `pluryprint-doc://doc.pluryprint.example/`. Every request on that origin is mapped by the same pure `render::resolve` as on Windows. A file is opened by `render::open_in_folder`: component by component with `openat(O_NOFOLLOW)`, following a symbolic link by hand only while its target stays inside the folder (so nothing outside is ever looked up), and then its `F_GETPATH` must lie inside the folder; the bytes come from that descriptor. WebKit gets no `file:` URL and no read access, and the rule list now blocks every `file:` URL. The page's policy names the private origins and no longer `file:`.

**Consequences.** Links inside the folder that stay inside still load. A hard link to an outside file cannot be told apart from the file itself (as on Windows). WebKit still does not draw an external SVG `<use>` document on macOS.

## Related

- ADR-0003 — the hybrid strategy this keeps, and whose HTML/Markdown converter choice this replaces.
- [`../04-architecture/print-engine.md`](../04-architecture/print-engine.md) §3.4–3.5 and [`../03-requirements/supported-formats.md`](../03-requirements/supported-formats.md) — the implementation.
