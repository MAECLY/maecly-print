//! Worker kind `svg`: an SVG/SVGZ drawing → PDF in a child process (FR-052a,
//! D10), so a panic in the SVG stack cannot take the app down.
//!
//! **Stub (round-3 contract, owned by lane L2).** The argument vector and the
//! exit codes follow [`super`]; until L2 lands, the worker reports that it
//! cannot convert.

use std::ffi::OsString;

/// Run the SVG worker with its own arguments; returns the exit code.
pub fn run(args: &[OsString]) -> i32 {
    let _ = args;
    eprintln!("pluryprint worker svg: not implemented yet");
    super::EXIT_FAILED
}
