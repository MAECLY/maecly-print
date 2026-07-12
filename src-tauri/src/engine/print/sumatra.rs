//! SumatraPDF backend: `-print-settings` string construction and locating (pure,
//! all platforms) plus, on Windows, spooling.
//!
//! Flag mapping (`docs/03-requirements/print-settings.md` §SumatraPDF table).
//! The `-print-settings` value is a comma-separated keyword list; SumatraPDF is
//! order-independent, but we emit a fixed order for a stable, testable preview.
//!
//! | Setting               | Token(s)                                   |
//! |-----------------------|--------------------------------------------|
//! | page ranges           | bare `1-5,10` (prepended, no key)          |
//! | duplex none/long/short| `simplex` / `duplex` / `duplexshort`       |
//! | colour / monochrome   | `color` / `monochrome`                     |
//! | scaling fit/shrink/none| `fit` / `shrink` / `noscale`              |
//! | paper                 | `paper=A4\|Letter\|Legal\|A3`              |
//! | copies > 1            | `copies=<n>`                               |
//! | orientation land/port | `landscape` / `portrait` *(auto omitted)*  |
//!
//! The doc table lists only `duplex`/`duplexshort`; SumatraPDF also accepts
//! `simplex` and `noscale`, which we use for the "none" cases so every setting
//! is stated explicitly. Copies use the doc's `copies=N` (not the older `Nx`).

use std::path::{Path, PathBuf};

use crate::engine::EngineError;
use crate::queue::job::{ColorMode, Duplex, JobSettings, Orientation, PaperSize, Scaling};

#[cfg(windows)]
use std::process::Command;

/// Build SumatraPDF's `-print-settings` value from `settings`.
pub fn print_settings_string(settings: &JobSettings) -> String {
    let mut parts: Vec<String> = Vec::new();

    // Page ranges first, as bare comma-separated tokens (SumatraPDF takes them
    // with no key). Whitespace-stripped so the join stays clean.
    if let Some(ranges) = &settings.page_ranges {
        let compact = strip_ws(ranges);
        if !compact.is_empty() {
            parts.push(compact);
        }
    }

    parts.push(
        match settings.duplex {
            Duplex::None => "simplex",
            Duplex::Long => "duplex",
            Duplex::Short => "duplexshort",
        }
        .to_string(),
    );

    parts.push(
        match settings.color_mode {
            ColorMode::Color => "color",
            ColorMode::Monochrome => "monochrome",
        }
        .to_string(),
    );

    parts.push(
        match settings.scaling {
            Scaling::Fit => "fit",
            Scaling::Shrink => "shrink",
            Scaling::None => "noscale",
        }
        .to_string(),
    );

    parts.push(format!("paper={}", paper(settings.paper_size)));

    if settings.copies > 1 {
        parts.push(format!("copies={}", settings.copies));
    }

    match settings.orientation {
        Orientation::Landscape => parts.push("landscape".to_string()),
        Orientation::Portrait => parts.push("portrait".to_string()),
        Orientation::Auto => {}
    }

    parts.join(",")
}

/// Full SumatraPDF argument vector:
/// `-print-to <printer> -print-settings <string> -silent -exit-when-done <pdf>`.
pub fn sumatra_args(printer: &str, settings: &JobSettings, pdf: &Path) -> Vec<String> {
    vec![
        "-print-to".to_string(),
        printer.to_string(),
        "-print-settings".to_string(),
        print_settings_string(settings),
        "-silent".to_string(),
        "-exit-when-done".to_string(),
        pdf.to_string_lossy().into_owned(),
    ]
}

fn paper(paper: PaperSize) -> &'static str {
    match paper {
        PaperSize::A4 => "A4",
        PaperSize::Letter => "Letter",
        PaperSize::Legal => "Legal",
        PaperSize::A3 => "A3",
    }
}

fn strip_ws(s: &str) -> String {
    s.split_whitespace().collect()
}

