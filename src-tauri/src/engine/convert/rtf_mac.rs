//! RTF → PDF on macOS through the system's `textutil` (FR-057), with no
//! LibreOffice.
//!
//! ```text
//! /usr/bin/textutil -convert html -output <work>/rtf/page.html <work>/rtf/input.rtf
//! ```
//!
//! `textutil` is part of macOS (like `sips` for HEIC) and keeps fonts,
//! styles, colours, lists and tables. It runs on a private copy of the input
//! in the job's work folder, bounded by the conversion timeout and the cancel
//! flag like every converter, and its HTML then goes through the page renderer
//! of [`super::html`] (scripts off, no network) with that folder as its base,
//! so any picture `textutil` wrote next to the page resolves and nothing
//! outside it does.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use super::process::run_bounded;
use super::{PageSetup, WorkDir};
use crate::engine::{EngineError, Failure};

/// The system tool.
pub const TEXTUTIL: &str = "/usr/bin/textutil";

/// Whether RTF is converted here instead of by LibreOffice: `textutil` is
/// there (it always is on a supported macOS).
pub fn available() -> bool {
    Path::new(TEXTUTIL).is_file()
}

/// `textutil`'s argument vector. Pure.
pub fn textutil_args(input: &Path, output: &Path) -> Vec<String> {
    vec![
        "-convert".into(),
        "html".into(),
        "-output".into(),
        output.to_string_lossy().into_owned(),
        input.to_string_lossy().into_owned(),
    ]
}

/// Convert the RTF at `input` into a PDF at `output`.
pub fn convert(
    input: &Path,
    output: &Path,
    work: &WorkDir,
    cancel: &AtomicBool,
    timeout: Option<Duration>,
    page: &PageSetup,
) -> Result<(), EngineError> {
    convert_with(
        Path::new(TEXTUTIL),
        input,
        output,
        work,
        cancel,
        timeout,
        page,
    )
}

/// [`convert`] with the `textutil` at `tool`.
pub fn convert_with(
    tool: &Path,
    input: &Path,
    output: &Path,
    work: &WorkDir,
    cancel: &AtomicBool,
    timeout: Option<Duration>,
    page: &PageSetup,
) -> Result<(), EngineError> {
    let started = Instant::now();
    let dir: PathBuf = work.path().join("rtf");
    let staged = dir.join("input.rtf");
    let html_path = dir.join("page.html");
    std::fs::create_dir_all(&dir).map_err(|e| {
        EngineError::ConversionFailed(Failure::new(
            "workdir_failed",
            format!("cannot prepare the conversion folder: {e}"),
        ))
    })?;
    std::fs::copy(input, &staged).map_err(|e| {
        EngineError::ConversionFailed(
            Failure::new(
                "cannot_read_input",
                format!("cannot read {}: {e}", input.display()),
            )
            .param("file", input.display().to_string()),
        )
    })?;
    let mut cmd = Command::new(tool);
    cmd.args(textutil_args(&staged, &html_path))
        .current_dir(&dir);
    let log = work.path().join("textutil.log");
    run_bounded(cmd, "textutil", cancel, timeout, &log)?;
    let html = std::fs::read(&html_path).map_err(|_| {
        EngineError::ConversionFailed(
            Failure::new(
                "converter_no_output",
                "textutil finished without producing a page",
            )
            .param("tool", "textutil"),
        )
    })?;
    // The input copy is not a resource of the page.
    let _ = std::fs::remove_file(&staged);
    let remaining = timeout.map(|t| t.saturating_sub(started.elapsed()));
    super::html::render_document(&html, &dir, output, work, cancel, remaining, page)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fr_057_textutil_args_convert_to_html() {
        assert_eq!(
            textutil_args(Path::new("/w/rtf/input.rtf"), Path::new("/w/rtf/page.html")),
            [
                "-convert",
                "html",
                "-output",
                "/w/rtf/page.html",
                "/w/rtf/input.rtf"
            ]
        );
        assert!(available(), "textutil ships with macOS");
    }
}
