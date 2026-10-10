//! HTML → PDF through the operating system's own web view (FR-055, D15):
//! WebView2 `PrintToPdf` on Windows, `WKWebView` printing on macOS. Runs only
//! inside the render worker ([`crate::worker::render`]), never in the app or
//! the CLI process itself, so a hostile page that crashes or wedges the web
//! view takes the worker down, not Pluryprint.
//!
//! The engine ([`crate::engine::convert::html`]) stages the page it wants
//! printed (the user's HTML, or the HTML made from Markdown or text) in the
//! job's work directory and hands the worker a [`RenderRequest`]. The worker
//! loads that page with:
//!
//! - **no script**: the web view's own switch (`IsScriptEnabled = false`,
//!   `allowsContentJavaScript = false`), on top of the page's
//!   `script-src 'none'` policy;
//! - **no network**: every request that is not the page's own folder or a
//!   `data:` URI is refused before it leaves the process (a WebView2
//!   `WebResourceRequested` filter; a `WKContentRuleList` on macOS), and so is
//!   any navigation away from the page (a `<meta http-equiv=refresh>`);
//! - **no file share**: a reference such as `//host/x.png` or
//!   `file://host/share/x.png` would make Windows open `\\host\share` over SMB
//!   or WebDAV and hand the user's NTLM credentials to that host. On Windows
//!   the page is therefore not loaded from `file:` at all: its folder is
//!   mapped to a private `https` origin ([`DOC_HOST`]), from which Chromium
//!   refuses every `file:` sub-resource, and a protocol-relative reference
//!   resolves to `https:`, which the request filter refuses. On macOS a
//!   `file:` URL with a host is refused by the rule list (and macOS does not
//!   mount shares for a web view anyway).
//! - **only the page's folder readable**: WebView2 serves the mapped folder
//!   and nothing else; WebKit gets read access to the base folder only.
//!
//! The page size comes from the job ([`RenderRequest::paper_in`]) and is set
//! natively on the print settings, so it does not depend on the page's CSS.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(windows)]
pub mod windows;

/// The private origin a page's own folder is served from on Windows. A
/// reserved name (RFC 2606), so it can never be a real site; WebView2 maps it
/// to the folder without any network or DNS.
pub const DOC_HOST: &str = "doc.pluryprint.example";

/// The private origin the staged page itself is served from on Windows (the
/// job's work folder for the page, separate from the document's folder).
pub const STAGE_HOST: &str = "stage.pluryprint.example";

/// What one render worker run does: print `page` to `output`.
#[derive(Debug, Clone, PartialEq)]
pub struct RenderRequest {
    /// The staged HTML page to load. It lives alone in its own folder, which
    /// is served (Windows) or readable (macOS) for the run.
    pub page: PathBuf,
    /// The folder relative references resolve against (the user's document's
    /// folder); the only other folder the page may read from.
    pub base_dir: PathBuf,
    /// Where the PDF is written.
    pub output: PathBuf,
    /// A private, throwaway folder for the web view's own data (profile,
    /// caches), inside the job's work directory.
    pub data_dir: PathBuf,
    /// Paper width × height in inches, or `None` for the web view's default
    /// (US Letter) when the job's paper is `auto` or unknown.
    pub paper_in: Option<(f64, f64)>,
}

const ARG_PAGE: &str = "--page";
const ARG_BASE: &str = "--base";
const ARG_OUT: &str = "--out";
const ARG_DATA: &str = "--data";
const ARG_PAPER: &str = "--paper-in";

impl RenderRequest {
    /// The worker's argument vector for this request (after
    /// `--worker render-pdf`). Paths travel as `OsString`, never through a
    /// shell, so any file name survives.
    pub fn to_args(&self) -> Vec<OsString> {
        let mut args = vec![
            OsString::from(ARG_PAGE),
            self.page.clone().into_os_string(),
            OsString::from(ARG_BASE),
            self.base_dir.clone().into_os_string(),
            OsString::from(ARG_OUT),
            self.output.clone().into_os_string(),
            OsString::from(ARG_DATA),
            self.data_dir.clone().into_os_string(),
        ];
        if let Some((w, h)) = self.paper_in {
            args.push(OsString::from(ARG_PAPER));
            args.push(OsString::from(format!("{w}x{h}")));
        }
        args
    }

