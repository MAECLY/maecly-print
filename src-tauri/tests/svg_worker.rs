//! FR-052a / D10: an SVG is converted by the real app executable running as a
//! hidden worker (`pluryprint --worker svg <in> <out>`), not in the app's own
//! process, and its failures come back with their stable reasons.

use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use pluryprint_lib::engine::convert::svg::{
    convert_with_worker, convert_with_worker_capped, WorkerCaps,
};
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

/// A drawing inside every pre-check (one filter region of about 50 Mpx,
/// under the 64 Mpx raster cap) that still needs ~200 MB to rasterise: the
/// kind of work only the worker's memory cap can bound.
fn heavy_filter_svg() -> String {
    // The filter region is 120 % of the shape, rasterised at 1.5 (svg2pdf):
    // 3900 × 1.2 × 1.5 = 7020 px a side ≈ 49.3 Mpx, ~197 MB as RGBA.
    r#"<svg xmlns="http://www.w3.org/2000/svg" width="3900" height="3900">
        <filter id="f"><feFlood flood-color="red"/><feGaussianBlur stdDeviation="40"/></filter>
        <rect width="3900" height="3900" filter="url(#f)"/></svg>"#
        .to_string()
}

/// Security note 8 (round 6, lane B): the worker process is the boundary. A
/// drawing that slips past every pre-check but needs more memory than the
/// worker's cap is stopped (Windows: the Job Object refuses the allocation;
/// macOS/Linux: the memory watchdog kills it) and the job ends with the
/// terminal `svg_too_large`, naming the cap. No PDF is left behind.
#[test]
fn nfr_security_svg_worker_memory_cap_stops_a_bomb() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("heavy.svg");
    std::fs::write(&input, heavy_filter_svg()).unwrap();
    let output = dir.path().join("heavy.pdf");
    let cancel = AtomicBool::new(false);
    let caps = WorkerCaps {
        memory: 128 * 1024 * 1024,
    };
    let started = std::time::Instant::now();
    let err = convert_with_worker_capped(
        app(),
        &input,
        &output,
        &cancel,
        Some(Duration::from_secs(300)),
        caps,
    )
    .unwrap_err();
    assert!(
        matches!(&err, EngineError::ConversionFailed(f)
            if f.reason == Some("svg_too_large") && f.params["limit"] == "128"),
        "{err:?}"
    );
    assert!(!output.exists());
    eprintln!("stopped for memory after {:?}", started.elapsed());

    // The same worker under the same cap converts an ordinary drawing: the
    // cap does not get in the way of normal work.
    let small = dir.path().join("small.svg");
    std::fs::write(&small, RECT).unwrap();
    convert_with_worker_capped(
        app(),
        &small,
        &dir.path().join("small.pdf"),
        &cancel,
        Some(Duration::from_secs(60)),
        caps,
    )
    .unwrap();
}

/// Security note 8: a worker that runs past the conversion timeout is
/// killed and the job ends with the terminal `tool_timed_out`.
#[test]
fn nfr_security_svg_worker_timeout_is_terminal() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("slow.svg");
    std::fs::write(&input, heavy_filter_svg()).unwrap();
    let cancel = AtomicBool::new(false);
    let err = convert_with_worker_capped(
        app(),
        &input,
        &dir.path().join("slow.pdf"),
        &cancel,
        Some(Duration::from_millis(300)),
        WorkerCaps::default(),
    )
    .unwrap_err();
    assert!(
        matches!(&err, EngineError::ConversionTimedOut(f) if f.reason == Some("tool_timed_out")),
        "{err:?}"
    );
}
