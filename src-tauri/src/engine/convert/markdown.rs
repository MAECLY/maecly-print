//! Markdown → HTML (Pandoc) → PDF (the headless browser of [`super::html`])
//! (FR-056).
//!
//! ```text
//! pandoc --from=markdown-raw_html-raw_attribute --to=html5 --standalone
//!        --sandbox --lua-filter=<work>/local-images.lua
//!        --metadata=pagetitle:<name> --include-in-header=<work>/head.html
//!        --output=<work>/page.html <input.md>
//! ```
//!
//! A Markdown file is text, and printing it must not turn it into active
//! content. Raw HTML (`<script>`, `<iframe src=file:///…>`, `<img
//! src=\\host\share>`) is not passed through: with `raw_html` and
//! `raw_attribute` off, Pandoc prints such markup as literal text.
//! `--sandbox` keeps Pandoc itself from reading anything but the files named
//! on its command line or fetching anything over the network. The Lua filter
//! ([`LOCAL_IMAGES_LUA`]) keeps only images that load from the local disk;
//! a remote or UNC image (`//host/share/x.png`, which Windows would resolve
//! over SMB and so hand the user's NTLM hash to that host) becomes its alt
//! text. The injected header adds a Content-Security-Policy that forbids
//! scripts, plugins and frames, so nothing can run even if a future Pandoc
//! emitted markup we did not expect, and the browser step runs without
//! network ([`super::html::SANDBOX_ARGS`]).
//!
//! Why not `pandoc -o out.pdf`: Pandoc's PDF output needs a LaTeX distribution
//! (or wkhtmltopdf), a multi-gigabyte dependency for the user to install. The
//! browser is already a dependency for HTML and is always present on Windows,
//! so Markdown reuses it and only Pandoc itself is added.
//!
//! The generated HTML lives in the job's work directory, not next to the
//! user's file. A `<base href>` pointing at the source folder (in the injected
//! header) keeps relative image links resolving as they would from the `.md`.
//! The header also carries a small print stylesheet, so the page looks like a
//! document rather than unstyled browser defaults.

use std::path::Path;
use std::process::Command;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use super::process::run_bounded;
use super::{file_url, PageSetup, WorkDir};
use crate::engine::EngineError;

/// Extensions treated as Markdown.
pub const MARKDOWN_EXTS: &[&str] = &["md", "markdown"];

/// Print stylesheet embedded into every converted Markdown page.
const PRINT_CSS: &str = "\
@page { size: auto; margin: 18mm 16mm; }
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

/// Escape `s` for use inside a double-quoted HTML attribute.
fn attr_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Content-Security-Policy for the generated page: a Markdown document has
/// no business running script, embedding plugins or framing other pages.
const PAGE_CSP: &str = "script-src 'none'; object-src 'none'; frame-src 'none'; \
                        worker-src 'none'; connect-src 'none'; form-action 'none'";

/// The `<head>` fragment injected by Pandoc: CSP + base URL + stylesheet,
/// plus the job's paper size when it names one (the browser would otherwise
/// lay the page out on US Letter). Pure.
pub fn head_fragment(source_dir: &Path, page: &PageSetup) -> String {
    let mut base = file_url(source_dir);
    if !base.ends_with('/') {
        base.push('/');
    }
    let size = page
        .css_size
        .as_deref()
        .map(|size| format!("@page {{ size: {size}; }}\n"))
        .unwrap_or_default();
    format!(
        "<meta http-equiv=\"Content-Security-Policy\" content=\"{PAGE_CSP}\">\n\
         <base href=\"{}\">\n<style>\n{PRINT_CSS}{size}</style>\n",
        attr_escape(&base)
    )
}

/// Pandoc Lua filter: an image whose source is not on the local disk is
/// replaced by its alt text. Local means a relative path, a drive path
/// (`C:\x.png`), `file:///…` with no host, or a `data:` URI; a
/// protocol-relative or UNC source (`//host/x`, `\\host\x`,
/// `file://host/x`, `file:////host/x`) and any other scheme are not.
pub const LOCAL_IMAGES_LUA: &str = r#"
local function is_local(src)
  local lower = src:lower()
  if lower:sub(1, 5) == 'data:' then return true end
  if src:match('^[/\\][/\\]') then return false end
  if src:match('^%a:[/\\]') then return true end
  local scheme = src:match('^(%a[%w+.-]*):')
  if scheme == nil then return true end
  if scheme:lower() ~= 'file' then return false end
  return lower:match('^file:///') ~= nil and lower:match('^file:///[/\\]') == nil
end

function Image(img)
  if is_local(img.src) then return nil end
  return img.caption
end
"#;

