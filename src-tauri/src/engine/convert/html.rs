//! HTML → PDF (FR-055), and the page renderer Markdown ([`super::markdown`])
//! and plain text ([`super::text`]) share.
//!
//! ## The system web view (default)
//!
//! The page is printed by the operating system's own web view — WebView2 on
//! Windows, WebKit on macOS — in a hidden worker process of this same
//! executable (`pluryprint --worker render-pdf …`, [`crate::worker::render`],
//! [`crate::render`]). Nothing has to be installed: Pluryprint needs WebView2
//! to run at all, and WebKit is part of macOS. Edge, Chrome and Pandoc are no
//! longer dependencies.
//!
//! - The engine never hands the worker the user's file. It writes a **staged
//!   copy** to the job's work directory ([`stage_page`]) carrying, at the start
//!   of its head, a Content-Security-Policy that forbids scripts, plugins,
//!   frames and connections ([`page_csp`]), a `<base href>` that points
//!   relative references at the document's folder ([`crate::render::base_href`]),
//!   and the job's paper as CSS `@page { size }`. The paper is also set
//!   natively on the print settings.
//! - The worker loads it with scripts off and **no network and no file
//!   share**: see [`crate::render`] (a security finding: a page could make
//!   Windows open `\\host\share` over SMB/WebDAV, handing the user's NTLM
//!   credentials to that host, through `file://host/…` or a protocol-relative
//!   `//host/…` reference).
//! - The worker is bounded by the conversion timeout and the job's cancel flag
//!   like any converter ([`super::process::run_until`]): a page that hangs or
//!   crashes the web view kills the worker, never the app.
//!
//! ## The browser fallback (hidden, one version, D15)
//!
//! With `[engine] html_renderer = "browser"` in `config.toml` (or
//! `PLURYPRINT_HTML_RENDERER=browser`), the staged page goes to a headless
//! Chromium-class browser instead — Microsoft Edge, Google Chrome or Chromium,
//! the pre-round-3 path:
//!
//! ```text
//! <browser> --headless --disable-gpu --no-pdf-header-footer
//!           --user-data-dir=<work>/browser --no-first-run ...
//!           --print-to-pdf=<work>/out.pdf file:///<work>/stage/page.html
//! ```
//!
//! - `--user-data-dir` points at a throwaway directory whose `Local State` is
//!   seeded first so Edge skips its blank-password logon probe, which otherwise
//!   records a failed Windows logon per conversion and locks the account after
//!   ten ([`crate::util::chromium`]).
//! - The step ends when the PDF is complete, not when the browser exits
//!   ([`pdf_is_complete`]): some Chrome builds write it and then idle forever.
//! - Every `http(s)`/`ws` request goes to a dead proxy on loopback and no host
//!   name resolves ([`SANDBOX_ARGS`]); the staged page's CSP forbids script.
//!   The browser loads the page from `file:`, so unlike the system renderer it
//!   cannot keep a `file://host/…` or `//host/…` sub-resource off a file share
//!   by itself. The page is therefore rewritten first ([`guard_browser_page`]):
//!   every reference the shared predicate ([`super::local_ref`]) calls a share
//!   is removed, and a local stylesheet that names one refuses the page
//!   (`share_reference_refused`). The check **fails closed**: the page is
//!   decoded by its real encoding first (a UTF-16 page used to pass unread)
//!   and staged as UTF-8 with a byte-order mark, so the browser parses the
//!   bytes that were checked; and a stylesheet that cannot be checked
//!   completely (too large, too many, an unreadable encoding or `@import`)
//!   refuses the page with `page_not_checkable` instead of loading unchecked.
//!   Still one more reason it is only a fallback: an external SVG `<use>`
//!   document's own references are not inspected.
//!
//! ## Both renderers
//!
//! The staged page is always UTF-8 with a byte-order mark
//! ([`super::html_scan::decode_page`]), and the policy must land as its very
//! first element: a page whose encoding cannot be read, or that leaves no
//! safe place for the policy (a doctype that never closes), is refused with
//! `page_not_checkable` ([`inject_head`]) rather than rendered without it.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use super::process::run_until;
use super::{file_url, PageSetup, WorkDir};
use crate::engine::{EngineError, Failure};
use crate::settings::HtmlRenderer;

/// Extensions rendered as HTML directly.
pub const HTML_EXTS: &[&str] = &["html", "htm"];

/// Environment variable that overrides the configured renderer
/// (`system` or `browser`), for diagnostics.
pub const RENDERER_ENV: &str = "PLURYPRINT_HTML_RENDERER";

/// Environment variable naming the executable that runs the render worker.
/// Unset in the shipped app (the worker is the app itself); tests point it at
/// the freshly built `pluryprint` binary.
pub const WORKER_EXE_ENV: &str = "PLURYPRINT_WORKER_EXE";

/// Label of the render worker in messages.
const RENDERER_LABEL: &str = "The page renderer";

/// Which renderer prints pages: [`RENDERER_ENV`] when set, else the hidden
/// `[engine] html_renderer` of `config.toml` (D15), else the system web view.
pub fn configured_renderer() -> HtmlRenderer {
    match std::env::var(RENDERER_ENV)
        .ok()
        .map(|v| v.trim().to_ascii_lowercase())
        .as_deref()
    {
        Some("browser") => return HtmlRenderer::Browser,
        Some("system") => return HtmlRenderer::System,
        _ => {}
    }
    // Unit tests never read the developer's own configuration.
    #[cfg(not(test))]
    {
        crate::settings::Config::load_or_default()
            .engine
            .html_renderer
    }
    #[cfg(test)]
    {
        HtmlRenderer::System
    }
}

/// Convert the HTML page at `input` into a PDF at `output`, laid out on the
/// job's paper. The route's entry point.
pub fn convert(
    input: &Path,
    output: &Path,
    work: &WorkDir,
    cancel: &AtomicBool,
    timeout: Option<Duration>,
    setup: &PageSetup,
) -> Result<(), EngineError> {
    let html = std::fs::read(input).map_err(|e| {
        EngineError::ConversionFailed(
            Failure::new(
                "cannot_read_input",
                format!("cannot read {}: {e}", input.display()),
            )
            .param("file", input.display().to_string()),
        )
    })?;
    let base_dir = input.parent().unwrap_or_else(|| Path::new("."));
    render_document(
        &html,
        base_dir,
        output,
        work,
        cancel,
        timeout,
        setup,
        configured_renderer(),
    )
}

