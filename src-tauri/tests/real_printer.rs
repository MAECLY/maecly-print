//! Real-spooler checks for the CUPS option mapping (macOS / Linux only).
//!
//! The unit tests in `engine::print::cups` pin the *argument vector* the
//! engine builds. This suite pins the other half: that CUPS actually turns
//! that vector into the IPP job attributes we meant. It goes through the
//! engine's public [`spool`] entry point — the very function the dispatcher
//! calls — so there is no second copy of the mapping to drift.
//!
//! # Safety: nothing is ever printed
//!
//! Every test is `#[ignore]`d **and** gated on `MAECLY_REAL_PRINTER=1`. Jobs go
//! to the queue named by `MAECLY_HOLD_QUEUE` (default `Maecly_Hold`), which
//! must be a queue whose default `job-hold-until` is `indefinite`: CUPS keeps
//! every job in `pending-held` and never sends it to the device. The suite
//! refuses to submit anything when that default is missing, re-checks that the
//! job really landed in `pending-held`, and cancels it on the way out (also on
//! panic, via a drop guard).
//!
//! Create such a queue once (it may point at any device, held jobs never
//! reach it):
//!
//! ```sh
//! lpadmin -p Maecly_Hold -E -v ipp://<host>/ipp/print -m everywhere \
//!         -o job-hold-until-default=indefinite
//! ```
//!
//! Run with:
//!
//! ```sh
//! MAECLY_REAL_PRINTER=1 cargo test --test real_printer -- --ignored --test-threads=1
//! ```
#![cfg(unix)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use maeclyprint_lib::engine::print::spool;
use maeclyprint_lib::platform::queue_of;
use maeclyprint_lib::queue::job::{
    ColorMode, Duplex, JobSettings, Orientation, PaperSize, Quality, Scaling,
};

/// Queue names this suite must never touch, whatever the environment says.
const FORBIDDEN_QUEUES: &[&str] = &["EPSON_L15160_Series"];

/// `Some(queue)` when the suite is enabled, `None` (skip) otherwise.
fn hold_queue() -> Option<String> {
    if std::env::var("MAECLY_REAL_PRINTER").ok().as_deref() != Some("1") {
        eprintln!("skipped: set MAECLY_REAL_PRINTER=1 to run the real-spooler suite");
        return None;
    }
    let queue = std::env::var("MAECLY_HOLD_QUEUE")
        .ok()
        .filter(|q| !q.trim().is_empty())
        .unwrap_or_else(|| "Maecly_Hold".to_string());
    assert!(
        !FORBIDDEN_QUEUES.contains(&queue.as_str()),
        "refusing to submit to {queue}: it is a real printer, not a hold queue"
    );
    assert_holds_every_job(&queue);
    Some(queue)
}

/// Refuse to run unless the queue's default `job-hold-until` is `indefinite`.
fn assert_holds_every_job(queue: &str) {
    let out = Command::new("lpoptions")
        .args(["-p", queue])
        .output()
        .expect("run lpoptions (is CUPS installed?)");
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success() && text.contains("job-hold-until=indefinite"),
        "queue {queue} does not hold jobs by default (job-hold-until=indefinite \
         missing from `lpoptions -p {queue}`); refusing to submit. Output: {text}"
    );
}

/// The ids of every job currently in `queue`, from `lpstat -o`.
fn job_ids(queue: &str) -> BTreeSet<u32> {
    let out = Command::new("lpstat")
        .args(["-o", queue])
        .output()
        .expect("run lpstat");
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| l.split_whitespace().next())
        .filter_map(|id| id.rsplit_once('-').and_then(|(_, n)| n.parse().ok()))
        .collect()
}

/// Cancels the job when dropped, so a failed assertion never leaves a job held.
struct CancelOnDrop(u32);

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        let _ = Command::new("cancel").arg(self.0.to_string()).status();
    }
}