/// Locate `SumatraPDF.exe`. Search order: next to our own executable
/// (`bin/`, alongside, `resources/`), then each `PATH` entry, then
/// `%LOCALAPPDATA%\SumatraPDF\`.
#[cfg(windows)]
pub fn locate() -> Result<PathBuf, EngineError> {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            for candidate in [
                dir.join("bin").join("SumatraPDF.exe"),
                dir.join("SumatraPDF.exe"),
                dir.join("resources").join("SumatraPDF.exe"),
            ] {
                if candidate.is_file() {
                    return Ok(candidate);
                }
            }
        }
    }

    if let Ok(path) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path) {
            let candidate = dir.join("SumatraPDF.exe");
            if candidate.is_file() {
                return Ok(candidate);
            }
        }
    }

    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        let candidate = PathBuf::from(local)
            .join("SumatraPDF")
            .join("SumatraPDF.exe");
        if candidate.is_file() {
            return Ok(candidate);
        }
    }

    Err(EngineError::DependencyMissing(
        "SumatraPDF.exe not found — install SumatraPDF or place it next to the app".to_string(),
    ))
}

/// On non-Windows platforms SumatraPDF is not used (CUPS handles printing).
#[cfg(not(windows))]
pub fn locate() -> Result<PathBuf, EngineError> {
    Err(EngineError::DependencyMissing(
        "SumatraPDF is only used on Windows".to_string(),
    ))
}

/// Spool `pdf` via SumatraPDF, hiding the child's console window. SumatraPDF
/// prints synchronously and reports no OS job id, so `os_job_id` is `None`.
/// `title` is accepted for parity with the CUPS backend but SumatraPDF has no
/// job-title flag, so it is not forwarded.
#[cfg(windows)]
pub fn spool(
    pdf: &Path,
    printer: &str,
    settings: &JobSettings,
    title: &str,
) -> Result<super::SpoolOutcome, EngineError> {
    use std::os::windows::process::CommandExt;

    // Suppress the transient console window when spawning the child.
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let _ = title;

    let exe = locate()?;
    let args = sumatra_args(printer, settings, pdf);
    let status = Command::new(exe)
        .args(&args)
        .creation_flags(CREATE_NO_WINDOW)
        .status()
        .map_err(|e| EngineError::PrintFailed(format!("failed to run SumatraPDF: {e}")))?;

    if status.success() {
        Ok(super::SpoolOutcome { os_job_id: None })
    } else {
        Err(EngineError::PrintFailed(format!(
            "SumatraPDF exited with {status}"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn strs(v: &[String]) -> Vec<&str> {
        v.iter().map(String::as_str).collect()
    }

    #[test]
    fn defaults_string_is_explicit() {
        assert_eq!(
            print_settings_string(&JobSettings::default()),
            "simplex,color,fit,paper=A4"
        );
    }

    #[test]
    fn full_string_order_and_tokens() {
        let settings = JobSettings {
            printer: None,
            duplex: Duplex::Long,
            color_mode: ColorMode::Monochrome,
            copies: 2,
            paper_size: PaperSize::A3,
            orientation: Orientation::Landscape,
            scaling: Scaling::None,
            page_ranges: Some("1-5, 10".to_string()),
        };
        assert_eq!(
            print_settings_string(&settings),
            "1-5,10,duplex,monochrome,noscale,paper=A3,copies=2,landscape"
        );
    }

    #[test]
    fn short_edge_portrait_shrink() {
        let settings = JobSettings {
            duplex: Duplex::Short,
            orientation: Orientation::Portrait,
            scaling: Scaling::Shrink,
            paper_size: PaperSize::Letter,
            ..JobSettings::default()
        };
        assert_eq!(
            print_settings_string(&settings),
            "duplexshort,color,shrink,paper=Letter,portrait"
        );
    }

    #[test]
    fn args_wrap_the_settings_string() {
        let args = sumatra_args(
            "HP LaserJet",
            &JobSettings::default(),
            Path::new("C:/x.pdf"),
        );
        assert_eq!(
            strs(&args),
            vec![
                "-print-to",
                "HP LaserJet",
                "-print-settings",
                "simplex,color,fit,paper=A4",
                "-silent",
                "-exit-when-done",
                "C:/x.pdf",
            ]
        );
    }

    #[cfg(not(windows))]
    #[test]
    fn locate_is_dependency_missing_off_windows() {
        assert!(matches!(locate(), Err(EngineError::DependencyMissing(_))));
    }
}