/// Print the HTML document `html`, whose relative references resolve against
/// `base_dir`, into `output` with `renderer`. The shared back half of the
/// HTML, Markdown and text routes.
#[allow(clippy::too_many_arguments)]
pub fn render_document(
    html: &[u8],
    base_dir: &Path,
    output: &Path,
    work: &WorkDir,
    cancel: &AtomicBool,
    timeout: Option<Duration>,
    setup: &PageSetup,
    renderer: HtmlRenderer,
) -> Result<(), EngineError> {
    // One decoding for every check and for the renderer (the staged page is
    // UTF-8 with a byte-order mark, which the renderer has to obey).
    let html = super::html_scan::decode_page(html)
        .ok_or_else(|| not_checkable("the page declares a text encoding that cannot be read"))?;
    let html = &html[..];
    match renderer {
        HtmlRenderer::System => render_system_with(
            &worker_exe()?,
            html,
            base_dir,
            output,
            work,
            cancel,
            timeout,
            setup,
        ),
        HtmlRenderer::Browser => {
            let mut base = file_url(base_dir);
            if !base.ends_with('/') {
                base.push('/');
            }
            let html = guard_browser_page(html, &base)?;
            let page = stage_page(&html, &base, setup, work)?;
            let browser = super::locate(crate::engine::deps::Dependency::Chromium)?;
            convert_with(&browser, &page, output, work, cancel, timeout)
        }
    }
}

/// The executable that runs the render worker: [`WORKER_EXE_ENV`], else this
/// program, except that the console CLI (`pluryprint-cli`) hands it to the
/// app next to it, which owns the worker branch.
pub fn worker_exe() -> Result<PathBuf, EngineError> {
    if let Some(exe) = std::env::var_os(WORKER_EXE_ENV).filter(|v| !v.is_empty()) {
        return Ok(PathBuf::from(exe));
    }
    let me = std::env::current_exe().map_err(|e| {
        EngineError::ConversionFailed(Failure::new(
            "render_failed",
            format!("cannot locate Pluryprint to start the page renderer: {e}"),
        ))
    })?;
    worker_exe_for(&me)
}

/// [`worker_exe`] for a given running executable. From the console CLI the
/// worker is the app next to it, which must be there: without it the job
/// fails with `render_worker_missing` (the app has to be reinstalled), not
/// with a dependency the remediation screen cannot name or install.
pub fn worker_exe_for(me: &Path) -> Result<PathBuf, EngineError> {
    let stem = me
        .file_stem()
        .map(|s| s.to_string_lossy().to_ascii_lowercase());
    if stem.as_deref() != Some("pluryprint-cli") {
        return Ok(me.to_path_buf());
    }
    let gui = if cfg!(windows) {
        "pluryprint.exe"
    } else {
        "pluryprint"
    };
    let sibling = me.with_file_name(gui);
    if sibling.is_file() {
        Ok(sibling)
    } else {
        Err(EngineError::ConversionFailed(Failure::new(
            "render_worker_missing",
            format!(
                "the page renderer is the Pluryprint app, which is not next to the CLI at {}",
                sibling.display()
            ),
        )))
    }
}

/// Paper width × height in inches for a CSS `size` value such as
/// `210mm 297mm`, `8.5in 11in` or `288pt 432pt`. `None` for anything else.
/// Pure.
pub fn paper_inches(css_size: &str) -> Option<(f64, f64)> {
    const UNITS: [(&str, f64); 4] = [
        ("mm", 1.0 / 25.4),
        ("cm", 1.0 / 2.54),
        ("in", 1.0),
        ("pt", 1.0 / 72.0),
    ];
    let mut parts = css_size.split_whitespace().map(|v| {
        let (number, factor) = UNITS
            .iter()
            .find_map(|(unit, factor)| v.strip_suffix(unit).map(|n| (n, *factor)))?;
        number
            .parse::<f64>()
            .ok()
            .filter(|n| n.is_finite() && *n > 0.0)
            .map(|n| n * factor)
    });
    let w = parts.next()??;
    let h = parts.next()??;
    parts.next().is_none().then_some((w, h))
}

/// Stage `html` and print it with the render worker run by `exe` (module
/// docs). [`render_document`] passes [`worker_exe`]; tests pass a fake.
#[allow(clippy::too_many_arguments)]
pub(crate) fn render_system_with(
    exe: &Path,
    html: &[u8],
    base_dir: &Path,
    output: &Path,
    work: &WorkDir,
    cancel: &AtomicBool,
    timeout: Option<Duration>,
    setup: &PageSetup,
) -> Result<(), EngineError> {
    let page = stage_page(html, &crate::render::base_href(base_dir), setup, work)?;
    let produced = work.path().join("out.pdf");
    let request = crate::render::RenderRequest {
        page,
        base_dir: base_dir.to_path_buf(),
        output: produced.clone(),
        data_dir: work.path().join("webview"),
        paper_in: setup.css_size.as_deref().and_then(paper_inches),
        // The worker's own watchdog: the same limit as the engine's, so a
        // worker left alone (a dead parent) still ends; none when the
        // setting is 0.
        timeout_secs: timeout.map(|t| t.as_secs().max(1)),
    };
    let mut cmd = Command::new(exe);
    cmd.arg(crate::worker::WORKER_FLAG)
        .arg(crate::worker::WorkerKind::RenderPdf.as_arg())
        .args(request.to_args())
        .current_dir(work.path());
    let log = work.path().join("render.log");
    match run_until(cmd, RENDERER_LABEL, cancel, timeout, &log, None) {
        Ok(()) => {}
        Err(EngineError::ConversionFailed(failure)) => {
            // The worker gave up on its own watchdog: a timeout, terminal
            // like the engine's own, never a render failure to retry.
            let full = std::fs::read_to_string(&log).unwrap_or_default();
            if full.contains(crate::render::TIMEOUT_MARKER) {
                return Err(super::process::timed_out_error(RENDERER_LABEL, timeout));
            }
            let detail = super::process::log_tail(&log, 400);
            tracing::warn!(event = "render_worker_failed", detail = %detail, error = %failure.detail);
            return Err(EngineError::ConversionFailed(Failure::new(
                "render_failed",
                if detail.is_empty() {
                    failure.detail
                } else {
                    detail
                },
            )));
        }
        Err(other) => return Err(other),
    }
    super::take_output(&produced, output, RENDERER_LABEL, &log)
}