/// A two-page PDF in a fresh temp file (A4 MediaBox, no content). Two pages
/// so a page range has something real to select.
fn two_page_pdf(tag: &str) -> PathBuf {
    let body = "%PDF-1.4\n\
        1 0 obj<</Type/Catalog/Pages 2 0 R>>endobj\n\
        2 0 obj<</Type/Pages/Kids[3 0 R 4 0 R]/Count 2>>endobj\n\
        3 0 obj<</Type/Page/Parent 2 0 R/MediaBox[0 0 595 842]>>endobj\n\
        4 0 obj<</Type/Page/Parent 2 0 R/MediaBox[0 0 595 842]>>endobj\n\
        trailer<</Root 1 0 R>>\n%%EOF\n";
    let path = std::env::temp_dir().join(format!(
        "maecly-real-printer-{}-{tag}.pdf",
        std::process::id()
    ));
    std::fs::write(&path, body).expect("write probe pdf");
    path
}

/// `get-job-attributes` for one job, as `name -> value` (the raw text after
/// `(type) = `).
fn job_attributes(job_id: u32) -> BTreeMap<String, String> {
    let out = Command::new("ipptool")
        .args([
            "-tv",
            &format!("ipp://localhost/jobs/{job_id}"),
            "get-job-attributes.test",
        ])
        .output()
        .expect("run ipptool");
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "ipptool failed: {text}");
    parse_attributes(&text)
}

fn parse_attributes(text: &str) -> BTreeMap<String, String> {
    text.lines()
        .filter_map(|line| {
            let line = line.trim();
            let (lhs, value) = line.split_once(" = ")?;
            let (name, _type) = lhs.split_once(" (")?;
            Some((name.to_string(), value.trim().to_string()))
        })
        .collect()
}

/// Spool `settings` through the engine and hand back the held job's
/// attributes. The job is cancelled when the returned guard drops.
fn submit(
    queue: &str,
    tag: &str,
    settings: &JobSettings,
) -> (CancelOnDrop, BTreeMap<String, String>) {
    let pdf = two_page_pdf(tag);
    let title = format!("maecly-real-printer-{tag}");
    let before = job_ids(queue);

    // The queue's real capabilities, exactly as the dispatcher resolves them.
    let (_, caps) = queue_of(queue);
    let outcome = spool(
        &pdf,
        queue,
        settings,
        &caps,
        &title,
        Some(Duration::from_secs(30)),
    )
    .unwrap_or_else(|e| panic!("spool to {queue} failed: {e}"));

    // Prefer the id the engine parsed; fall back to the queue diff because
    // `lp` localizes its success line on macOS (see the dedicated test below).
    let id = outcome
        .os_job_id
        .as_deref()
        .and_then(|s| s.rsplit_once('-'))
        .and_then(|(_, n)| n.parse().ok())
        .or_else(|| {
            let after = job_ids(queue);
            let fresh: Vec<u32> = after.difference(&before).copied().collect();
            assert_eq!(
                fresh.len(),
                1,
                "expected exactly one new job, got {fresh:?}"
            );
            fresh.first().copied()
        })
        .expect("a job id for the submitted job");
    let guard = CancelOnDrop(id);
    let _ = std::fs::remove_file(&pdf);

    let attrs = job_attributes(id);
    assert_eq!(
        attrs.get("job-state").map(String::as_str),
        Some("pending-held"),
        "job {id} is not held; cancel it now. Attributes: {attrs:#?}"
    );
    (guard, attrs)
}

fn attr<'a>(attrs: &'a BTreeMap<String, String>, name: &str) -> Option<&'a str> {
    attrs.get(name).map(String::as_str)
}

/// CUPS stores `media` as given (`A4`) or as the PWG self-describing name;
/// accept either spelling of the same size.
fn assert_media(attrs: &BTreeMap<String, String>, expected: &[&str]) {
    let media = attr(attrs, "media")
        .unwrap_or_default()
        .to_ascii_lowercase();
    assert!(
        expected.iter().any(|e| media == e.to_ascii_lowercase()),
        "media = {media:?}, expected one of {expected:?}"
    );
}

