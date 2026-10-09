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
//! - Network fetches the page itself asks for are not blocked: the
//!   output must match what the browser shows. Remote `http(s)://` *inputs*
//!   are not accepted — only local files reach this converter.

use std::path::Path;
use std::process::Command;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use super::process::run_until;
use super::{file_url, WorkDir};
use crate::engine::EngineError;

/// Extensions rendered by the browser directly.
pub const HTML_EXTS: &[&str] = &["html", "htm"];

/// The browser's argument vector for printing `page_url` to `output`. Pure.
///
/// Both spellings of "no header/footer" are passed: Chrome ≥ 112 understands
/// `--no-pdf-header-footer`, older builds `--print-to-pdf-no-header`, and each
/// ignores the flag it does not know.
pub fn browser_args(user_data_dir: &Path, output: &Path, page_url: &str) -> Vec<String> {
    vec![
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
        format!("--print-to-pdf={}", output.display()),
        page_url.to_string(),
    ]
}

/// Render the local HTML file `page` with the browser at `browser` into
/// `output`.
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