/// The browser's argument vector for printing `page_url` to `output`. Pure.
///
/// Both spellings of "no header/footer" are passed: Chrome ≥ 112 understands
/// `--no-pdf-header-footer`, older builds `--print-to-pdf-no-header`, and each
/// ignores the flag it does not know.
///
/// [`SANDBOX_ARGS`] turn networking off; see the module docs.
pub fn browser_args(user_data_dir: &Path, output: &Path, page_url: &str) -> Vec<String> {
    let mut args = vec![
        "--headless".to_string(),
        "--disable-gpu".to_string(),
        "--no-pdf-header-footer".to_string(),
        "--print-to-pdf-no-header".to_string(),
        format!("--user-data-dir={}", user_data_dir.display()),
        "--no-first-run".to_string(),
        "--no-default-browser-check".to_string(),
        "--disable-extensions".to_string(),
        "--disable-background-networking".to_string(),
        "--disable-sync".to_string(),
        "--mute-audio".to_string(),
    ];
    args.extend(SANDBOX_ARGS.iter().map(|a| a.to_string()));
    args.push(format!("--print-to-pdf={}", output.display()));
    args.push(page_url.to_string());
    args
}

/// The flags that keep a printed page from reaching the network in the
/// browser fallback (module docs).
///
/// - All proxied traffic goes to the discard port on loopback, where nothing
///   listens, and `<-loopback>` removes Chrome's implicit "never proxy
///   localhost" rule so local services are refused the same way.
/// - `MAP * ~NOTFOUND` makes every host name fail to resolve, so not even a
///   DNS lookup leaks the page's links.
/// - WebRTC is the one API that can send UDP straight to an IP address
///   around a proxy; `disable_non_proxied_udp` closes that, and QUIC is off.
/// - Web fonts are a fetch the proxy would refuse anyway; turning them off
///   avoids waiting on them, as does turning off preconnect/prefetch.
pub const SANDBOX_ARGS: &[&str] = &[
    "--proxy-server=127.0.0.1:9",
    "--proxy-bypass-list=<-loopback>",
    "--host-resolver-rules=MAP * ~NOTFOUND",
    "--force-webrtc-ip-handling-policy=disable_non_proxied_udp",
    "--disable-quic",
    "--disable-remote-fonts",
    "--disable-features=NetworkPrediction",
];

/// Print the staged page `page` with the browser at `browser` into `output`
/// (the browser fallback, module docs).
pub fn convert_with(
    browser: &Path,
    page: &Path,
    output: &Path,
    work: &WorkDir,
    cancel: &AtomicBool,
    timeout: Option<Duration>,
) -> Result<(), EngineError> {
    let produced = work.path().join("out.pdf");
    let profile = work.path().join("browser");
    // Without this, every conversion records a failed Windows logon for the
    // user (Chromium's blank-password probe) and ten of them lock the account.
    if let Err(e) = crate::util::chromium::suppress_blank_password_probe(&profile) {
        tracing::warn!(event = "browser_logon_probe_suppress_failed", error = %e);
    }
    let mut cmd = Command::new(browser);
    cmd.args(browser_args(&profile, &produced, &file_url(page)))
        .current_dir(work.path());
    let log = work.path().join("browser.log");
    // Some Chrome builds write the PDF and then never exit; finish as soon as
    // the file is complete instead of waiting for an exit that may not come.
    let last_size = std::cell::Cell::new(None);
    let done = || pdf_is_complete(&produced, &last_size);
    run_until(cmd, "The browser", cancel, timeout, &log, Some(&done))?;
    super::take_output(&produced, output, "The browser", &log)
}

/// Most stylesheets [`guard_browser_page`] reads for one page.
const MAX_STYLESHEETS: usize = 64;

/// Largest stylesheet [`guard_browser_page`] reads.
const MAX_STYLESHEET_BYTES: u64 = 8 * 1024 * 1024;

/// The `page_not_checkable` refusal: the page, or something it loads, could
/// not be checked completely, so it is not printed (fail closed).
fn not_checkable(detail: impl Into<String>) -> EngineError {
    tracing::warn!(event = "html_page_not_checkable");
    EngineError::ConversionFailed(Failure::new("page_not_checkable", detail))
}

/// The `share_reference_refused` refusal for the stylesheet at `what`.
fn share_refused(what: &str) -> EngineError {
    tracing::warn!(event = "html_share_ref_in_stylesheet");
    EngineError::ConversionFailed(Failure::new(
        "share_reference_refused",
        format!("a stylesheet of the page refers to a network share: {what}"),
    ))
}