/// Pandoc's argument vector. Pure.
pub fn pandoc_args(
    input: &Path,
    header: &Path,
    filter: &Path,
    output: &Path,
    title: &str,
) -> Vec<String> {
    vec![
        "--from=markdown-raw_html-raw_attribute".to_string(),
        "--to=html5".to_string(),
        "--standalone".to_string(),
        "--sandbox".to_string(),
        format!("--lua-filter={}", filter.display()),
        format!("--metadata=pagetitle:{title}"),
        format!("--include-in-header={}", header.display()),
        format!("--output={}", output.display()),
        input.to_string_lossy().into_owned(),
    ]
}
/// Convert the Markdown file at `input` into a PDF at `output`, laid out on
/// the job's paper. The route's entry point: it resolves what it needs itself
/// (today Pandoc and a browser; `pulldown-cmark` and the system web view once
/// lane L4 lands).
pub fn convert(
    input: &Path,
    output: &Path,
    work: &WorkDir,
    cancel: &AtomicBool,
    timeout: Option<Duration>,
    page_setup: &PageSetup,
) -> Result<(), EngineError> {
    use crate::engine::deps::Dependency;
    let pandoc = super::locate(Dependency::Pandoc)?;
    let browser = super::locate(Dependency::Chromium)?;
    convert_with(
        &pandoc, &browser, input, output, work, cancel, timeout, page_setup,
    )
}

/// Convert `input` with Pandoc at `pandoc`, then print the page with the
/// browser at `browser` into `output`. The whole pipeline shares one
/// `timeout` budget.
#[allow(clippy::too_many_arguments)]
pub fn convert_with(
    pandoc: &Path,
    browser: &Path,
    input: &Path,
    output: &Path,
    work: &WorkDir,
    cancel: &AtomicBool,
    timeout: Option<Duration>,
    page_setup: &PageSetup,
) -> Result<(), EngineError> {
    let started = std::time::Instant::now();
    let source_dir = input.parent().unwrap_or_else(|| Path::new("."));
    let header = work.path().join("head.html");
    let filter = work.path().join("local-images.lua");
    std::fs::write(&header, head_fragment(source_dir, page_setup))
        .and_then(|()| std::fs::write(&filter, LOCAL_IMAGES_LUA))
        .map_err(|e| {
            EngineError::ConversionFailed(
                format!("cannot prepare the conversion folder: {e}").into(),
            )
        })?;
    let page = work.path().join("page.html");
    let title = input
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "document".to_string());

    let mut cmd = Command::new(pandoc);
    cmd.args(pandoc_args(input, &header, &filter, &page, &title))
        .current_dir(work.path());
    let log = work.path().join("pandoc.log");
    run_bounded(cmd, "Pandoc", cancel, timeout, &log)?;
    if !page.is_file() {
        return Err(EngineError::ConversionFailed(
            "Pandoc finished without producing a page"
                .to_string()
                .into(),
        ));
    }

    let remaining = timeout.map(|t| t.saturating_sub(started.elapsed()));
    if remaining == Some(Duration::ZERO) {
        return Err(EngineError::ConversionFailed(
            "Pandoc used the whole conversion time budget"
                .to_string()
                .into(),
        ));
    }
    // The page already carries the paper size in its header.
    super::html::convert_with(
        browser,
        &page,
        output,
        work,
        cancel,
        remaining,
        &PageSetup::default(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fr_056_pandoc_args_make_standalone_html_with_the_injected_header() {
        let args = pandoc_args(
            Path::new("/docs/notes.md"),
            Path::new("/w/head.html"),
            Path::new("/w/local-images.lua"),
            Path::new("/w/page.html"),
            "notes",
        );
        assert_eq!(
            args,
            [
                "--from=markdown-raw_html-raw_attribute",
                "--to=html5",
                "--standalone",
                "--sandbox",
                "--lua-filter=/w/local-images.lua",
                "--metadata=pagetitle:notes",
                "--include-in-header=/w/head.html",
                "--output=/w/page.html",
                "/docs/notes.md",
            ]
        );
    }

    #[test]
    fn fr_056_header_points_relative_links_at_the_source_folder() {
        let (dir, base) = if cfg!(windows) {
            (r"C:\docs\my notes", "file:///C:/docs/my%20notes/")
        } else {
            ("/docs/my notes", "file:///docs/my%20notes/")
        };
        let head = head_fragment(Path::new(dir), &PageSetup::default());
        assert!(head.contains(&format!("<base href=\"{base}\">")), "{head}");
        assert!(head.contains("@page"));
        assert!(
            head.contains("Content-Security-Policy") && head.contains("script-src 'none'"),
            "{head}"
        );
        assert!(!head.contains("size: 210mm"));

        let a4 = PageSetup {
            css_size: Some("210mm 297mm".to_string()),
        };
        assert!(head_fragment(Path::new(dir), &a4).contains("@page { size: 210mm 297mm; }"));
    }
}