    /// Parse the worker's arguments back into a request. Every path is
    /// required; the paper is optional and must be two positive numbers.
    pub fn parse(args: &[OsString]) -> Result<Self, String> {
        let mut page = None;
        let mut base = None;
        let mut out = None;
        let mut data = None;
        let mut paper = None;
        let mut it = args.iter();
        while let Some(flag) = it.next() {
            let flag = flag.to_string_lossy();
            let value = it
                .next()
                .ok_or_else(|| format!("{flag} needs a value"))?
                .as_os_str();
            match flag.as_ref() {
                ARG_PAGE => page = Some(PathBuf::from(value)),
                ARG_BASE => base = Some(PathBuf::from(value)),
                ARG_OUT => out = Some(PathBuf::from(value)),
                ARG_DATA => data = Some(PathBuf::from(value)),
                ARG_PAPER => paper = Some(parse_paper(value)?),
                other => return Err(format!("unknown argument: {other}")),
            }
        }
        let need = |v: Option<PathBuf>, name: &str| v.ok_or_else(|| format!("{name} is required"));
        Ok(Self {
            page: need(page, ARG_PAGE)?,
            base_dir: need(base, ARG_BASE)?,
            output: need(out, ARG_OUT)?,
            data_dir: need(data, ARG_DATA)?,
            paper_in: paper,
        })
    }
}

fn parse_paper(value: &OsStr) -> Result<(f64, f64), String> {
    let text = value.to_string_lossy();
    let parsed = text.split_once('x').and_then(|(w, h)| {
        let w = w.parse::<f64>().ok()?;
        let h = h.parse::<f64>().ok()?;
        (w.is_finite() && h.is_finite() && w > 0.0 && h > 0.0).then_some((w, h))
    });
    parsed.ok_or_else(|| format!("{ARG_PAPER} must be <width>x<height> in inches, got {text}"))
}

/// The `<base href>` a staged page uses so its relative references resolve
/// against `base_dir` in the web view that prints it: the private
/// [`DOC_HOST`] origin on Windows (module docs), the folder's `file:` URL
/// elsewhere. Always ends with `/`.
pub fn base_href(base_dir: &Path) -> String {
    if cfg!(windows) {
        let _ = base_dir;
        format!("https://{DOC_HOST}/")
    } else {
        let mut url = crate::engine::convert::file_url(base_dir);
        if !url.ends_with('/') {
            url.push('/');
        }
        url
    }
}

/// Whether a request the page makes may be served: a `data:` URI or one of
/// the private origins of this run. Everything else (`http`, `https` to any
/// other host, `ws`, `ftp`, `file:` with or without a host, custom schemes)
/// is refused. Case-insensitive on the scheme and host. Pure; used by the
/// Windows request filter and pinned by tests on every OS.
pub fn is_allowed_request(uri: &str) -> bool {
    let lower = uri.trim().to_ascii_lowercase();
    if lower.starts_with("data:") || lower == "about:blank" {
        return true;
    }
    [DOC_HOST, STAGE_HOST].iter().any(|host| {
        let prefix = format!("https://{host}");
        lower
            .strip_prefix(&prefix)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
    })
}

/// The content rule list (WebKit's JSON) that blocks every load except
/// `data:` URIs and host-less `file:` URLs, so nothing reaches the network or
/// a file share (module docs). Pure.
pub fn webkit_rule_list() -> &'static str {
    r#"[
  {"trigger": {"url-filter": ".*"}, "action": {"type": "block"}},
  {"trigger": {"url-filter": "^file:///"}, "action": {"type": "ignore-previous-rules"}},
  {"trigger": {"url-filter": "^file:////"}, "action": {"type": "block"}},
  {"trigger": {"url-filter": "^data:"}, "action": {"type": "ignore-previous-rules"}},
  {"trigger": {"url-filter": "^about:blank"}, "action": {"type": "ignore-previous-rules"}}
]"#
}

/// The text encoding a staged page is declared in, for a web view that needs
/// it named up front (`WKWebView loadData`): a byte-order mark first, then a
/// `charset` in the first kilobyte, then UTF-8 when the bytes are valid
/// UTF-8, else Windows-1252 (what a browser assumes for Western text). Pure.
pub fn sniff_charset(html: &[u8]) -> String {
    if html.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return "utf-8".into();
    }
    if html.starts_with(&[0xFF, 0xFE]) {
        return "utf-16le".into();
    }
    if html.starts_with(&[0xFE, 0xFF]) {
        return "utf-16be".into();
    }
    let head: Vec<u8> = html.iter().take(1024).map(u8::to_ascii_lowercase).collect();
    if let Some(at) = head.windows(8).position(|w| w == b"charset=") {
        let name: String = head[at + 8..]
            .iter()
            .skip_while(|b| matches!(b, b'"' | b'\'' | b' '))
            .take_while(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b':' | b'.'))
            .map(|&b| b as char)
            .collect();
        if !name.is_empty() {
            return name;
        }
    }
    if std::str::from_utf8(html).is_ok() {
        "utf-8".into()
    } else {
        "windows-1252".into()
    }
}

