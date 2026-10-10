//! Worker kind `svg`: an SVG/SVGZ drawing → PDF in a child process (FR-052a,
//! D10), so a panic in the SVG stack cannot take the app down.
//!
//! `pluryprint --worker svg <input> <output>`: converts with
//! [`crate::engine::convert::svg::convert_in_process`] and exits
//! [`super::EXIT_OK`]. On failure it prints one
//! [`crate::engine::convert::svg::report_failure`] line on stderr (the stable
//! reason, its params and the English detail) and exits
//! [`super::EXIT_FAILED`]; the parent turns that line back into the same
//! failure. Nothing else is printed: the paths are the parent's to log.

use std::ffi::OsString;
use std::path::PathBuf;

use crate::engine::convert::svg;
use crate::engine::{EngineError, Failure};

/// Run the SVG worker with its own arguments; returns the exit code.
pub fn run(args: &[OsString]) -> i32 {
    let [input, output] = args else {
        eprintln!("usage: pluryprint --worker svg <input.svg> <output.pdf>");
        return super::EXIT_USAGE;
    };
    let (input, output) = (PathBuf::from(input), PathBuf::from(output));
    match svg::convert_in_process(&input, &output) {
        Ok(()) => super::EXIT_OK,
        Err(e) => {
            let failure = match e {
                EngineError::ConversionFailed(f) => f,
                other => Failure::new("svg_convert_failed", other.to_string()),
            };
            eprintln!("{}", svg::report_failure(&failure));
            super::EXIT_FAILED
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// D10: the worker converts, reports a failure on stderr with its reason,
    /// and refuses a malformed command line.
    #[test]
    fn nfr_worker_svg_exit_codes() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("a.svg");
        std::fs::write(
            &input,
            br#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"/>"#,
        )
        .unwrap();
        let output = dir.path().join("a.pdf");
        let args = [
            input.clone().into_os_string(),
            output.clone().into_os_string(),
        ];
        assert_eq!(run(&args), super::super::EXIT_OK);
        assert!(output.is_file());

        std::fs::write(&input, b"nope").unwrap();
        assert_eq!(run(&args), super::super::EXIT_FAILED);

        assert_eq!(run(&args[..1]), super::super::EXIT_USAGE);
    }
}