/// Landscape reaches CUPS either as the IPP `orientation-requested=landscape`
/// or as the `landscape` boolean `lp -o landscape` produces.
fn is_landscape(attrs: &BTreeMap<String, String>) -> bool {
    attr(attrs, "orientation-requested") == Some("landscape")
        || attr(attrs, "landscape") == Some("true")
}

/// Quality is sent in both spellings; CUPS keeps the PPD `cupsPrintQuality`
/// and drops the IPP `print-quality` when a queue has a (generated) PPD, so
/// either one carrying the right value is a pass.
fn assert_quality(attrs: &BTreeMap<String, String>, ipp: &str, ppd: &str) {
    assert!(
        attr(attrs, "print-quality") == Some(ipp) || attr(attrs, "cupsPrintQuality") == Some(ppd),
        "expected quality {ipp}/{ppd}: print-quality={:?} cupsPrintQuality={:?}",
        attr(attrs, "print-quality"),
        attr(attrs, "cupsPrintQuality")
    );
}

/// Mono reaches CUPS as the PPD `ColorModel=Gray` or the IPP
/// `print-color-mode=monochrome`.
fn is_monochrome(attrs: &BTreeMap<String, String>) -> bool {
    attr(attrs, "ColorModel") == Some("Gray")
        || attr(attrs, "print-color-mode") == Some("monochrome")
}

#[test]
#[ignore = "submits to a real CUPS hold queue; set MAECLY_REAL_PRINTER=1"]
fn fr_040_defaults_reach_cups_as_one_sided_a4_colour_single_copy() {
    let Some(queue) = hold_queue() else { return };
    let settings = JobSettings {
        printer: Some(queue.clone()),
        ..JobSettings::default()
    };
    let (_guard, attrs) = submit(&queue, "defaults", &settings);

    assert_eq!(attr(&attrs, "sides"), Some("one-sided"));
    assert_media(&attrs, &["A4", "iso_a4_210x297mm"]);
    assert!(
        !is_monochrome(&attrs),
        "colour must not be forced grey: {attrs:#?}"
    );
    assert!(
        matches!(attr(&attrs, "copies"), None | Some("1")),
        "copies = {:?}",
        attr(&attrs, "copies")
    );
    assert_eq!(attr(&attrs, "page-ranges"), None);
    assert!(!is_landscape(&attrs));
    assert_eq!(attr(&attrs, "media-source"), None);
    assert_quality(&attrs, "normal", "Normal");
    assert_eq!(attr(&attrs, "fit-to-page"), Some("true"));
}

#[test]
#[ignore = "submits to a real CUPS hold queue; set MAECLY_REAL_PRINTER=1"]
fn fr_040_full_override_set_reaches_cups_as_ipp_attributes() {
    let Some(queue) = hold_queue() else { return };
    let settings = JobSettings {
        printer: Some(queue.clone()),
        duplex: Duplex::Long,
        color_mode: ColorMode::Monochrome,
        copies: 3,
        paper_size: PaperSize::LETTER,
        orientation: Orientation::Landscape,
        scaling: Scaling::None,
        page_ranges: Some(" 1-2 ".to_string()),
        quality: Quality::High,
        tray: Some("Rear".to_string()),
        collate: false,
    };
    let (_guard, attrs) = submit(&queue, "full", &settings);

    assert_eq!(attr(&attrs, "sides"), Some("two-sided-long-edge"));
    assert!(is_monochrome(&attrs), "expected monochrome: {attrs:#?}");
    assert_eq!(attr(&attrs, "copies"), Some("3"));
    assert_media(&attrs, &["Letter", "na_letter_8.5x11in"]);
    assert!(is_landscape(&attrs), "expected landscape: {attrs:#?}");
    assert_eq!(attr(&attrs, "page-ranges"), Some("1-2"));
    assert_eq!(attr(&attrs, "media-source"), Some("Rear"));
    assert_eq!(attr(&attrs, "InputSlot"), Some("Rear"));
    assert_quality(&attrs, "high", "High");
    assert_eq!(
        attr(&attrs, "fit-to-page"),
        None,
        "scaling none must not fit"
    );
}

