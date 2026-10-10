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
//!   cannot keep a `file://host/…` sub-resource off a file share: that is one
//!   more reason it is only a fallback.

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
            let page = stage_page(html, &base, setup, work)?;
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
    Ok(worker_exe_for(&me))
}

/// [`worker_exe`] for a given running executable. Pure.
pub fn worker_exe_for(me: &Path) -> PathBuf {
    let stem = me
        .file_stem()
        .map(|s| s.to_string_lossy().to_ascii_lowercase());
    if stem.as_deref() == Some("pluryprint-cli") {
        let gui = if cfg!(windows) {
            "pluryprint.exe"
        } else {
            "pluryprint"
        };
        me.with_file_name(gui)
    } else {
        me.to_path_buf()
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

/// The Content-Security-Policy every staged page carries: no script, plugin,
/// frame, worker, form or connection; styles, images, fonts and media only
/// from the page's own folder (a `file:` URL, or the renderer's private
/// origins on Windows) or `data:`. Defence in depth: the renderer already
/// turns scripts off and refuses every other request natively.
pub fn page_csp() -> String {
    let hosts = format!(
        "https://{} https://{}",
        crate::render::DOC_HOST,
        crate::render::STAGE_HOST
    );
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

/// Position just past the `>` of the first tag named `name` (ASCII,
/// case-insensitive, `name` followed by whitespace, `/` or `>`), if any.
fn after_tag(html: &[u8], name: &str) -> Option<usize> {
    let lower: Vec<u8> = html.iter().map(u8::to_ascii_lowercase).collect();
    let needle = format!("<{name}");
    let mut from = 0;
    while let Some(rel) = lower[from..]
        .windows(needle.len())
        .position(|w| w == needle.as_bytes())
    {
        let start = from + rel;
        let next = lower.get(start + needle.len()).copied();
        if matches!(next, Some(b'>' | b'/' | b' ' | b'\t' | b'\n' | b'\r')) {
            let close = lower[start..].iter().position(|&b| b == b'>')?;
            return Some(start + close + 1);
        }
        from = start + 1;
    }
    None
}

/// `html` with the renderer's head inserted at the start of its head: the
/// [`page_csp`] meta, a `<base href>` (unless the page has its own), and the
/// job's paper as `@page { size }` when `css_size` names one. Pure.
///
/// The insertion point is just after `<head>`, else after `<html>`, else
/// after the doctype, else the very start: never before the doctype, which
/// would drop the page into quirks mode. A UTF-16 page (a byte-order mark) is
/// re-encoded as UTF-8 with a BOM first, so ASCII can be spliced into it.
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
    if after_tag(html, "base").is_none() {
        snippet.push_str(&format!("<base href=\"{}\">", escape(base_href)));
    }
    if let Some(size) = css_size {
        let css: String = size
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | ' '))
            .collect();
        snippet.push_str(&format!("<style>@page {{ size: {css}; }}</style>"));
    }

    let at = after_tag(html, "head")
        .or_else(|| after_tag(html, "html"))
        .or_else(|| after_tag(html, "!doctype"))
        .unwrap_or_else(|| {
            // Keep a UTF-8 BOM first.
            if html.starts_with(&[0xEF, 0xBB, 0xBF]) {
                3
            } else {
                0
            }
        });
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
    fn fr_055_the_head_is_injected_after_the_head_tag() {
        let html = b"<!DOCTYPE html><html lang=en><HEAD><title>x</title></head><body><header>h</header></body></html>";
        let out =
            String::from_utf8(inject_head(html, "file:///docs/", Some("210mm 297mm"))).unwrap();
        let head_at = out.find("<HEAD>").unwrap() + "<HEAD>".len();
        assert!(
            out[head_at..].starts_with("<meta http-equiv=\"Content-Security-Policy\""),
            "{out}"
        );
        assert!(
            out.contains(
                "<base href=\"file:///docs/\"><style>@page { size: 210mm 297mm; }</style><title>"
            ),
            "{out}"
        );
        assert!(out.ends_with("<body><header>h</header></body></html>"));
    }

    #[test]
    fn fr_055_injection_keeps_the_doctype_first_and_an_own_base() {
        // No head or html tag: after the doctype, never before it.
        let out = String::from_utf8(inject_head(
            b"<!doctype html><p>hi",
            "file:///d/",
            Some("8.5in 11in"),
        ))
        .unwrap();
        assert!(out.starts_with("<!doctype html><meta "), "{out}");

        // The page's own <base> is respected.
        let out = String::from_utf8(inject_head(
            b"<html><head><base href='https://x/'></head></html>",
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
        assert!(text.contains("<head><meta http-equiv"), "{text}");
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
        let dir = if cfg!(windows) { r"C:\P" } else { "/opt/p" };
        let dir = Path::new(dir);
        let (cli, gui) = if cfg!(windows) {
            ("pluryprint-cli.exe", "pluryprint.exe")
        } else {
            ("pluryprint-cli", "pluryprint")
        };
        assert_eq!(worker_exe_for(&dir.join(cli)), dir.join(gui));
        assert_eq!(worker_exe_for(&dir.join(gui)), dir.join(gui));
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
