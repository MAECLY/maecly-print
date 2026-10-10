//! Markdown → HTML (`pulldown-cmark`, in-process) → PDF through the page
//! renderer of [`super::html`] (FR-056). No Pandoc, no browser.
//!
//! A Markdown file is text, and printing it must not turn it into active
//! content:
//!
//! - **Raw HTML is dropped** (`Event::Html`, `Event::InlineHtml`): a
//!   `<script>`, an `<iframe src=file:///…>` or an `<img src=\\host\share>`
//!   written in the Markdown never reaches the page. The text between inline
//!   tags stays (`H<sub>2</sub>O` prints `H2O`).
//! - **Only local images are kept** ([`is_local_image`]): a relative path, a
//!   drive path, a host-less `file:///` URL or a `data:` URI. A remote or
//!   UNC source (`http://…`, `//host/x`, `\\host\x`, `file://host/x`) — which
//!   Windows would resolve over SMB and so hand the user's NTLM hash to that
//!   host — is replaced by its alt text.
//! - The page then goes through the same staging as an HTML file (a
//!   Content-Security-Policy that forbids scripts, plugins, frames and
//!   connections; a `<base href>` at the Markdown file's folder, so relative
//!   images resolve as they do next to the `.md`) and the same renderer, with
//!   scripts off and no network.
//!
//! Extensions on: tables, footnotes, strikethrough, task lists and smart
//! punctuation (GitHub-flavoured documents print as they read on GitHub).
//! The page carries a small print stylesheet ([`PRINT_CSS`]) so it looks like
//! a document rather than unstyled browser defaults.

use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use pulldown_cmark::{CowStr, Event, Options, Parser, Tag, TagEnd};

use super::{PageSetup, WorkDir};
use crate::engine::{EngineError, Failure};

/// Extensions treated as Markdown.
pub const MARKDOWN_EXTS: &[&str] = &["md", "markdown"];

/// Print stylesheet embedded into every converted Markdown page. No `@page`
/// rule: the paper and margins come from the job ([`PageSetup`]).
pub const PRINT_CSS: &str = "\
html { font-family: -apple-system, 'Segoe UI', Roboto, 'Helvetica Neue', Arial, sans-serif;
       font-size: 11pt; line-height: 1.5; color: #111; }
