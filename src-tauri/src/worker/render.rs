//! Worker kind `render-pdf`: an HTML page → PDF through the system web view
//! (FR-055, D15), driven by [`crate::render`].
//!
//! ```text
//! pluryprint --worker render-pdf --page <stage>/page.html --base <doc folder>
//!            --out <work>/out.pdf --data <work>/webview [--paper-in 8.27x11.69]
//! ```
//!
//! The arguments are a [`RenderRequest`]. Exit codes follow [`super`]: `0`
//! with the PDF written, `1` with the reason on stderr (the engine quotes its
//! tail), `2` for arguments it does not understand. The engine bounds the run
//! with the conversion timeout and the job's cancel flag and kills the whole
//! process tree when either fires, web view helpers included.
//!
//! Before it reads the page it applies the memory cap its parent passed
//! ([`crate::engine::convert::process::apply_memory_cap`]), as the SVG worker
//! does: on Windows it waits until it is inside the parent's Job Object,
//! whose limit covers the web view's processes too.

use std::ffi::OsString;

use crate::render::{self, RenderRequest};

/// Run the render worker with its own arguments; returns the exit code.
pub fn run(args: &[OsString]) -> i32 {
    let request = match RenderRequest::parse(args) {
        Ok(request) => request,
        Err(message) => {
            eprintln!("pluryprint worker render-pdf: {message}");
            return super::EXIT_USAGE;
        }
    };
    // ADR-0014: the memory cap is in force before the page is read.
    if let Err(e) = crate::engine::convert::process::apply_memory_cap() {
        eprintln!("pluryprint worker render-pdf: could not apply the memory limit: {e}");
        return super::EXIT_FAILED;
    }
    match render::render(&request) {
        Ok(()) => super::EXIT_OK,
        Err(message) => {
            eprintln!("pluryprint worker render-pdf: {message}");
            super::EXIT_FAILED
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nfr_worker_render_refuses_bad_arguments_with_the_usage_code() {
        assert_eq!(run(&[]), super::super::EXIT_USAGE);
        assert_eq!(
            run(&["--page".into(), "a.html".into()]),
            super::super::EXIT_USAGE
        );
    }
}
