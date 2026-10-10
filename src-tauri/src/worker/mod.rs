//! The hidden worker process (decision D10).
//!
//! Work that may crash or hang on hostile input — rendering HTML/Markdown/text
//! through the system web view, converting an SVG — runs in a child process of
//! the same executable, `pluryprint --worker <kind> …`, so that a panic (the
//! release profile aborts on panic) or a wedged web view kills the worker, not
//! the app, and a cancel or timeout can kill it outright.
//!
//! `main` checks for the worker branch **first**, before the single-instance
//! plugin, the updater, the tray or any menu exist: a worker is never "a second
//! launch" to forward, never shows UI of its own and has no IPC.
//!
//! The branch and its argument contract are fixed here (lane L0); the kinds are
//! implemented by their lanes: [`render`] by L4 (HTML → PDF, D15), [`svg`] by L2.

use std::ffi::OsString;

pub mod render;
pub mod svg;

/// The flag that selects the worker branch. Always the first argument.
pub const WORKER_FLAG: &str = "--worker";

/// Exit code of a worker that did its job.
pub const EXIT_OK: i32 = 0;
/// Exit code of a worker whose job failed (the reason is on stderr).
pub const EXIT_FAILED: i32 = 1;
/// Exit code of a worker started with arguments it does not understand.
pub const EXIT_USAGE: i32 = 2;

/// What a worker process does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkerKind {
    /// Render an HTML page to PDF through the system web view (FR-055).
    RenderPdf,
    /// Convert an SVG/SVGZ drawing to PDF (FR-052a).
    Svg,
}

impl WorkerKind {
    /// The kind's name on the command line.
    pub fn as_arg(self) -> &'static str {
        match self {
            Self::RenderPdf => "render-pdf",
            Self::Svg => "svg",
        }
    }

    fn parse(name: &str) -> Option<Self> {
        [Self::RenderPdf, Self::Svg]
            .into_iter()
            .find(|k| k.as_arg() == name)
    }
}

/// Whether `argv` (the full argument vector, program name first) asks for a
/// worker, and which. `None`: not a worker launch, carry on as the app or the
/// CLI. `Some(Err(..))`: the worker flag with a missing or unknown kind.
pub fn parse(argv: &[OsString]) -> Option<Result<(WorkerKind, Vec<OsString>), String>> {
    if argv.get(1).and_then(|a| a.to_str()) != Some(WORKER_FLAG) {
        return None;
    }
    let Some(name) = argv.get(2) else {
        return Some(Err(format!("{WORKER_FLAG} needs a kind")));
    };
    let name = name.to_string_lossy();
    Some(match WorkerKind::parse(&name) {
        Some(kind) => Ok((kind, argv[3..].to_vec())),
        None => Err(format!("unknown worker kind: {name}")),
    })
}

/// Run the worker `argv` asks for, if it asks for one, and return its exit
/// code. `None` when this is not a worker launch.
pub fn dispatch(argv: &[OsString]) -> Option<i32> {
    Some(match parse(argv)? {
        Ok((kind, args)) => run(kind, &args),
        Err(message) => {
            eprintln!("pluryprint worker: {message}");
            EXIT_USAGE
        }
    })
}

/// Run one worker of `kind` with its own arguments.
pub fn run(kind: WorkerKind, args: &[OsString]) -> i32 {
    match kind {
        WorkerKind::RenderPdf => render::run(args),
        WorkerKind::Svg => svg::run(args),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(args: &[&str]) -> Vec<OsString> {
        args.iter().map(OsString::from).collect()
    }

    /// D10: the worker branch is recognised from argv alone, and only as the
    /// first argument, so a file named `--worker` in a print selection is
    /// never mistaken for one.
    #[test]
    fn nfr_worker_branch_is_recognised_from_argv_only() {
        assert_eq!(parse(&argv(&["pluryprint"])), None);
        assert_eq!(parse(&argv(&["pluryprint", "print", "--worker"])), None);
        assert_eq!(
            parse(&argv(&[
                "pluryprint",
                "--worker",
                "svg",
                "in.svg",
                "out.pdf"
            ])),
            Some(Ok((WorkerKind::Svg, argv(&["in.svg", "out.pdf"]))))
        );
        assert_eq!(
            parse(&argv(&["pluryprint", "--worker", "render-pdf"])),
            Some(Ok((WorkerKind::RenderPdf, Vec::new())))
        );
        assert!(matches!(
            parse(&argv(&["pluryprint", "--worker", "gif"])),
            Some(Err(_))
        ));
        assert!(matches!(
            parse(&argv(&["pluryprint", "--worker"])),
            Some(Err(_))
        ));
        assert_eq!(
            dispatch(&argv(&["pluryprint", "--worker", "nope"])),
            Some(EXIT_USAGE)
        );
        assert_eq!(dispatch(&argv(&["pluryprint", "list"])), None);
    }
}
