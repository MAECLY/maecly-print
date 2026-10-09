//! Office, OpenDocument, TXT and RTF → PDF through LibreOffice headless
//! (FR-053, FR-054, FR-057).
//!
//! ```text
//! soffice -env:UserInstallation=file:///<work>/profile --headless --invisible
//!         --nologo --nodefault --nolockcheck --norestore
//!         --convert-to pdf --outdir <work>/out <work>/input.<ext>
//! ```
//!
//! Two details make this safe to run on a real desktop:
//!
//! - **A private profile per conversion.** LibreOffice allows one process per
//!   user profile; without `-env:UserInstallation` a headless convert either
//!   hands the file to the user's open LibreOffice window (and exits without
//!   output) or blocks on its lock. A throwaway profile inside the job's work
//!   directory isolates every conversion, including parallel ones, at the cost
//!   of LibreOffice's first-start time (a few seconds).
//! - **A private copy of the input.** The source is copied to
//!   `input.<ext>` first, so the output name is predictable (`input.pdf`)
//!   whatever Unicode or punctuation the original file name contains, and
//!   LibreOffice never writes a lock file next to the user's document.
//!
//! Plain text is opened with an explicit UTF-8 import filter; without it
//! LibreOffice guesses the encoding from the system locale and mangles
//! accented text on a Windows-1252 machine.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use super::process::run_bounded;
use super::{file_url, WorkDir};
use crate::engine::EngineError;

/// Extensions LibreOffice converts for us. Legacy binary Office formats ride
/// the same path at no extra cost.
pub const OFFICE_EXTS: &[&str] = &[
    "docx", "doc", "xlsx", "xls", "pptx", "ppt", "odt", "ods", "odp", "rtf", "txt",
];

/// The argument vector for one conversion (everything after the program).
/// Pure, so the flags are pinned by a unit test rather than by reading code.
pub fn soffice_args(profile_dir: &Path, out_dir: &Path, input: &Path, ext: &str) -> Vec<String> {
    let mut args = vec![
        format!("-env:UserInstallation={}", file_url(profile_dir)),
        "--headless".to_string(),
        "--invisible".to_string(),
        "--nologo".to_string(),
        "--nodefault".to_string(),
        "--nolockcheck".to_string(),
        "--norestore".to_string(),
    ];
    if ext == "txt" {
        // Filter options: charset, line ending, font (default), language.
        args.push("--infilter=Text (encoded):UTF8,LF,,,".to_string());
    }
    args.extend([
        "--convert-to".to_string(),
        "pdf".to_string(),
        "--outdir".to_string(),
        out_dir.to_string_lossy().into_owned(),
        input.to_string_lossy().into_owned(),
    ]);
    args
}

/// Convert `input` with the LibreOffice at `soffice`, writing the PDF to
/// `output`. `work` is the job's scratch directory (deleted by the caller).
pub fn convert_with(
    soffice: &Path,
    input: &Path,
    output: &Path,
    work: &WorkDir,
    cancel: &AtomicBool,
    timeout: Option<Duration>,
) -> Result<(), EngineError> {
    let ext = crate::engine::extension_of(input);
    let staged = work.path().join(format!("input.{ext}"));
    std::fs::copy(input, &staged).map_err(|e| {
        EngineError::ConversionFailed(format!("cannot read {}: {e}", input.display()))
    })?;
    let profile = work.path().join("profile");
    let out_dir = work.path().join("out");
    std::fs::create_dir_all(&out_dir).map_err(|e| {
        EngineError::ConversionFailed(format!("cannot prepare the conversion folder: {e}"))
    })?;

    let mut cmd = Command::new(soffice);
    cmd.args(soffice_args(&profile, &out_dir, &staged, &ext))
        .current_dir(work.path());
    run_bounded(
        cmd,
        "LibreOffice",
        cancel,
        timeout,
        &work.path().join("soffice.log"),
    )?;

    // soffice exits 0 even when it could not load the document ("Error:
    // source file could not be loaded"), so the output file is the real
    // verdict.
    let produced: PathBuf = out_dir.join("input.pdf");
    super::take_output(
        &produced,
        output,
        "LibreOffice",
        &work.path().join("soffice.log"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fr_053_soffice_args_use_a_private_profile_and_headless_convert() {
        let args = soffice_args(
            Path::new("/w/profile"),
            Path::new("/w/out"),
            Path::new("/w/input.docx"),
            "docx",
        );
        assert_eq!(args[0], "-env:UserInstallation=file:///w/profile");
        assert!(args.contains(&"--headless".to_string()));
        assert!(args.contains(&"--norestore".to_string()));
        let tail: Vec<&str> = args
            .iter()
            .rev()
            .take(5)
            .rev()
            .map(String::as_str)
            .collect();
        assert_eq!(
            tail,
            ["--convert-to", "pdf", "--outdir", "/w/out", "/w/input.docx"]
        );
        assert!(!args.iter().any(|a| a.starts_with("--infilter")));
    }

    #[test]
    fn fr_057_txt_is_imported_as_utf8() {
        let args = soffice_args(
            Path::new("/w/p"),
            Path::new("/w/o"),
            Path::new("/w/input.txt"),
            "txt",
        );
        assert!(args.contains(&"--infilter=Text (encoded):UTF8,LF,,,".to_string()));
    }
}