/// The bytes of a `data:` URL (`data:[<type>][;base64],<body>`), or `None`
/// when it is not one or its base64 is malformed. Pure.
fn data_url_bytes(url: &str) -> Option<Vec<u8>> {
    let rest = url.trim().get(5..).filter(|_| {
        url.trim()
            .get(..5)
            .is_some_and(|s| s.eq_ignore_ascii_case("data:"))
    })?;
    let (meta, body) = rest.split_once(',')?;
    let body = percent_encoding::percent_decode_str(body).collect::<Vec<u8>>();
    if !meta.to_ascii_lowercase().trim_end().ends_with(";base64") {
        return Some(body);
    }
    let mut out = Vec::with_capacity(body.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0u32);
    for b in body {
        let v = match b {
            b'A'..=b'Z' => b - b'A',
            b'a'..=b'z' => b - b'a' + 26,
            b'0'..=b'9' => b - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            b'=' | b' ' | b'\t' | b'\n' | b'\r' | b'\x0c' => continue,
            _ => return None,
        };
        acc = (acc << 6) | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Some(out)
}

/// Make `html` (as [`super::html_scan::decode_page`] returns it) safe for
/// the browser fallback, which loads the page from `file:` and so cannot keep
/// a share reference off SMB the way the system renderer does (module docs):
/// every share reference in the page is neutralised
/// ([`super::html_scan::neutralize_share_refs`]), and the local stylesheets
/// it loads (`<link>` and `@import`, recursively, `data:` ones included) are
/// decoded and read. One that names a share refuses the page with
/// `share_reference_refused`, since it cannot be rewritten in place.
///
/// **Fails closed.** Whatever cannot be checked completely refuses the page
/// with `page_not_checkable` instead of being loaded unchecked: a stylesheet
/// over [`MAX_STYLESHEET_BYTES`], more than [`MAX_STYLESHEETS`] of them, one
/// that cannot be read or decoded faithfully
/// ([`super::html_scan::decode_stylesheet`]), and an `@import` whose target
/// cannot be parsed. `base_url` is the document folder's `file:` URL.
pub fn guard_browser_page(html: &[u8], base_url: &str) -> Result<Vec<u8>, EngineError> {
    use super::{html_scan, local_ref};
    let html = html_scan::decode_page(html)
        .ok_or_else(|| not_checkable("the page declares a text encoding that cannot be read"))?;
    let neutralized = html_scan::neutralize_share_refs(&html);
    if neutralized.removed > 0 {
        tracing::info!(
            event = "html_share_refs_removed",
            count = neutralized.removed
        );
    }
    if html_scan::style_blocks(&neutralized.html)
        .iter()
        .any(|css| html_scan::has_unreadable_import(css))
    {
        return Err(not_checkable(
            "a style block imports a stylesheet whose name cannot be read",
        ));
    }
    let base = url::Url::parse(&effective_base(&neutralized.html, base_url))
        .map_err(|_| not_checkable("the page's folder has no usable address"))?;
    let base_dir = url::Url::parse(base_url)
        .ok()
        .and_then(|u| u.to_file_path().ok());
    let mut pending: Vec<(url::Url, String)> = html_scan::stylesheet_refs(&neutralized.html)
        .into_iter()
        .map(|r| (base.clone(), r))
        .collect();
    let mut seen = std::collections::HashSet::new();
    let mut read = 0usize;
    while let Some((from, reference)) = pending.pop() {
        if !local_ref::is_local_ref(&reference) {
            // http(s) and other schemes: the browser's network is off. A
            // share named in the page was neutralised above (what is left is
            // text, such as an `@import` quoted in a code sample), and one
            // named in a stylesheet refused the page already.
            continue;
        }
        // A reference the browser cannot resolve is one it cannot load.
        let Ok(url) = from.join(reference.trim()) else {
            continue;
        };
        if !seen.insert(url.to_string()) {
            continue;
        }
        read += 1;
        if read > MAX_STYLESHEETS {
            return Err(not_checkable(format!(
                "the page loads more than {MAX_STYLESHEETS} stylesheets"
            )));
        }
        let (css, label) = if url.scheme() == "data" {
            let bytes = data_url_bytes(url.as_str())
                .ok_or_else(|| not_checkable("an inline stylesheet cannot be decoded"))?;
            // Relative references in a `data:` stylesheet resolve against the
            // page (a superset: the browser resolves none of them).
            (bytes, "an inline stylesheet".to_string())
        } else if url.scheme() == "file" {
            // `file://host/...` is a share whatever the predicate said.
            let path = url
                .to_file_path()
                .map_err(|()| share_refused(url.as_str()))?;
            // A share is never opened, except the document's own folder when
            // the user printed it from one.
            let allowed = match base_dir.as_deref() {
                Some(dir) => local_ref::may_open(&path, dir),
                None => !local_ref::is_remote_path(&path),
            };
            if !allowed {
                return Err(share_refused(&path.display().to_string()));
            }
            let meta = match std::fs::metadata(&path) {
                Ok(meta) => meta,
                // Nothing there: nothing the browser could load either.
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => {
                    return Err(not_checkable(format!(
                        "cannot read the stylesheet {}: {e}",
                        path.display()
                    )))
                }
            };
            if meta.is_dir() {
                continue;
            }
            if meta.len() > MAX_STYLESHEET_BYTES {
                return Err(not_checkable(format!(
                    "the stylesheet {} is larger than {} MB",
                    path.display(),
                    MAX_STYLESHEET_BYTES / (1024 * 1024)
                )));
            }
            let bytes = std::fs::read(&path).map_err(|e| {
                not_checkable(format!(
                    "cannot read the stylesheet {}: {e}",
                    path.display()
                ))
            })?;
            if bytes.len() as u64 > MAX_STYLESHEET_BYTES {
                return Err(not_checkable(format!(
                    "the stylesheet {} grew past the limit while it was read",
                    path.display()
                )));
            }
            (bytes, path.display().to_string())
        } else {
            continue;
        };
        let text = html_scan::decode_stylesheet(&css)
            .ok_or_else(|| not_checkable(format!("the text encoding of {label} cannot be read")))?;
        if html_scan::css_has_share_ref(&text) {
            return Err(share_refused(&label));
        }
        if html_scan::has_unreadable_import(&text) {
            return Err(not_checkable(format!(
                "{label} imports a stylesheet whose name cannot be read"
            )));
        }
        let from = if url.scheme() == "data" { from } else { url };
        pending.extend(
            html_scan::stylesheet_refs(text.as_bytes())
                .into_iter()
                .map(|r| (from.clone(), r)),
        );
    }
    Ok(neutralized.html)
}

/// The Content-Security-Policy every staged page carries: no script, plugin,
/// frame, worker, form or connection; styles, images, fonts and media only
/// from the page's own folder (a `file:` URL, or the renderer's private
/// origin on Windows) or `data:`. Defence in depth: the renderer already
/// turns scripts off and refuses every other request natively. It is the
/// first thing in the staged page, and on Windows also a response header.
pub fn page_csp() -> String {
    let hosts = format!("https://{}", crate::render::DOC_HOST);
    format!(
        "default-src file: data: 'unsafe-inline' {hosts}; script-src 'none'; \
         object-src 'none'; frame-src 'none'; child-src 'none'; worker-src 'none'; \
         connect-src 'none'; form-action 'none'; base-uri file: {hosts}"
    )
}

/// Write the staged copy of `html` the renderer prints (module docs) to
/// `<work>/stage/page.html`, alone in its folder, and return its path.
fn stage_page(
    html: &[u8],
    base_href: &str,
    setup: &PageSetup,
    work: &WorkDir,
) -> Result<PathBuf, EngineError> {
    let staged_html = inject_head(html, base_href, setup.css_size.as_deref())?;
    let dir = work.path().join("stage");
    let page = dir.join("page.html");
    std::fs::create_dir_all(&dir)
        .and_then(|()| std::fs::write(&page, staged_html))
        .map_err(|e| {
            EngineError::ConversionFailed(Failure::new(
                "workdir_failed",
                format!("cannot prepare the conversion folder: {e}"),
            ))
        })?;
    Ok(page)
}

/// The base the staged page resolves its references against: the page's own
/// `<base href>` (its first real one, [`super::html_scan::own_base_href`])
/// resolved against `base_href`, the document folder's URL, when it stays on
/// that origin and is no share; else `base_href` itself. A relative
/// `<base href="assets/">` therefore means the document's `assets` folder on
/// every renderer, never the private stage folder the page is served from.
/// Pure.
pub fn effective_base(html: &[u8], base_href: &str) -> String {
    let Some(own) = super::html_scan::own_base_href(html) else {
        return base_href.to_string();
    };
    let Ok(ours) = url::Url::parse(base_href) else {
        return base_href.to_string();
    };
    let Ok(resolved) = ours.join(own.trim()) else {
        return base_href.to_string();
    };
    let same_origin = resolved.scheme() == ours.scheme()
        && resolved.host_str() == ours.host_str()
        && resolved.port() == ours.port();
    if same_origin && !super::local_ref::is_share_ref(resolved.as_str()) {
        resolved.to_string()
    } else {
        base_href.to_string()
    }
}

/// `html` with the renderer's head inserted first: the [`page_csp`] meta, the
/// `<base href>` of [`effective_base`], and the job's paper as
/// `@page { size }` when `css_size` names one. Pure.
///
/// The insertion point ([`super::html_scan::insertion_point`]) is just after
/// a leading doctype, else the very start: never before the doctype (quirks
/// mode), never inside a comment (a page that begins `<!-- <head> -->` used to
/// get its policy spliced into the comment), and before any script or
/// `<base>` of the page, so the policy covers the whole page and the injected
/// base is the one that counts (the first `<base href>` wins). The page is
/// decoded by its real encoding first and re-encoded as UTF-8 with a BOM
/// ([`super::html_scan::decode_page`]), so ASCII can be spliced into it and
/// the renderer reads it the way it was checked.
///
/// **Fails closed** with `page_not_checkable`: when the encoding cannot be
/// read, when there is no safe insertion point (a doctype that never
/// closes), or when the result does not start, for the HTML tokenizer, with
/// the policy as its very first element.
pub fn inject_head(
    html: &[u8],
    base_href: &str,
    css_size: Option<&str>,
) -> Result<Vec<u8>, EngineError> {
    let decoded = super::html_scan::decode_page(html)
        .ok_or_else(|| not_checkable("the page declares a text encoding that cannot be read"))?;
    let html = &decoded[..];
    let escape = |s: &str| {
        s.replace('&', "&amp;")
            .replace('"', "&quot;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
    };
    let mut snippet = format!(
        "<meta http-equiv=\"Content-Security-Policy\" content=\"{}\">",
        escape(&page_csp())
    );
    snippet.push_str(&format!(
        "<base href=\"{}\">",
        escape(&effective_base(html, base_href))
    ));
    if let Some(size) = css_size {
        let css: String = size
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | ' '))
            .collect();
        snippet.push_str(&format!("<style>@page {{ size: {css}; }}</style>"));
    }

    let at = super::html_scan::insertion_point(html)
        .ok_or_else(|| not_checkable("the page's doctype is never closed"))?;
    let mut out = Vec::with_capacity(html.len() + snippet.len());
    out.extend_from_slice(&html[..at]);
    out.extend_from_slice(snippet.as_bytes());
    out.extend_from_slice(&html[at..]);
    // The policy must be the first element the tokenizer meets.
    let first = super::html_scan::tags(&out[..(at + snippet.len()).min(out.len())])
        .into_iter()
        .next();
    let csp_first = first.is_some_and(|t| {
        !t.is_end
            && t.name == "meta"
            && t.span.start == at
            && t.attr("http-equiv")
                .is_some_and(|v| v.eq_ignore_ascii_case("content-security-policy"))
            && t.attr("content") == Some(page_csp().as_str())
    });
    if !csp_first {
        return Err(not_checkable(
            "the security policy could not be placed at the start of the page",
        ));
    }
    Ok(out)
}