#[test]
#[ignore = "submits to a real CUPS hold queue; set MAECLY_REAL_PRINTER=1"]
fn fr_040_short_edge_legal_draft_single_page() {
    let Some(queue) = hold_queue() else { return };
    let settings = JobSettings {
        printer: Some(queue.clone()),
        duplex: Duplex::Short,
        paper_size: PaperSize::LEGAL,
        orientation: Orientation::Portrait,
        scaling: Scaling::Shrink,
        page_ranges: Some("2".to_string()),
        quality: Quality::Draft,
        ..JobSettings::default()
    };
    let (_guard, attrs) = submit(&queue, "short", &settings);

    assert_eq!(attr(&attrs, "sides"), Some("two-sided-short-edge"));
    assert_media(&attrs, &["Legal", "na_legal_8.5x14in"]);
    assert_eq!(attr(&attrs, "page-ranges"), Some("2-2"));
    assert!(
        !is_landscape(&attrs),
        "portrait must not rotate: {attrs:#?}"
    );
    assert_quality(&attrs, "draft", "Draft");
    assert_eq!(
        attr(&attrs, "fit-to-page"),
        Some("true"),
        "shrink maps to fit-to-page"
    );
}

#[test]
#[ignore = "submits to a real CUPS hold queue; set MAECLY_REAL_PRINTER=1"]
fn fr_040_a3_with_multiple_ranges() {
    let Some(queue) = hold_queue() else { return };
    let settings = JobSettings {
        printer: Some(queue.clone()),
        copies: 2,
        paper_size: PaperSize::A3,
        page_ranges: Some("1, 2".to_string()),
        ..JobSettings::default()
    };
    let (_guard, attrs) = submit(&queue, "a3", &settings);

    assert_eq!(attr(&attrs, "copies"), Some("2"));
    assert_media(&attrs, &["A3", "iso_a3_297x420mm"]);
    // Two singleton ranges; CUPS prints a 1setOf rangeOfInteger comma-joined.
    assert_eq!(attr(&attrs, "page-ranges"), Some("1-1,2-2"));
}

/// FR-032: cancelling a printing job needs the CUPS job id `spool` parses out
/// of `lp`'s success line. `lp` localizes that line on macOS (it follows the
/// user's macOS language, not `LC_ALL`/`LANG`), so the parser cannot rely on
/// the English marker (`cups::parse_request_id`); on a non-English host this
/// is the end-to-end check that the dispatcher still gets the handle it would
/// cancel with.
#[test]
#[ignore = "submits to a real CUPS hold queue; set MAECLY_REAL_PRINTER=1"]
fn fr_032_spool_reports_the_cups_job_id() {
    let Some(queue) = hold_queue() else { return };
    let pdf = two_page_pdf("request-id");
    let before = job_ids(&queue);
    let outcome = spool(
        &pdf,
        &queue,
        &JobSettings::default(),
        &queue_of(&queue).1,
        "maecly-real-printer-request-id",
        Some(Duration::from_secs(30)),
    )
    .expect("spool");
    let _ = std::fs::remove_file(&pdf);
    let fresh: Vec<u32> = job_ids(&queue).difference(&before).copied().collect();
    let _guards: Vec<CancelOnDrop> = fresh.iter().copied().map(CancelOnDrop).collect();

    let expected = fresh.first().map(|n| format!("{queue}-{n}"));
    assert_eq!(
        outcome.os_job_id, expected,
        "spool did not return the CUPS job id (lp's success line is localized on this host?)"
    );
}

#[test]
fn ipptool_output_parses_into_attribute_map() {
    let sample = "        copies (integer) = 2\n        \
                  sides (keyword) = two-sided-long-edge\n        \
                  page-ranges (rangeOfInteger) = 1-2\n    \
                  Get job info with get-job-attributes   [PASS]\n";
    let attrs = parse_attributes(sample);
    assert_eq!(attr(&attrs, "copies"), Some("2"));
    assert_eq!(attr(&attrs, "sides"), Some("two-sided-long-edge"));
    assert_eq!(attr(&attrs, "page-ranges"), Some("1-2"));
    assert_eq!(attrs.len(), 3);
}