body { max-width: none; margin: 0; padding: 0; }
h1, h2, h3, h4 { line-height: 1.25; page-break-after: avoid; }
h1 { font-size: 1.9em; } h2 { font-size: 1.45em; } h3 { font-size: 1.2em; }
pre, code { font-family: 'SF Mono', Consolas, Menlo, monospace; font-size: 0.92em; }
pre { background: #f5f5f5; padding: 8pt; border-radius: 4pt; white-space: pre-wrap; }
table { border-collapse: collapse; } th, td { border: 1px solid #bbb; padding: 3pt 6pt; }
img { max-width: 100%; } blockquote { border-left: 3pt solid #ccc; margin-left: 0; padding-left: 10pt; color: #444; }
pre, blockquote, table, img { page-break-inside: avoid; }
";

/// Whether an image source loads from the local disk. Local means a relative
/// path, a drive path (`C:\x.png`), `file:///…` with no host, or a `data:`
/// URI; a protocol-relative or UNC source (`//host/x`, `\\host\x`,
/// `file://host/x`, `file:////host/x`, and their escaped or slash-mixed
/// spellings) and any other scheme are not. This is the shared predicate of
/// [`super::local_ref`]. Pure.
pub fn is_local_image(src: &str) -> bool {
    super::local_ref::is_local_ref(src)
}

/// Escape text for HTML content.
fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// The whole HTML page for the Markdown `text`, titled `title`. Pure.
pub fn to_html(text: &str, title: &str) -> String {
    let options = Options::ENABLE_TABLES
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_SMART_PUNCTUATION;
    // For each open image: whether it is dropped (its alt text stays).
    let mut images: Vec<bool> = Vec::new();
    let events = Parser::new_ext(text, options).filter_map(|event| match event {
        Event::Html(_) | Event::InlineHtml(_) => None,
        Event::Start(Tag::Image { ref dest_url, .. }) => {
            let drop = !is_local_image(dest_url);
            images.push(drop);
            (!drop).then_some(event)
        }
        Event::End(TagEnd::Image) => {
            let drop = images.pop().unwrap_or(false);
            (!drop).then_some(event)
        }
        // Links are not fetched when printing, but a link to a share is
        // still not something to put on paper as a live target.
        Event::Start(Tag::Link {
            link_type,
            dest_url,
            title,
            id,
        }) if !is_local_image(&dest_url) && !is_web_link(&dest_url) => {
            Some(Event::Start(Tag::Link {
                link_type,
                dest_url: CowStr::Borrowed(""),
                title,
                id,
            }))
        }
        other => Some(other),
    });
    let mut body = String::with_capacity(text.len() * 2);
    pulldown_cmark::html::push_html(&mut body, events);
    format!(
        "<!DOCTYPE html>\n<html><head><meta charset=\"utf-8\"><title>{}</title>\n\
         <style>\n{PRINT_CSS}</style>\n</head><body>\n{body}</body></html>\n",
        escape(title)
    )
}

/// `http(s)` and `mailto` link targets, printed as they are.
fn is_web_link(dest: &str) -> bool {
    let lower = dest.trim().to_ascii_lowercase();
    ["http://", "https://", "mailto:"]
        .iter()
        .any(|p| lower.starts_with(p))
}

/// Convert the Markdown file at `input` into a PDF at `output`, laid out on
/// the job's paper. The route's entry point.
pub fn convert(
    input: &Path,
    output: &Path,
    work: &WorkDir,
    cancel: &AtomicBool,
    timeout: Option<Duration>,
    page: &PageSetup,
) -> Result<(), EngineError> {
    convert_with_renderer(
        input,
        output,
        work,
        cancel,
        timeout,
        page,
        super::html::configured_renderer(),
    )
}

/// [`convert`] with an explicit renderer.
pub fn convert_with_renderer(
    input: &Path,
    output: &Path,
    work: &WorkDir,
    cancel: &AtomicBool,
    timeout: Option<Duration>,
    page: &PageSetup,
    renderer: crate::settings::HtmlRenderer,
) -> Result<(), EngineError> {
    let bytes = std::fs::read(input).map_err(|e| {
        EngineError::ConversionFailed(
            Failure::new(
                "cannot_read_input",
                format!("cannot read {}: {e}", input.display()),
            )
            .param("file", input.display().to_string()),
        )
    })?;
    let text = super::text::decode(&bytes);
    let title = input
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "document".to_string());
    let html = to_html(&text, &title);
    let base_dir = input.parent().unwrap_or_else(|| Path::new("."));
    super::html::render_document(
        html.as_bytes(),
        base_dir,
        output,
        work,
        cancel,
        timeout,
        page,
        renderer,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fr_056_markdown_no_pandoc_raw_html_dropped_remote_img_alt() {
        let md = "# Title\n\n\
             <script>document.write('ran')</script>\n\n\
             <iframe src=\"file:///etc/passwd\"></iframe>\n\n\
             H<sub>2</sub>O and <b>bold</b>\n\n\
             ![keep-rel](pic.png) ![keep-abs](file:///tmp/x.png) \
             ![keep-drive](C:\\pics\\x.png) ![keep-data](data:image/png;base64,AA==)\n\n\
             ![drop-http](http://example.com/x.png) ![drop-proto](//attacker/s/x.png)\n\n\
             ![drop-unc](\\\\\\\\attacker\\\\s\\\\x.png) ![drop-host](file://attacker/x.png)\n\n\
             ![drop-four](file:////attacker/x.png) ![drop-smb](smb://attacker/x.png)\n";
        let html = to_html(md, "README");
        let body = &html[html.find("<body>").unwrap()..];
        assert!(!body.contains("<script"), "{body}");
        assert!(!body.contains("ran')"), "{body}");
        assert!(!body.contains("<iframe"), "{body}");
        assert!(!body.contains("<b>"), "{body}");
        assert!(body.contains("H2O"), "inline text survives: {body}");
        for keep in ["keep-rel", "keep-abs", "keep-drive", "keep-data"] {
            assert!(body.contains(&format!("alt=\"{keep}\"")), "{keep}: {body}");
        }
        for drop in [
            "drop-http",
            "drop-proto",
            "drop-unc",
            "drop-host",
            "drop-four",
            "drop-smb",
        ] {
            assert!(!body.contains(&format!("alt=\"{drop}\"")), "{drop}: {body}");
            assert!(body.contains(drop), "{drop} keeps its alt text: {body}");
        }
        assert!(!body.contains("attacker"), "{body}");
        assert!(!body.contains("example.com"), "{body}");
        assert!(!html.contains("@page"), "the job sets the paper: {html}");
    }

    #[test]
    fn fr_056_gfm_tables_tasks_and_footnotes_render() {
        let html = to_html(
            "| a | b |\n|---|---|\n| 1 | 2 |\n\n- [x] done\n- [ ] todo\n\n~~old~~ text[^1]\n\n[^1]: note\n",
            "t",
        );
        assert!(html.contains("<table>"), "{html}");
        assert!(html.contains("type=\"checkbox\""), "{html}");
        assert!(html.contains("<del>old</del>"), "{html}");
        assert!(html.contains("footnote"), "{html}");
    }

    #[test]
    fn fr_056_links_to_shares_lose_their_target() {
        let html = to_html(
            "[web](https://example.org) [share](file://host/s) [unc](//host/s) [rel](notes.md)",
            "t",
        );
        assert!(html.contains("href=\"https://example.org\""), "{html}");
        assert!(html.contains("href=\"notes.md\""), "{html}");
        assert!(!html.contains("host/s"), "{html}");
    }

    #[test]
    fn fr_056_the_title_is_escaped() {
        let html = to_html("x", "a<b>&\"c");
        assert!(
            html.contains("<title>a&lt;b&gt;&amp;&quot;c</title>"),
            "{html}"
        );
    }

    #[test]
    fn fr_056_local_image_table() {
        for (src, local) in [
            ("pic.png", true),
            ("./img/a b.png", true),
            ("../up.png", true),
            ("/abs/x.png", true),
            ("C:\\x.png", true),
            ("c:/x.png", true),
            ("file:///tmp/x.png", true),
            ("FILE:///C:/x.png", true),
            ("data:image/png;base64,AA==", true),
            ("http://x/y.png", false),
            ("https://x/y.png", false),
            ("//host/x.png", false),
            ("\\\\host\\x.png", false),
            ("/\\host\\x.png", false),
            ("file://host/x.png", false),
            ("file:////host/x.png", false),
            ("file:///\\\\host/x.png", false),
            // Round 4 bypass vectors: escaped and slash-mixed shares.
            ("file:///%5C%5Chost/x.png", false),
            ("%5C%5Chost%5Cx.png", false),
            ("\\/host/x.png", false),
            ("\\\\?\\UNC\\host\\x.png", false),
            ("/\t/host/x.png", false),
            ("smb://host/x.png", false),
            ("javascript:alert(1)", false),
        ] {
            assert_eq!(is_local_image(src), local, "{src}");
        }
    }
}