/// Whether the browser has finished writing `pdf`: the file ends with the
/// `%%EOF` trailer and its size did not change since the previous poll
/// (tracked in `last_size`), so a half-flushed file is never taken.
pub fn pdf_is_complete(pdf: &Path, last_size: &std::cell::Cell<Option<u64>>) -> bool {
    use std::io::{Read, Seek, SeekFrom};
    let Ok(meta) = std::fs::metadata(pdf) else {
        return false;
    };
    let size = meta.len();
    let stable = last_size.replace(Some(size)) == Some(size);
    if !stable || size < 8 {
        return false;
    }
    let Ok(mut file) = std::fs::File::open(pdf) else {
        return false;
    };
    let tail_len = size.min(1024);
    if file.seek(SeekFrom::End(-(tail_len as i64))).is_err() {
        return false;
    }
    let mut tail = Vec::with_capacity(tail_len as usize);
    if file.read_to_end(&mut tail).is_err() {
        return false;
    }
    tail.windows(5).any(|w| w == b"%%EOF")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// [`inject_head`]'s staged page without its UTF-8 byte-order mark.
    fn inject(html: &[u8], base: &str, size: Option<&str>) -> Vec<u8> {
        let out = inject_head(html, base, size).unwrap();
        assert!(out.starts_with(b"\xEF\xBB\xBF"), "staged with a BOM");
        out[3..].to_vec()
    }

    fn reason(err: &EngineError) -> Option<&'static str> {
        match err {
            EngineError::ConversionFailed(f) => f.reason,
            _ => None,
        }
    }

    /// A browser-fallback test folder and its `file:` base URL.
    fn fallback_dir() -> (tempfile::TempDir, String) {
        let dir = tempfile::tempdir().unwrap();
        let base = format!("{}/", file_url(&dir.path().canonicalize().unwrap()));
        (dir, base)
    }

    fn utf16le(text: &str) -> Vec<u8> {
        let mut out = vec![0xFF, 0xFE];
        for unit in text.encode_utf16() {
            out.extend_from_slice(&unit.to_le_bytes());
        }
        out
    }

    /// Round-5 finding: a UTF-16 page reached the share filter undecoded and
    /// kept its `//host` reference. It is decoded first now, on every route.
    #[test]
    fn nfr_security_browser_fallback_decodes_a_utf16_page_before_the_filter() {
        let (_dir, base) = fallback_dir();
        let page =
            utf16le("<p>x</p><img src=\"//attacker/x.png\"><img src=\"file://attacker/y.png\">");
        let out = guard_browser_page(&page, &base).unwrap();
        assert!(out.starts_with(b"\xEF\xBB\xBF"));
        let text = String::from_utf8(out[3..].to_vec()).unwrap();
        assert!(!text.contains("attacker"), "{text}");
        assert!(text.contains("<p>x</p>"), "{text}");
        // The staged page stays UTF-8 (no second decoding by its old label).
        let staged = inject_head(&out, &base, None).unwrap();
        assert!(!String::from_utf8_lossy(&staged).contains("attacker"));
    }

    /// Round-5 finding: every way a stylesheet used to escape the check
    /// (too large, too many, UTF-16, an obfuscated `@import`, an unknown
    /// encoding) now refuses the page instead of loading it unchecked.
    #[test]
    fn nfr_security_browser_fallback_fails_closed_on_unchecked_stylesheets() {
        let page = br#"<link rel="stylesheet" href="a.css"><p>x"#;
        let refused = |setup: &dyn Fn(&Path)| {
            let (dir, base) = fallback_dir();
            setup(dir.path());
            reason(&guard_browser_page(page, &base).unwrap_err())
        };

        // (a) over the size cap.
        assert_eq!(
            refused(&|d| {
                let f = std::fs::File::create(d.join("a.css")).unwrap();
                f.set_len(MAX_STYLESHEET_BYTES + 1).unwrap();
            }),
            Some("page_not_checkable")
        );
        // (b) more stylesheets than the cap, the share in the last one.
        assert_eq!(
            refused(&|d| {
                let mut chain = String::new();
                for i in 0..=MAX_STYLESHEETS {
                    chain.push_str(&format!("@import 's{i}.css';"));
                    std::fs::write(d.join(format!("s{i}.css")), "p{}").unwrap();
                }
                std::fs::write(d.join("a.css"), chain).unwrap();
            }),
            Some("page_not_checkable")
        );
        // (c) a UTF-16 stylesheet that names a share is decoded and caught.
        assert_eq!(
            refused(&|d| {
                std::fs::write(
                    d.join("a.css"),
                    utf16le("p{background:url(//attacker/x.png)}"),
                )
                .unwrap()
            }),
            Some("share_reference_refused")
        );
        // ... and one without a BOM cannot be read faithfully.
        assert_eq!(
            refused(&|d| {
                std::fs::write(
                    d.join("a.css"),
                    &utf16le("p{background:url(//attacker/x.png)}")[2..],
                )
                .unwrap()
            }),
            Some("page_not_checkable")
        );
        // (d) `@import/**/` reaches the imported file.
        assert_eq!(
            refused(&|d| {
                std::fs::write(d.join("a.css"), "@import/**/\"evil.css\";").unwrap();
                std::fs::write(d.join("evil.css"), "p{background:url(//attacker/x.png)}").unwrap();
            }),
            Some("share_reference_refused")
        );
        // An `@import` whose target cannot be parsed.
        assert_eq!(
            refused(&|d| std::fs::write(d.join("a.css"), "@import foo;").unwrap()),
            Some("page_not_checkable")
        );
        // An unknown `@charset`.
        assert_eq!(
            refused(&|d| std::fs::write(d.join("a.css"), "@charset \"x-nope\"; p{}").unwrap()),
            Some("page_not_checkable")
        );
        // A `data:` stylesheet is read like a file (percent-encoded or base64).
        for href in [
            "data:text/css,p%7Bbackground:url(%22%2F%2Fattacker%2Fx.png%22)%7D",
            "data:text/css;base64,cHtiYWNrZ3JvdW5kOnVybCgvL2F0dGFja2VyL3gucG5nKX0=",
        ] {
            let (_dir, base) = fallback_dir();
            let page = format!("<link rel=stylesheet href=\"{href}\">");
            let err = guard_browser_page(page.as_bytes(), &base).unwrap_err();
            assert_eq!(reason(&err), Some("share_reference_refused"), "{href}");
        }
        // A style block with an unreadable import.
        let (_dir, base) = fallback_dir();
        let err = guard_browser_page(b"<style>@import/**/foo;</style>", &base).unwrap_err();
        assert_eq!(reason(&err), Some("page_not_checkable"));
        // A page whose declared encoding is unknown.
        let err = guard_browser_page(b"<meta charset=x-nope><p>x", &base).unwrap_err();
        assert_eq!(reason(&err), Some("page_not_checkable"));

        // A clean page with clean stylesheets still prints, and an `@import`
        // quoted in the page's text is not mistaken for one.
        let (dir, base) = fallback_dir();
        std::fs::write(dir.path().join("a.css"), "@import 'b.css'; p{}").unwrap();
        std::fs::write(dir.path().join("b.css"), "p{color:red}").unwrap();
        let ok =
            br#"<link rel="stylesheet" href="a.css"><pre>@import url(//fonts.example/x);</pre>"#;
        assert!(guard_browser_page(ok, &base).is_ok());
        // A stylesheet that does not exist cannot be loaded either.
        let (_dir, base) = fallback_dir();
        assert!(guard_browser_page(page, &base).is_ok());
    }

    /// Security note 6: the default renderer's injection fails closed when
    /// the policy cannot be placed first.
    #[test]
    fn nfr_security_injection_fails_closed() {
        for page in [
            &b"<!DOCTYPE html, no end to the doctype"[..],
            b"<meta charset=x-nope><p>x",
            b"<meta charset=iso-2022-kr><p>x",
        ] {
            let err = inject_head(page, "file:///d/", None).unwrap_err();
            assert_eq!(reason(&err), Some("page_not_checkable"));
        }
        // ... and the system route refuses such a page before the worker runs.
        let work = WorkDir::new("inject-closed").unwrap();
        let err = render_system_with(
            Path::new("/nonexistent/worker"),
            b"<!DOCTYPE html",
            Path::new("/tmp"),
            Path::new("/tmp/never.pdf"),
            &work,
            &AtomicBool::new(false),
            None,
            &PageSetup::default(),
        )
        .unwrap_err();
        assert_eq!(reason(&err), Some("page_not_checkable"));
    }

    #[test]
    fn fr_055_browser_args_print_headless_with_a_private_profile() {
        let args = browser_args(
            Path::new("/w/browser"),
            Path::new("/w/out.pdf"),
            "file:///docs/a%20b.html",
        );
        for flag in ["--headless", "--disable-gpu", "--no-pdf-header-footer"] {
            assert!(args.contains(&flag.to_string()), "{flag}");
        }
        assert!(args.contains(&"--user-data-dir=/w/browser".to_string()));
        assert!(args.contains(&"--print-to-pdf=/w/out.pdf".to_string()));
        assert_eq!(args.last().unwrap(), "file:///docs/a%20b.html");
    }

    /// A printed page reaches no network (loopback included), whatever it
    /// asks for.
    #[test]
    fn fr_055_browser_runs_without_network() {
        let args = browser_args(Path::new("/w/b"), Path::new("/w/o.pdf"), "file:///p.html");
        for flag in [
            "--proxy-server=127.0.0.1:9",
            "--force-webrtc-ip-handling-policy=disable_non_proxied_udp",
            "--proxy-bypass-list=<-loopback>",
            "--host-resolver-rules=MAP * ~NOTFOUND",
            "--disable-remote-fonts",
        ] {
            assert!(args.contains(&flag.to_string()), "{flag}: {args:?}");
        }
        // This switch makes headless --print-to-pdf hang (Chrome 154).
        assert!(
            !args.iter().any(|a| a.contains("scriptEnabled")),
            "{args:?}"
        );
        assert_eq!(args.last().unwrap(), "file:///p.html");
    }

    #[test]
    fn fr_055_the_head_is_injected_right_after_the_doctype() {
        let html = b"<!DOCTYPE html><html lang=en><HEAD><title>x</title></head><body><header>h</header></body></html>";
        let out = String::from_utf8(inject(html, "file:///docs/", Some("210mm 297mm"))).unwrap();
        assert!(
            out.starts_with("<!DOCTYPE html><meta http-equiv=\"Content-Security-Policy\""),
            "{out}"
        );
        assert!(
            out.contains(
                "<base href=\"file:///docs/\"><style>@page { size: 210mm 297mm; }</style><html lang=en>"
            ),
            "{out}"
        );
        assert!(out.ends_with("<body><header>h</header></body></html>"));
    }

    #[test]
    fn fr_055_injection_keeps_the_doctype_first() {
        // No head or html tag: after the doctype, never before it.
        let out = String::from_utf8(inject(
            b"<!doctype html><p>hi",
            "file:///d/",
            Some("8.5in 11in"),
        ))
        .unwrap();
        assert!(out.starts_with("<!doctype html><meta "), "{out}");
        assert!(out.contains("size: 8.5in 11in"));

        // No paper: no @page rule at all.
        let out = String::from_utf8(inject(b"<p>x", "file:///d/", None)).unwrap();
        assert!(!out.contains("@page"), "{out}");
        assert!(out.ends_with("<p>x"));
    }

    /// Security finding: a `<head>` (or `<base>`) written inside a comment
    /// used to receive the policy, inside the comment, where it does
    /// nothing. The policy now comes first, before any comment or script of
    /// the page, and a commented-out `<base>` is not the page's.
    #[test]
    fn nfr_security_csp_is_never_spliced_into_a_comment() {
        let page = b"<!-- <head> <base href='//evil/'> --><script>x()</script><html><head><title>t</title>";
        let out = String::from_utf8(inject(page, "file:///d/", None)).unwrap();
        assert!(
            out.starts_with("<meta http-equiv=\"Content-Security-Policy\""),
            "{out}"
        );
        assert!(
            out.contains("<base href=\"file:///d/\"><!-- <head>"),
            "{out}"
        );

        let page = b"<!DOCTYPE html><!-- <head> --><head><title>t</title>";
        let out = String::from_utf8(inject(page, "file:///d/", None)).unwrap();
        assert!(
            out.starts_with("<!DOCTYPE html><meta http-equiv=\"Content-Security-Policy\""),
            "{out}"
        );
        let csp_at = out.find("Content-Security-Policy").unwrap();
        assert!(csp_at < out.find("<!--").unwrap(), "{out}");
    }

    /// The page's own relative `<base href>` means the document's folder on
    /// every renderer (it used to resolve against the private stage folder on
    /// Windows); an absolute, remote or share base is not followed.
    #[test]
    fn fr_055_a_relative_own_base_resolves_against_the_document_folder() {
        let base = |page: &[u8], ours: &str| effective_base(page, ours);
        let page = b"<html><head><base href=\"assets/\"></head>";
        assert_eq!(base(page, "file:///docs/"), "file:///docs/assets/");
        assert_eq!(
            base(page, "https://doc.pluryprint.example/"),
            "https://doc.pluryprint.example/assets/"
        );
        assert_eq!(
            base(b"<base href='./'>", "https://doc.pluryprint.example/"),
            "https://doc.pluryprint.example/"
        );
        for other in [
            "<base href='https://x/'>",
            "<base href='//host/share/'>",
            "<base href='file://host/share/'>",
            "<base href='\\\\host\\share\\'>",
            "<base href='/\\host/share/'>",
        ] {
            assert_eq!(
                base(other.as_bytes(), "file:///docs/"),
                "file:///docs/",
                "{other}"
            );
            assert_eq!(
                base(other.as_bytes(), "https://doc.pluryprint.example/"),
                "https://doc.pluryprint.example/",
                "{other}"
            );
        }
        // The injected base comes first, so it is the one the page uses.
        let out = String::from_utf8(inject(page, "https://doc.pluryprint.example/", None)).unwrap();
        let ours = out
            .find("<base href=\"https://doc.pluryprint.example/assets/\">")
            .unwrap();
        assert!(ours < out.find("<base href=\"assets/\">").unwrap(), "{out}");
    }

    /// Security finding: the browser fallback loads the page from `file:`,
    /// so a share reference would reach SMB. The page is rewritten first, and
    /// a local stylesheet that names a share refuses it.
    #[test]
    fn nfr_security_browser_fallback_never_hands_a_share_to_the_browser() {
        let dir = tempfile::tempdir().unwrap();
        let base = format!("{}/", file_url(&dir.path().canonicalize().unwrap()));
        let page =
            br#"<img src="//host/x.png"><img src="file:///%5C%5Chost/y.png"><img src="ok.png">"#;
        let out = String::from_utf8(guard_browser_page(page, &base).unwrap()).unwrap();
        assert!(!out.contains("host"), "{out}");
        assert!(out.contains("ok.png"), "{out}");

        std::fs::write(dir.path().join("a.css"), "@import 'b.css';").unwrap();
        std::fs::write(
            dir.path().join("b.css"),
            "@font-face { src: url(\\2f\\2fhost/f.woff) }",
        )
        .unwrap();
        let page = br#"<link rel="stylesheet" href="a.css"><p>x"#;
        let err = guard_browser_page(page, &base).unwrap_err();
        assert!(
            matches!(&err, EngineError::ConversionFailed(f) if f.reason == Some("share_reference_refused")),
            "{err:?}"
        );
        std::fs::write(dir.path().join("b.css"), "p { color: red }").unwrap();
        assert_eq!(
            guard_browser_page(page, &base).unwrap(),
            [&b"\xEF\xBB\xBF"[..], page].concat()
        );
    }

    /// The worker's own watchdog is a timeout (terminal), never a render
    /// failure the queue would retry; the worker gets the job's limit.
    #[cfg(unix)]
    #[test]
    fn nfr_render_worker_timeout_is_terminal_with_reason() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let fake = dir.path().join("fake-worker.sh");
        let args_log = dir.path().join("args.txt");
        std::fs::write(
            &fake,
            format!(
                "#!/bin/sh\necho \"$@\" > '{}'\necho '{}: the render did not finish within 7s' >&2\nexit 1\n",
                args_log.display(),
                crate::render::TIMEOUT_MARKER
            ),
        )
        .unwrap();
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        let work = WorkDir::new("timeout-test").unwrap();
        let cancel = AtomicBool::new(false);
        let err = render_system_with(
            &fake,
            b"<p>x",
            dir.path(),
            &dir.path().join("o.pdf"),
            &work,
            &cancel,
            Some(Duration::from_secs(7)),
            &PageSetup::default(),
        )
        .unwrap_err();
        assert!(
            matches!(&err, EngineError::ConversionTimedOut(f) if f.reason == Some("tool_timed_out")),
            "{err:?}"
        );
        let args = std::fs::read_to_string(&args_log).unwrap();
        assert!(args.contains("--timeout-secs 7"), "{args}");

        // No limit (the setting is 0): no watchdog argument at all.
        let _ = render_system_with(
            &fake,
            b"<p>x",
            dir.path(),
            &dir.path().join("o.pdf"),
            &work,
            &cancel,
            None,
            &PageSetup::default(),
        );
        let args = std::fs::read_to_string(&args_log).unwrap();
        assert!(!args.contains("--timeout-secs"), "{args}");
    }

    #[test]
    fn fr_055_injection_paper_rule() {
        let out = String::from_utf8(inject(
            b"<html><head></head></html>",
            "file:///d/",
            Some("4in 6in"),
        ))
        .unwrap();
        assert_eq!(out.matches("<base").count(), 1, "{out}");
        assert!(out.contains("size: 4in 6in"));

        // No paper: no @page rule at all.
        let out = String::from_utf8(inject(b"<p>x", "file:///d/", None)).unwrap();
        assert!(!out.contains("@page"), "{out}");
        assert!(out.ends_with("<p>x"));
    }

    #[test]
    fn fr_055_a_utf16_page_is_reencoded_before_the_injection() {
        let mut page = vec![0xFF, 0xFE];
        for unit in "<html><head></head><p>Ñandú</p>".encode_utf16() {
            page.extend_from_slice(&unit.to_le_bytes());
        }
        let out = inject_head(&page, "file:///d/", None).unwrap();
        assert!(out.starts_with(&[0xEF, 0xBB, 0xBF]));
        let text = String::from_utf8(out[3..].to_vec()).unwrap();
        assert!(text.starts_with("<meta http-equiv"), "{text}");
        assert!(text.contains("Ñandú"));
    }

    /// The CSP forbids everything active and every connection; the page's
    /// own policies can only narrow it further (CSPs intersect).
    #[test]
    fn fr_055_the_page_policy_forbids_script_frames_and_connections() {
        let csp = page_csp();
        for directive in [
            "script-src 'none'",
            "object-src 'none'",
            "frame-src 'none'",
            "connect-src 'none'",
            "worker-src 'none'",
            "form-action 'none'",
        ] {
            assert!(csp.contains(directive), "{directive}: {csp}");
        }
        assert!(!csp.contains("http:"), "{csp}");
        assert!(!csp.contains("https: "), "no blanket https: {csp}");
        assert!(!csp.contains('*'), "{csp}");
    }

    #[test]
    fn fr_055_page_size_from_pagesetup() {
        let a4 = paper_inches("210mm 297mm").unwrap();
        assert!((a4.0 - 8.2677).abs() < 1e-3 && (a4.1 - 11.6929).abs() < 1e-3);
        assert_eq!(paper_inches("11in 8.5in"), Some((11.0, 8.5)));
        assert_eq!(paper_inches("288pt 432pt"), Some((4.0, 6.0)));
        assert_eq!(paper_inches("auto"), None);
        assert_eq!(paper_inches("210mm"), None);
        assert_eq!(paper_inches("0mm 297mm"), None);
        assert_eq!(paper_inches("1in 2in 3in"), None);
        // Every size the job settings can produce is understood.
        for paper in ["A3", "A4", "A5", "Letter", "Legal", "4x6", "w288h432"] {
            for orientation in [
                crate::queue::job::Orientation::Portrait,
                crate::queue::job::Orientation::Landscape,
            ] {
                let css = super::super::css_page_size(
                    &crate::queue::job::PaperSize::parse(paper).unwrap(),
                    orientation,
                )
                .unwrap();
                assert!(paper_inches(&css).is_some(), "{paper}: {css}");
            }
        }
    }

    #[test]
    fn nfr_worker_exe_is_the_app_even_from_the_console_cli() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        let (cli, gui) = if cfg!(windows) {
            ("pluryprint-cli.exe", "pluryprint.exe")
        } else {
            ("pluryprint-cli", "pluryprint")
        };
        std::fs::write(dir.join(gui), b"app").unwrap();
        assert_eq!(worker_exe_for(&dir.join(cli)).unwrap(), dir.join(gui));
        assert_eq!(worker_exe_for(&dir.join(gui)).unwrap(), dir.join(gui));
    }

    /// The CLI without the app next to it fails with a registered reason the
    /// queue can explain, not a reasonless "dependency missing" pause.
    #[test]
    fn nfr_render_worker_missing_is_a_registered_reason() {
        let tmp = tempfile::tempdir().unwrap();
        let cli = if cfg!(windows) {
            "pluryprint-cli.exe"
        } else {
            "pluryprint-cli"
        };
        let err = worker_exe_for(&tmp.path().join(cli)).unwrap_err();
        assert!(
            matches!(&err, EngineError::ConversionFailed(f) if f.reason == Some("render_worker_missing")),
            "{err:?}"
        );
        assert!(crate::util::reasons::JOB_REASONS
            .iter()
            .any(|(code, reason, _)| *code == "conversion_failed"
                && *reason == "render_worker_missing"));
    }

    #[test]
    fn fr_055_a_pdf_counts_as_complete_only_with_a_trailer_and_a_stable_size() {
        let dir = tempfile::tempdir().unwrap();
        let pdf = dir.path().join("out.pdf");
        let last = std::cell::Cell::new(None);
        assert!(!pdf_is_complete(&pdf, &last), "absent");

        std::fs::write(&pdf, b"%PDF-1.4\n1 0 obj").unwrap();
        assert!(!pdf_is_complete(&pdf, &last), "first sighting");
        assert!(!pdf_is_complete(&pdf, &last), "no trailer yet");

        std::fs::write(&pdf, b"%PDF-1.4\n1 0 obj\n%%EOF\n").unwrap();
        assert!(!pdf_is_complete(&pdf, &last), "size just changed");
        assert!(pdf_is_complete(&pdf, &last), "trailer + stable size");
    }
}
