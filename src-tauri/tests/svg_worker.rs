//! FR-052a / D10: an SVG is converted by the real app executable running as a
//! hidden worker (`pluryprint --worker svg <in> <out>`), not in the app's own
//! process, and its failures come back with their stable reasons.

use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use pluryprint_lib::engine::convert::svg::convert_with_worker;
use pluryprint_lib::engine::EngineError;

/// The `pluryprint` binary Cargo built for this test run.
fn app() -> &'static Path {
    Path::new(env!("CARGO_BIN_EXE_pluryprint"))
}

const RECT: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="100">
    <rect x="10" y="10" width="180" height="80" fill="#36c"/></svg>"##;

#[test]
fn fr_052a_svg_converts_in_the_worker_process() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("drawing.svg");
    std::fs::write(&input, RECT).unwrap();
    let output = dir.path().join("drawing.pdf");
    let cancel = AtomicBool::new(false);
    convert_with_worker(
        app(),
        &input,
        &output,
        &cancel,
        Some(Duration::from_secs(60)),
    )
    .unwrap();
    let doc = lopdf::Document::load(&output).unwrap();
    assert_eq!(doc.get_pages().len(), 1);
}

#[test]
fn fr_052a_worker_failure_keeps_its_reason() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("broken.svg");
    std::fs::write(&input, b"this is not an svg").unwrap();
    let output = dir.path().join("broken.pdf");
    let cancel = AtomicBool::new(false);
    let err = convert_with_worker(
        app(),
        &input,
        &output,
        &cancel,
        Some(Duration::from_secs(60)),
    )
    .unwrap_err();
    assert!(
        matches!(&err, EngineError::ConversionFailed(f) if f.reason == Some("svg_invalid")),
        "{err:?}"
    );
    assert!(!output.exists());
}

#[test]
fn fr_052a_worker_is_stopped_on_cancel() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("drawing.svg");
    std::fs::write(&input, RECT).unwrap();
    let cancel = AtomicBool::new(true);
    let err =
        convert_with_worker(app(), &input, &dir.path().join("o.pdf"), &cancel, None).unwrap_err();
    assert!(matches!(err, EngineError::Cancelled), "{err:?}");
}

/// D10: a worker launch with a bad command line is a usage error (exit 2),
/// and never opens the app.
#[test]
fn nfr_worker_svg_usage_error_exits_2() {
    let status = std::process::Command::new(app())
        .args(["--worker", "svg"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(2));
}