/// Render `request` with this platform's web view. Must run on the process'
/// main thread (both web views need it). `Err` carries a technical message
/// for the worker's stderr.
pub fn render(request: &RenderRequest) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        macos::render(request)
    }
    #[cfg(windows)]
    {
        windows::render(request)
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        let _ = request;
        Err("rendering HTML is supported on Windows and macOS only".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(paper: Option<(f64, f64)>) -> RenderRequest {
        RenderRequest {
            page: PathBuf::from("/w/stage/page é.html"),
            base_dir: PathBuf::from("/docs/my notes"),
            output: PathBuf::from("/w/out.pdf"),
            data_dir: PathBuf::from("/w/webview"),
            paper_in: paper,
        }
    }

    #[test]
    fn fr_055_render_request_round_trips_through_the_worker_args() {
        for paper in [None, Some((8.27, 11.69)), Some((11.0, 8.5))] {
            let r = req(paper);
            assert_eq!(RenderRequest::parse(&r.to_args()), Ok(r));
        }
    }

    #[test]
    fn fr_055_render_request_refuses_incomplete_or_bad_args() {
        let mut args = req(None).to_args();
        args.truncate(6); // no --data
        assert!(RenderRequest::parse(&args).unwrap_err().contains("--data"));
        let mut args = req(None).to_args();
        args.push("--paper-in".into());
        args.push("0x11".into());
        assert!(RenderRequest::parse(&args).is_err());
        let mut args = req(None).to_args();
        args.push("--paper-in".into());
        assert!(RenderRequest::parse(&args).is_err(), "flag without value");
        assert!(RenderRequest::parse(&["--evil".into(), "x".into()]).is_err());
    }

    /// The security finding: no request leaves for the network or a file
    /// share, whatever form the reference takes.
    #[test]
    fn fr_055_remote_resources_blocked() {
        for uri in [
            "http://example.com/x.png",
            "https://example.com/x.png",
            "HTTPS://Example.com/",
            "ws://127.0.0.1:9/",
            "ftp://host/x",
            "file://host/share/x.png",
            "file:////host/share/x.png",
            "file:///C:/Users/me/secret.png",
            "\\\\host\\share\\x.png",
            "https://doc.pluryprint.example.evil.com/x",
            "https://doc.pluryprint.example@evil.com/x",
            "https://evil.com/https://doc.pluryprint.example/",
            "javascript:alert(1)",
            "blob:https://x/y",
        ] {
            assert!(!is_allowed_request(uri), "{uri} must be refused");
        }
        for uri in [
            "https://doc.pluryprint.example/img/a.png",
            "https://DOC.pluryprint.example/a%20b.css",
            "https://stage.pluryprint.example/page.html",
            "https://stage.pluryprint.example",
            "data:image/png;base64,AAAA",
            "about:blank",
        ] {
            assert!(is_allowed_request(uri), "{uri} must be served");
        }
    }

    #[test]
    fn fr_055_webkit_rule_list_blocks_everything_but_local_files_and_data() {
        let rules: serde_json::Value = serde_json::from_str(webkit_rule_list()).unwrap();
        let rules = rules.as_array().unwrap();
        assert_eq!(rules[0]["trigger"]["url-filter"], ".*");
        assert_eq!(rules[0]["action"]["type"], "block");
        // A file URL with a host (file:////host) is blocked again after the
        // host-less file exemption.
        let filters: Vec<&str> = rules
            .iter()
            .map(|r| r["trigger"]["url-filter"].as_str().unwrap())
            .collect();
        let block_unc = filters.iter().position(|f| *f == "^file:////").unwrap();
        let allow_file = filters.iter().position(|f| *f == "^file:///").unwrap();
        assert!(block_unc > allow_file);
        assert!(!filters.iter().any(|f| f.contains("http")));
    }

    #[test]
    fn fr_055_base_href_points_at_the_document_folder() {
        let href = base_href(Path::new(if cfg!(windows) {
            r"C:\docs\my notes"
        } else {
            "/docs/my notes"
        }));
        if cfg!(windows) {
            assert_eq!(href, "https://doc.pluryprint.example/");
        } else {
            assert_eq!(href, "file:///docs/my%20notes/");
        }
    }

    #[test]
    fn fr_057_charset_sniffing() {
        assert_eq!(sniff_charset(b"\xEF\xBB\xBF<p>x"), "utf-8");
        assert_eq!(sniff_charset(&[0xFF, 0xFE, b'<', 0]), "utf-16le");
        assert_eq!(
            sniff_charset(b"<meta charset=\"ISO-8859-1\"><p>caf\xe9"),
            "iso-8859-1"
        );
        assert_eq!(
            sniff_charset(
                b"<meta http-equiv=content-type content='text/html; charset=windows-1252'>"
            ),
            "windows-1252"
        );
        assert_eq!(sniff_charset("<p>café".as_bytes()), "utf-8");
        assert_eq!(sniff_charset(b"<p>caf\xe9"), "windows-1252");
    }
}
