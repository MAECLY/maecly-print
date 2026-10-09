//! HTML → PDF through a headless Chromium-class browser (FR-055): Microsoft
//! Edge (always present on Windows), Google Chrome or Chromium.
//!
//! ```text
//! <browser> --headless --disable-gpu --no-pdf-header-footer
//!           --user-data-dir=<work>/browser --no-first-run ...
//!           --print-to-pdf=<work>/out.pdf file:///path/to/page.html
//! ```
//!
//! - The page is loaded **from its own location**, so relative CSS and images
//!   resolve exactly as they do when the user opens the file in a browser.
//! - `--user-data-dir` points at a throwaway directory: the user's running
//!   browser and its profile are never touched, and a headless run cannot
//!   attach to (or be blocked by) an open browser window.
//! - The step ends when the PDF is complete, not when the browser exits:
//!   some Chrome builds (156 on macOS, at least) write the file in a second
//!   and then idle forever. [`pdf_is_complete`] waits for the `%%EOF`
//!   trailer and a stable size, then the browser tree is killed.
//! - The page gets **no network**. A file someone sent the user must not
//!   phone home (IP address, "opened at" timing, a tracking pixel) just
//!   because it was printed, so every `http(s)`/`ws` request goes to a proxy
//!   on a closed local port (`127.0.0.1:9`, loopback included, so
//!   `localhost` services are not reachable either), host names never
//!   resolve, WebRTC may not send UDP around the proxy, and web fonts are
//!   not downloaded ([`SANDBOX_ARGS`]). Local files (`file://` stylesheets
//!   and images next to the page) still load, so a saved page prints the way
//!   it looks offline. Remote `http(s)://` *inputs* are not accepted — only
//!   local files reach this converter.
//! - JavaScript stays **on** for HTML files. Chrome's only per-run switch
//!   for it (`--blink-settings=scriptEnabled=false`) makes headless
//!   `--print-to-pdf` hang without writing anything (Chrome 154, verified),
//!   and a saved page that draws its content with script would print blank.
//!   With the network gone a script has nowhere to send what it sees, and
//!   Chrome's file-origin rules already stop a `file://` page from reading
//!   other local files. Markdown pages carry no script at all (see
//!   [`super::markdown`]).

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use super::process::run_until;
use super::{file_url, PageSetup, WorkDir};
use crate::engine::EngineError;

/// Extensions rendered by the browser directly.
pub const HTML_EXTS: &[&str] = &["html", "htm"];

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

/// The flags that keep a printed page from reaching the network (module docs).
///
/// - All proxied traffic goes to the discard port on loopback, where nothing
///   listens, and `<-loopback>` removes Chrome's implicit "never proxy
///   localhost" rule so local services are refused the same way.
/// - `MAP * ~NOTFOUND` makes every host name fail to resolve, so not even a
///   DNS lookup leaks the page's links (the proxy's address is an IP literal
///   and needs no lookup).
/// - WebRTC is the one API that can send UDP straight to an IP address
///   around a proxy; `disable_non_proxied_udp` closes that, and QUIC is off
///   so nothing tries HTTP/3 outside the proxy either.
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

/// Render the local HTML file `page` with the browser at `browser` into
/// `output`, on the paper `setup` names.
///
/// The browser lays pages out on US Letter unless the page says otherwise
/// with CSS `@page { size: … }`. When the job names a paper, the page is
/// loaded from a copy in the work directory that starts with that rule (and
/// a `<base href>` pointing at the original folder, so relative CSS and
/// images still resolve); the user's file is never modified. A size the page
/// sets itself still wins, since its own rule comes later in the cascade.
pub fn convert_with(
    browser: &Path,
    page: &Path,
    output: &Path,
    work: &WorkDir,
    cancel: &AtomicBool,
    timeout: Option<Duration>,
    setup: &PageSetup,
) -> Result<(), EngineError> {
    let produced = work.path().join("out.pdf");
    let profile = work.path().join("browser");
    let target = staged_page(page, work, setup)?;
    let mut cmd = Command::new(browser);
    cmd.args(browser_args(&profile, &produced, &file_url(&target)))
        .current_dir(work.path());
    let log = work.path().join("browser.log");
    // Some Chrome builds write the PDF and then never exit; finish as soon as
    // the file is complete instead of waiting for an exit that may not come.
    let last_size = std::cell::Cell::new(None);
    let done = || pdf_is_complete(&produced, &last_size);
    run_until(cmd, "The browser", cancel, timeout, &log, Some(&done))?;
    super::take_output(&produced, output, "The browser", &log)
}

