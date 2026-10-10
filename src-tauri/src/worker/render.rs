//! Worker kind `render-pdf`: an HTML page → PDF through the system web view
//! (FR-055, D15), driven by [`crate::render`].
//!
//! **Stub (round-3 contract, owned by lane L4).** The argument vector and the
//! exit codes follow [`super`]; until L4 lands, the worker reports that it
//! cannot render.

use std::ffi::OsString;

/// Run the render worker with its own arguments; returns the exit code.
pub fn run(args: &[OsString]) -> i32 {
    let _ = args;
    eprintln!("pluryprint worker render-pdf: not implemented yet");
    super::EXIT_FAILED
}
