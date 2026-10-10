//! Real-spooler checks for the Windows print path (Windows only).
//!
//! The unit tests in `engine::print::windows` pin the pure job matching and
//! the `JOB_STATUS_*` mapping. This suite pins the other half against the
//! real spooler: SumatraPDF spools a PDF, the job is found by its document
//! path among the current user's new jobs, `GetJob` reads its state, and the
//! cancel removes it.
//!
//! # Safety: nothing is ever printed
//!
//! Every test is `#[ignore]`d **and** gated on `MAECLY_REAL_PRINTER=1`. Jobs go
//! to the queue named by `MAECLY_HOLD_QUEUE` (default `Maecly_Hold_Win`),
//! which must be **paused**: a paused Windows queue keeps every job and sends
//! nothing to its port. The suite refuses to submit anything to a queue that
//! is not paused, or to a real device it knows, and deletes its job on the way
//! out (also on panic, via a drop guard).
//!
//! Create such a queue once, as an administrator (the port is a file that is
//! never written while the queue is paused):
//!
//! ```powershell
//! Add-PrinterPort -Name 'C:\maecly\hold.prn'
//! Add-Printer -Name Maecly_Hold_Win -DriverName 'Microsoft Print To PDF' -PortName 'C:\maecly\hold.prn'
//! Invoke-CimMethod -MethodName Pause -InputObject (Get-CimInstance Win32_Printer -Filter "Name='Maecly_Hold_Win'")
//! ```
//!
//! Run with SumatraPDF on `PATH` (or bundled next to the test binary):
//!
//! ```powershell
//! $env:MAECLY_REAL_PRINTER = '1'
//! cargo test --test real_printer_windows -- --ignored --test-threads=1
//! ```
#![cfg(windows)]

use std::path::PathBuf;
use std::time::Duration;

use pluryprint_lib::engine::print::{cancel_os_job, job_state, spool, OsJobState};
use pluryprint_lib::ipc::printers::PrinterStatus;
use pluryprint_lib::platform::{list_printers, queue_of};
use pluryprint_lib::queue::job::JobSettings;

/// Queue-name prefixes this suite must never touch, whatever the environment
/// says.
const FORBIDDEN_PREFIXES: &[&str] = &["EPSON"];

/// `Some(queue)` when the suite is enabled and the queue is safe to use.
fn hold_queue() -> Option<String> {
    if std::env::var("MAECLY_REAL_PRINTER").ok().as_deref() != Some("1") {
        eprintln!("skipped: set MAECLY_REAL_PRINTER=1 to run the real-spooler suite");
        return None;
    }
    let queue = std::env::var("MAECLY_HOLD_QUEUE")
        .ok()
        .filter(|q| !q.trim().is_empty())
        .unwrap_or_else(|| "Maecly_Hold_Win".to_string());
    assert!(
        !FORBIDDEN_PREFIXES
            .iter()
            .any(|p| queue.to_ascii_uppercase().starts_with(p)),
        "refusing to submit to {queue}: it is a real printer, not a hold queue"
    );
    let printer = list_printers()
        .expect("the spooler answers")
        .into_iter()
        .find(|p| p.id == queue)
        .unwrap_or_else(|| panic!("no queue named {queue}"));
    // A paused queue reads Offline (status_from); Ready/Busy would print.
    assert!(
        matches!(printer.status, PrinterStatus::Offline),
        "refusing to submit to {queue}: it is not paused ({:?})",
        printer.status
    );
    Some(queue)
}

/// Deletes the job on drop, so a failing assertion never leaves it queued.
struct CancelOnDrop<'a>(&'a str, String);

impl Drop for CancelOnDrop<'_> {
    fn drop(&mut self) {
        let _ = cancel_os_job(self.0, &self.1);
    }
}

fn two_page_pdf(tag: &str) -> PathBuf {
    let body = "%PDF-1.4\n\
        1 0 obj<</Type/Catalog/Pages 2 0 R>>endobj\n\
        2 0 obj<</Type/Pages/Kids[3 0 R 4 0 R]/Count 2>>endobj\n\
        3 0 obj<</Type/Page/Parent 2 0 R/MediaBox[0 0 595 842]>>endobj\n\
        4 0 obj<</Type/Page/Parent 2 0 R/MediaBox[0 0 595 842]>>endobj\n\
        trailer<</Root 1 0 R>>\n%%EOF\n";
    let path = std::env::temp_dir().join(format!(
        "maecly real printer {}-{tag}.pdf",
        std::process::id()
    ));
    std::fs::write(&path, body).expect("write probe pdf");
    path
}

/// FR-031: the spooled job is found (by its document path, among this user's
/// new jobs), its state is read while it waits on the paused queue, and the
/// cancel removes it — after which the spooler no longer knows it.
#[test]
#[ignore = "submits to a paused Windows hold queue; set MAECLY_REAL_PRINTER=1"]
fn fr_031_a_spooled_job_is_tracked_and_cancelled_on_a_paused_queue() {
    let Some(queue) = hold_queue() else { return };
    let pdf = two_page_pdf("tracked");
    let outcome = spool(
        &pdf,
        &queue,
        &JobSettings::default(),
        &queue_of(&queue).1,
        "maecly-real-printer-tracked",
        Some(Duration::from_secs(60)),
    )
    .expect("SumatraPDF spools to the paused queue");
    let id = outcome
        .os_job_id
        .expect("the job SumatraPDF created is found by its document path");
    let guard = CancelOnDrop(&queue, id.clone());

    let state = job_state(&queue, &id).expect("GetJob answers");
    assert!(
        matches!(state, OsJobState::Pending | OsJobState::Held),
        "a job on a paused queue waits: {state:?}"
    );

    cancel_os_job(&queue, &id).expect("the job is deleted");
    // Deleting is asynchronous; the job is either gone or on its way out.
    let mut last = OsJobState::Pending;
    for _ in 0..50 {
        last = job_state(&queue, &id).expect("GetJob answers");
        if last == OsJobState::Gone {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(
        matches!(last, OsJobState::Gone | OsJobState::Canceled),
        "a deleted job that never printed is gone or cancelled, never completed: {last:?}"
    );
    std::mem::forget(guard);
    let _ = std::fs::remove_file(&pdf);
}