/// The file the browser should load: `page` itself, or a copy in the work
/// directory carrying the job's paper size ([`inject_page_setup`]).
fn staged_page(page: &Path, work: &WorkDir, setup: &PageSetup) -> Result<PathBuf, EngineError> {
    let Some(size) = setup.css_size.as_deref() else {
        return Ok(page.to_path_buf());
    };
    let html = std::fs::read(page).map_err(|e| {
        EngineError::ConversionFailed(format!("cannot read {}: {e}", page.display()))
    })?;
    let mut base = file_url(page.parent().unwrap_or_else(|| Path::new(".")));
    if !base.ends_with('/') {
        base.push('/');
    }
    let Some(staged_html) = inject_page_setup(&html, &base, size) else {
        return Ok(page.to_path_buf());
    };
    let staged = work.path().join("page.html");
    std::fs::write(&staged, staged_html).map_err(|e| {
        EngineError::ConversionFailed(format!("cannot prepare the conversion folder: {e}"))
    })?;
    Ok(staged)
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

/// `html` with a `<style>@page { size: … }</style>` (and a `<base href>`
/// when the page has none) inserted at the start of its head. Pure.
///
/// The insertion point is just after `<head>`, else after `<html>`, else
/// after the doctype, else the very start: never before the doctype, which
/// would drop the page into quirks mode. `None` for a UTF-16 page (a BOM),
/// where ASCII bytes cannot be spliced in; the original is printed then.
pub fn inject_page_setup(html: &[u8], base_href: &str, css_size: &str) -> Option<Vec<u8>> {
    if html.starts_with(&[0xFF, 0xFE]) || html.starts_with(&[0xFE, 0xFF]) {
        return None;
    }
    let escape = |s: &str| {
        s.replace('&', "&amp;")
            .replace('"', "&quot;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
    };
    let has_base = after_tag(html, "base").is_some();
    let mut snippet = String::new();
    if !has_base {
        snippet.push_str(&format!("<base href=\"{}\">", escape(base_href)));
    }
    let css: String = css_size
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | ' '))
        .collect();
    snippet.push_str(&format!("<style>@page {{ size: {css}; }}</style>"));

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
    Some(out)
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
        // This switch makes headless --print-to-pdf hang (module docs).
        assert!(
            !args.iter().any(|a| a.contains("scriptEnabled")),
            "{args:?}"
        );
        // The page URL stays the last argument, after every switch.
        assert_eq!(args.last().unwrap(), "file:///p.html");
    }

    #[test]
    fn fr_055_the_paper_size_is_injected_after_the_head_tag() {
        let html = b"<!DOCTYPE html><html lang=en><HEAD><title>x</title></head><body><header>h</header></body></html>";
        let out =
            String::from_utf8(inject_page_setup(html, "file:///docs/", "210mm 297mm").unwrap())
                .unwrap();
        assert!(
            out.starts_with("<!DOCTYPE html><html lang=en><HEAD><base href=\"file:///docs/\"><style>@page { size: 210mm 297mm; }</style><title>"),
            "{out}"
        );
        assert!(out.ends_with("<body><header>h</header></body></html>"));
    }

    #[test]
    fn fr_055_injection_keeps_the_doctype_first_and_an_own_base() {
        // No head or html tag: after the doctype, never before it.
        let out = String::from_utf8(
            inject_page_setup(b"<!doctype html><p>hi", "file:///d/", "8.5in 11in").unwrap(),
        )
        .unwrap();
        assert!(out.starts_with("<!doctype html><base href="), "{out}");

        // The page's own <base> is respected.
        let out = String::from_utf8(
            inject_page_setup(
                b"<html><head><base href='https://x/'></head></html>",
                "file:///d/",
                "4in 6in",
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(out.matches("<base").count(), 1, "{out}");
        assert!(out.contains("size: 4in 6in"));

        // UTF-16 cannot be spliced with ASCII bytes.
        assert!(inject_page_setup(&[0xFF, 0xFE, b'<', 0], "file:///d/", "4in 6in").is_none());
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
