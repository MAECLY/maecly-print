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
//!   (`share_reference_refused`). Still one more reason it is only a fallback:
//!   an external SVG `<use>` document's own references are not inspected.

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

/// Make `html` safe for the browser fallback, which loads the page from
/// `file:` and so cannot keep a share reference off SMB the way the system
/// renderer does (module docs): every share reference in the page is
/// neutralised ([`super::html_scan::neutralize_share_refs`]), and the local
/// stylesheets it loads (`<link>` and `@import`, recursively) are read: one
/// that names a share refuses the page with `share_reference_refused`, since
/// it cannot be rewritten in place. `base_url` is the document folder's
/// `file:` URL.
pub fn guard_browser_page(html: &[u8], base_url: &str) -> Result<Vec<u8>, EngineError> {
    use super::{html_scan, local_ref};
    let neutralized = html_scan::neutralize_share_refs(html);
    if neutralized.removed > 0 {
        tracing::info!(
            event = "html_share_refs_removed",
            count = neutralized.removed
        );
    }
    let base = url::Url::parse(&effective_base(&neutralized.html, base_url)).ok();
    let base_dir = url::Url::parse(base_url)
        .ok()
        .and_then(|u| u.to_file_path().ok());
    let mut pending: Vec<(url::Url, String)> = base
        .iter()
        .flat_map(|b| {
            html_scan::stylesheet_refs(&neutralized.html)
                .into_iter()
                .map(move |r| (b.clone(), r))
        })
        .collect();
    let mut seen = std::collections::HashSet::new();
    while let Some((from, reference)) = pending.pop() {
        if seen.len() >= MAX_STYLESHEETS {
            break;
        }
        if !local_ref::is_local_ref(&reference) {
            continue;
        }
        let Ok(url) = from.join(reference.trim()) else {
            continue;
        };
        let Ok(path) = url.to_file_path() else {
            continue;
        };
        // A share is never opened, except the document's own folder when the
        // user printed it from one.
        let allowed = match base_dir.as_deref() {
            Some(dir) => local_ref::may_open(&path, dir),
            None => !local_ref::is_remote_path(&path),
        };
        if !allowed || !seen.insert(path.clone()) {
            continue;
        }
        let small =
            std::fs::metadata(&path).is_ok_and(|m| m.is_file() && m.len() <= MAX_STYLESHEET_BYTES);
        let Some(css) = small.then(|| std::fs::read(&path).ok()).flatten() else {
            continue;
        };
        if html_scan::css_has_share_ref(&String::from_utf8_lossy(&css)) {
            tracing::warn!(event = "html_share_ref_in_stylesheet");
            return Err(EngineError::ConversionFailed(Failure::new(
                "share_reference_refused",
                format!(
                    "a stylesheet of the page refers to a network share: {}",
                    path.display()
                ),
            )));
        }
        pending.extend(
            html_scan::stylesheet_refs(&css)
                .into_iter()
                .map(|r| (url.clone(), r)),
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
    let staged_html = inject_head(html, base_href, setup.css_size.as_deref());
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
/// base is the one that counts (the first `<base href>` wins). A UTF-16 page
/// (a byte-order mark) is re-encoded as UTF-8 with a BOM first, so ASCII can
/// be spliced into it.
pub fn inject_head(html: &[u8], base_href: &str, css_size: Option<&str>) -> Vec<u8> {
    let transcoded;
    let html = match encoding_rs::Encoding::for_bom(html) {
        Some((enc, bom_len)) if enc != encoding_rs::UTF_8 => {
            let (text, _) = enc.decode_without_bom_handling(&html[bom_len..]);
            let mut bytes = vec![0xEF, 0xBB, 0xBF];
            bytes.extend_from_slice(text.as_bytes());
            transcoded = bytes;
            &transcoded[..]
        }
        _ => html,
    };
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

    let at = super::html_scan::insertion_point(html);
    let mut out = Vec::with_capacity(html.len() + snippet.len());
    out.extend_from_slice(&html[..at]);
    out.extend_from_slice(snippet.as_bytes());
    out.extend_from_slice(&html[at..]);
    out
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
        let out =
            String::from_utf8(inject_head(html, "file:///docs/", Some("210mm 297mm"))).unwrap();
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
        let out = String::from_utf8(inject_head(
            b"<!doctype html><p>hi",
            "file:///d/",
            Some("8.5in 11in"),
        ))
        .unwrap();
        assert!(out.starts_with("<!doctype html><meta "), "{out}");
        assert!(out.contains("size: 8.5in 11in"));

        // No paper: no @page rule at all.
        let out = String::from_utf8(inject_head(b"<p>x", "file:///d/", None)).unwrap();
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
        let out = String::from_utf8(inject_head(page, "file:///d/", None)).unwrap();
        assert!(
            out.starts_with("<meta http-equiv=\"Content-Security-Policy\""),
            "{out}"
        );
        assert!(
            out.contains("<base href=\"file:///d/\"><!-- <head>"),
            "{out}"
        );

        let page = b"<!DOCTYPE html><!-- <head> --><head><title>t</title>";
        let out = String::from_utf8(inject_head(page, "file:///d/", None)).unwrap();
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
        let out =
            String::from_utf8(inject_head(page, "https://doc.pluryprint.example/", None)).unwrap();
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
        assert_eq!(guard_browser_page(page, &base).unwrap(), page.to_vec());
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
        let out = String::from_utf8(inject_head(
            b"<html><head></head></html>",
            "file:///d/",
            Some("4in 6in"),
        ))
        .unwrap();
        assert_eq!(out.matches("<base").count(), 1, "{out}");
        assert!(out.contains("size: 4in 6in"));

        // No paper: no @page rule at all.
        let out = String::from_utf8(inject_head(b"<p>x", "file:///d/", None)).unwrap();
        assert!(!out.contains("@page"), "{out}");
        assert!(out.ends_with("<p>x"));
    }

    #[test]
    fn fr_055_a_utf16_page_is_reencoded_before_the_injection() {
        let mut page = vec![0xFF, 0xFE];
        for unit in "<html><head></head><p>Ñandú</p>".encode_utf16() {
            page.extend_from_slice(&unit.to_le_bytes());
        }
        let out = inject_head(&page, "file:///d/", None);
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
