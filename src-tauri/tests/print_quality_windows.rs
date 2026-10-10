//! Windows print quality and the other SumatraPDF settings, against the real
//! spooler (decision D16, `docs/03-requirements/print-settings.md`).
//!
//! The swap logic itself (journal, restore rules, lock, crash recovery) is
//! unit-tested on every platform in `engine::print::devmode_swap`. This suite
//! checks what only a real spooler can say: that SumatraPDF really starts
//! from the per-user default DEVMODE, that the job keeps the requested
//! quality after the default is put back, that `SetPrinter` level 9 restores
//! byte for byte (and clears again when the user had none), and that
//! `paperkind=`/`bin=` ids land in the job.
//!
//! The oracle is the job's own DEVMODE (`JOB_INFO_2.pDevMode`), read while
//! the job waits on a **paused** queue.
//!
//! # Safety: nothing is ever printed
//!
//! Every test is `#[ignore]`d **and** gated on `MAECLY_REAL_PRINTER=1`. The
//! queues come from `PLURYPRINT_QUALITY_QUEUES` (comma-separated, default
//! `Maecly_Hold_Win`); each must be **paused** and never a known real device.
//! Every job is deleted on the way out, also on panic, and the user's
//! per-user default is put back by a guard.
//!
//! ```powershell
//! $env:MAECLY_REAL_PRINTER = '1'
//! $env:PLURYPRINT_QUALITY_QUEUES = 'Maecly_Hold_Win,R3L6_Hold_IPP'
//! cargo test --test print_quality_windows -- --ignored --test-threads=1 --nocapture
//! ```
//! `scripts/vm/print-quality-e2e.ps1` creates the queues and runs this.
#![cfg(windows)]

use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use pluryprint_lib::engine::print::devmode_swap::{
    self as swap, default_source_field, job_devmode, job_quality, paper_size_field, print_quality,
    quality_support, recover_stale_journals, with_print_quality, QualityOutcome, QualitySupport,
    RestoreAction, SpoolerDefaults, Swap, Target, UserDefaults,
};
use pluryprint_lib::engine::print::{cancel_os_job, sumatra, PrintError};
use pluryprint_lib::engine::{EngineError, PdfProtection};
use pluryprint_lib::ipc::printers::PrinterStatus;
use pluryprint_lib::platform::{driver_bin_id, list_printers};
use pluryprint_lib::queue::job::{JobSettings, PaperSize, Quality};

const FORBIDDEN_PREFIXES: &[&str] = &["EPSON"];
const ROLE_ENV: &str = "PLURYPRINT_QT_ROLE";
const QUEUE_ENV: &str = "PLURYPRINT_QT_QUEUE";
const TIMEOUT: Option<Duration> = Some(Duration::from_secs(90));

/// The paused queues to use, or `None` when the suite is not enabled.
fn queues() -> Option<Vec<String>> {
    if std::env::var("MAECLY_REAL_PRINTER").ok().as_deref() != Some("1") {
        eprintln!("skipped: set MAECLY_REAL_PRINTER=1 to run the real-spooler suite");
        return None;
    }
    let names: Vec<String> = std::env::var("PLURYPRINT_QUALITY_QUEUES")
        .unwrap_or_else(|_| "Maecly_Hold_Win".into())
        .split(',')
        .map(|q| q.trim().to_string())
        .filter(|q| !q.is_empty())
        .collect();
    let printers = list_printers().expect("the spooler answers");
    for q in &names {
        assert!(
            !FORBIDDEN_PREFIXES
                .iter()
                .any(|p| q.to_ascii_uppercase().starts_with(p)),
            "refusing to submit to {q}: it is a real printer"
        );
        let p = printers
            .iter()
            .find(|p| &p.id == q)
            .unwrap_or_else(|| panic!("no queue named {q}"));
        assert!(
            matches!(p.status, PrinterStatus::Offline),
            "refusing to submit to {q}: it is not paused ({:?})",
            p.status
        );
    }
    Some(names)
}

/// Puts the user's per-user default back on drop, whatever a test did.
struct KeepDefault {
    queue: String,
    original: Option<Vec<u8>>,
}

impl KeepDefault {
    fn new(queue: &str) -> Self {
        Self {
            queue: queue.to_string(),
            original: SpoolerDefaults.get(queue).expect("GetPrinter(9)"),
        }
    }
}

impl Drop for KeepDefault {
    fn drop(&mut self) {
        let _ = SpoolerDefaults.set(&self.queue, self.original.as_deref());
    }
}

/// Deletes a job on drop.
struct CancelOnDrop(String, String);

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        let _ = cancel_os_job(&self.0, &self.1);
    }
}

fn probe_pdf(tag: &str) -> PathBuf {
    let body = "%PDF-1.4\n\
        1 0 obj<</Type/Catalog/Pages 2 0 R>>endobj\n\
        2 0 obj<</Type/Pages/Kids[3 0 R]/Count 1>>endobj\n\
        3 0 obj<</Type/Page/Parent 2 0 R/MediaBox[0 0 595 842]>>endobj\n\
        trailer<</Root 1 0 R>>\n%%EOF\n";
    let path = std::env::temp_dir().join(format!("pluryprint-qt-{}-{tag}.pdf", std::process::id()));
    std::fs::write(&path, body).expect("write probe pdf");
    path
}

fn settings(quality: Quality) -> JobSettings {
    JobSettings {
        quality,
        ..JobSettings::default()
    }
}

/// Spool one probe page and return its spooler job id.
fn spool(queue: &str, s: &JobSettings, tag: &str) -> String {
    let pdf = probe_pdf(tag);
    let outcome = sumatra::spool(&pdf, queue, s, &format!("qt-{tag}.pdf"), TIMEOUT)
        .expect("SumatraPDF spools to the paused queue");
    let _ = std::fs::remove_file(&pdf);
    outcome
        .os_job_id
        .expect("the job SumatraPDF created is found")
}

/// The spooler job id inside a recorded `os_job_id` (`12@<stamp>`, see
/// `windows::JobRef`).
fn spooler_id(os_job_id: &str) -> u32 {
    pluryprint_lib::engine::print::windows::JobRef::parse(os_job_id)
        .unwrap_or_else(|| panic!("'{os_job_id}' is not a spooler job reference"))
        .id
}

fn hex_prefix(b: &Option<Vec<u8>>) -> String {
    match b {
        None => "none".into(),
        Some(b) => format!("{} bytes, quality {:?}", b.len(), print_quality(b)),
    }
}

/// E1/E2: a Draft or High job carries its quality, and the user's default is
/// exactly what it was afterwards. A driver that offers no such quality is
/// left alone and the job says so.
#[test]
#[ignore = "submits to paused Windows queues; set MAECLY_REAL_PRINTER=1"]
fn print_settings_quality_draft_high_applied_windows() {
    let Some(queues) = queues() else { return };
    for queue in &queues {
        let keep = KeepDefault::new(queue);
        match swap::print_capabilities(queue) {
            Ok(caps) => println!(
                "CAPS queue={queue} PageOutputQuality={:?} ({} bytes)",
                swap::quality_options(&caps),
                caps.len()
            ),
            Err(e) => println!("CAPS queue={queue} unavailable: {e}"),
        }
        for (quality, target) in [
            (Quality::Draft, Target::Draft),
            (Quality::High, Target::High),
        ] {
            let support = quality_support(queue, target);
            let id = spool(queue, &settings(quality), target.label());
            let _guard = CancelOnDrop(queue.clone(), id.clone());
            let after = SpoolerDefaults.get(queue).unwrap();
            assert_eq!(
                after, keep.original,
                "{queue}: the user's default is restored byte for byte"
            );
            let outcome = job_quality(queue, spooler_id(&id), target);
            let dm = job_devmode(queue, spooler_id(&id)).ok().flatten();
            println!(
                "QUALITY queue={queue} target={} support={support:?} outcome={outcome:?} job_dmPrintQuality={:?} user_default={}",
                target.label(),
                dm.as_deref().and_then(print_quality),
                hex_prefix(&keep.original),
            );
            match support {
                QualitySupport::Offered => assert_eq!(outcome, QualityOutcome::Applied, "{queue}"),
                QualitySupport::NotOffered => assert!(
                    !matches!(outcome, QualityOutcome::Applied),
                    "{queue}: nothing was swapped, so the job cannot carry it by our doing"
                ),
                // `quality_support` asks the driver only; a pending restore
                // is the preview's business (`sumatra::quality_support_for`).
                QualitySupport::Unknown | QualitySupport::PendingRestore => {}
            }
        }
        // Normal is never forced: the default is not even rewritten.
        let id = spool(queue, &settings(Quality::Normal), "normal");
        let _guard = CancelOnDrop(queue.clone(), id);
        assert_eq!(SpoolerDefaults.get(queue).unwrap(), keep.original);
    }
}

/// FR-031: the Windows queue shows the job under the user's file name, not
/// the temporary folder SumatraPDF printed it from, with and without a
/// quality swap (the job is found at a different moment in each).
#[test]
#[ignore = "submits to paused Windows queues; set MAECLY_REAL_PRINTER=1"]
fn fr_031_the_windows_queue_shows_the_file_name_windows() {
    let Some(queues) = queues() else { return };
    for queue in &queues {
        let _keep = KeepDefault::new(queue);
        for (quality, tag) in [(Quality::Normal, "title-n"), (Quality::Draft, "title-d")] {
            let id = spool(queue, &settings(quality), tag);
            let _guard = CancelOnDrop(queue.clone(), id.clone());
            let shown = pluryprint_lib::engine::print::windows::job_document(queue, &id);
            println!("TITLE queue={queue} quality={quality:?} document={shown:?}");
            assert_eq!(
                shown.as_deref(),
                Some(format!("qt-{tag}.pdf").as_str()),
                "{queue}"
            );
        }
    }
}

/// E3: a user with no per-user default gets none back (not ours, not a copy
/// of the global default).
#[test]
#[ignore = "submits to paused Windows queues; set MAECLY_REAL_PRINTER=1"]
fn print_settings_quality_null_user_devmode_restored_windows() {
    let Some(queues) = queues() else { return };
    for queue in &queues {
        let _keep = KeepDefault::new(queue);
        SpoolerDefaults
            .set(queue, None)
            .expect("SetPrinter(9, NULL)");
        let cleared = SpoolerDefaults.get(queue).unwrap();
        println!("NULLDM queue={queue} after_clear={}", hex_prefix(&cleared));
        assert_eq!(
            cleared, None,
            "{queue}: SetPrinter(9, NULL) clears the per-user default"
        );
        let id = spool(queue, &settings(Quality::Draft), "null");
        let _guard = CancelOnDrop(queue.clone(), id);
        assert_eq!(SpoolerDefaults.get(queue).unwrap(), None, "{queue}");
    }
}

/// E1 with a user who has their own default (High): a Draft job gets Draft,
/// and their High default comes back byte for byte.
#[test]
#[ignore = "submits to paused Windows queues; set MAECLY_REAL_PRINTER=1"]
fn print_settings_quality_own_user_devmode_restored_bytewise_windows() {
    let Some(queues) = queues() else { return };
    for queue in &queues {
        let _keep = KeepDefault::new(queue);
        let Some(high) = swap::build_job_devmode(queue, Target::High).unwrap() else {
            println!("OWNDM queue={queue} skipped: the driver offers no high quality");
            continue;
        };
        SpoolerDefaults.set(queue, Some(&high)).unwrap();
        let theirs = SpoolerDefaults.get(queue).unwrap();
        assert!(theirs.is_some());
        let id = spool(queue, &settings(Quality::Draft), "owndm");
        let _guard = CancelOnDrop(queue.clone(), id.clone());
        let outcome = job_quality(queue, spooler_id(&id), Target::Draft);
        println!(
            "OWNDM queue={queue} outcome={outcome:?} restored={}",
            SpoolerDefaults.get(queue).unwrap() == theirs
        );
        assert_eq!(outcome, QualityOutcome::Applied, "{queue}");
        assert_eq!(SpoolerDefaults.get(queue).unwrap(), theirs, "{queue}");
        // And a Normal job simply inherits the user's own High.
        let id = spool(queue, &settings(Quality::Normal), "owndm-normal");
        let _guard = CancelOnDrop(queue.clone(), id.clone());
        assert_eq!(
            job_quality(queue, spooler_id(&id), Target::High),
            QualityOutcome::Applied,
            "{queue}: SumatraPDF starts from the per-user default"
        );
    }
}

/// E5: a change made to the default while the swap is in place is kept.
#[test]
#[ignore = "changes the per-user default of paused Windows queues; set MAECLY_REAL_PRINTER=1"]
fn print_settings_quality_does_not_clobber_user_change_windows() {
    let Some(queues) = queues() else { return };
    for queue in &queues {
        let _keep = KeepDefault::new(queue);
        let dir = tempfile::tempdir().unwrap();
        let built = |t| swap::build_job_devmode(queue, t);
        let Some(ours) = built(Target::Draft)
            .unwrap()
            .or(built(Target::High).unwrap())
        else {
            println!("CLOBBER queue={queue} skipped: the driver offers neither quality");
            continue;
        };
        let mut s = Swap::begin(
            SpoolerDefaults,
            dir.path(),
            queue,
            Target::Draft,
            Duration::from_secs(5),
            || Ok(Some(ours.clone())),
        )
        .expect("swap");
        // "The user" picks something else meanwhile.
        let current = SpoolerDefaults.get(queue).unwrap().unwrap();
        let theirs = with_print_quality(&current, -2).unwrap();
        SpoolerDefaults.set(queue, Some(&theirs)).unwrap();
        let theirs = SpoolerDefaults.get(queue).unwrap();
        assert_eq!(s.restore(), Ok(RestoreAction::LeaveUserChange), "{queue}");
        assert_eq!(SpoolerDefaults.get(queue).unwrap(), theirs, "{queue}");
        println!("CLOBBER queue={queue} kept the user's change");
    }
}

/// E4: a process killed right after the swap leaves a journal; the next start
/// restores the default byte for byte.
#[test]
#[ignore = "changes the per-user default of paused Windows queues; set MAECLY_REAL_PRINTER=1"]
fn print_settings_quality_swap_restores_bytewise_after_abort_windows() {
    let Some(queues) = queues() else { return };
    for queue in &queues {
        let keep = KeepDefault::new(queue);
        if quality_support(queue, Target::Draft) == QualitySupport::NotOffered {
            println!("ABORT queue={queue} skipped: no draft quality to swap in");
            continue;
        }
        let status = child("abort", queue).status().expect("run child");
        assert_eq!(status.code(), Some(swap::ABORT_AFTER_SWAP_EXIT), "{queue}");
        let journal = swap::journal_path(&swap::journal_dir(), queue);
        assert!(journal.exists(), "{queue}: the crash left its journal");
        assert_ne!(
            SpoolerDefaults.get(queue).unwrap(),
            keep.original,
            "{queue}: the swapped default is still in place"
        );
        recover_stale_journals();
        assert_eq!(
            SpoolerDefaults.get(queue).unwrap(),
            keep.original,
            "{queue}: restored byte for byte"
        );
        assert!(!journal.exists());
        println!("ABORT queue={queue} recovered");
    }
}

/// E6: two CLI-like processes print Draft and High to the same queue at the
/// same time; each job carries its own quality.
#[test]
#[ignore = "submits to paused Windows queues; set MAECLY_REAL_PRINTER=1"]
fn print_settings_quality_concurrent_cli_each_job_own_quality_windows() {
    let Some(queues) = queues() else { return };
    for queue in &queues {
        let keep = KeepDefault::new(queue);
        let both = [Target::Draft, Target::High];
        if both
            .iter()
            .any(|&t| quality_support(queue, t) != QualitySupport::Offered)
        {
            println!("CONCURRENT queue={queue} skipped: the driver lacks draft or high");
            continue;
        }
        let kids: Vec<_> = both
            .iter()
            .map(|t| (t, child(t.label(), queue).output_spawn()))
            .collect();
        for (target, kid) in kids {
            let out = kid.wait_with_output().expect("child");
            let stdout = String::from_utf8_lossy(&out.stdout);
            assert!(out.status.success(), "{queue} {}: {stdout}", target.label());
            let id = stdout
                .lines()
                // libtest prints the test name on the same line.
                .find_map(|l| l.split("PLURYPRINT_JOB=").nth(1))
                .and_then(|rest| rest.split_whitespace().next())
                .unwrap_or_else(|| panic!("no job id in: {stdout}"))
                .to_string();
            let _guard = CancelOnDrop(queue.clone(), id.clone());
            let outcome = job_quality(queue, spooler_id(&id), *target);
            println!(
                "CONCURRENT queue={queue} target={} outcome={outcome:?}",
                target.label()
            );
            assert_eq!(
                outcome,
                QualityOutcome::Applied,
                "{queue} {}",
                target.label()
            );
        }
        assert_eq!(SpoolerDefaults.get(queue).unwrap(), keep.original);
    }
}

/// E7 (ids): `paperkind=` and `bin=` land in the job's own DEVMODE as the
/// driver's ids, whatever the driver calls them.
#[test]
#[ignore = "submits to paused Windows queues; set MAECLY_REAL_PRINTER=1"]
fn print_settings_paper_and_bin_ids_reach_the_job_windows() {
    let Some(queues) = queues() else { return };
    let printers = list_printers().unwrap();
    for queue in &queues {
        let p = printers.iter().find(|p| &p.id == queue).unwrap();
        let letter = p
            .papers
            .iter()
            .any(|n| n.eq_ignore_ascii_case("Letter") || n == "Carta");
        let s = JobSettings {
            paper_size: if letter {
                PaperSize::LETTER
            } else {
                PaperSize::A4
            },
            tray: p.trays.last().map(|t| t.name.clone()),
            ..JobSettings::default()
        };
        let id = spool(queue, &s, "ids");
        let _guard = CancelOnDrop(queue.clone(), id.clone());
        let dm = job_devmode(queue, spooler_id(&id)).unwrap().unwrap();
        let want_paper = if letter { 1 } else { 9 };
        let want_bin = s.tray.as_deref().and_then(|t| driver_bin_id(queue, t));
        println!(
            "IDS queue={queue} paper={:?} (want {want_paper}) bin={:?} (want {want_bin:?}, tray {:?})",
            paper_size_field(&dm),
            default_source_field(&dm),
            s.tray
        );
        assert_eq!(paper_size_field(&dm), Some(want_paper), "{queue}");
        if let Some(bin) = want_bin {
            assert_eq!(default_source_field(&dm), Some(bin), "{queue}");
        }
    }
}

/// FR-059: a PDF whose permissions forbid printing is refused by the engine
/// before SumatraPDF sees it — the terminal `printing_not_allowed`.
///
/// It has to be: the SumatraPDF 3.5.2 release build prints such a file
/// without a word (exit 0; verified on a paused Windows 11 ARM64 queue). The
/// second half records what SumatraPDF does, so a version that starts
/// refusing (and is mapped by `sumatra::failure_from_exit`) shows up here.
#[test]
#[ignore = "runs SumatraPDF against a paused Windows queue; set MAECLY_REAL_PRINTER=1"]
fn fr_059_a_no_print_pdf_is_refused_before_sumatrapdf_windows() {
    use pluryprint_lib::engine::convert::{convert_to_pdf, PageSetup};
    let Some(queues) = queues() else { return };
    let queue = &queues[0];
    let pdf = no_print_pdf();
    let cache = tempfile::tempdir().unwrap();
    let engine = convert_to_pdf(
        &pdf,
        cache.path(),
        "no-print",
        &std::sync::atomic::AtomicBool::new(false),
        &|_| {},
        None,
        &PageSetup::for_settings(&JobSettings::default()),
        None,
    );
    assert!(
        matches!(
            engine,
            Err(EngineError::PdfProtected(PdfProtection::PrintingNotAllowed))
        ),
        "the engine refuses it: {engine:?}"
    );

    let r = sumatra::spool(
        &pdf,
        queue,
        &JobSettings::default(),
        "no-print.pdf",
        TIMEOUT,
    );
    let _ = std::fs::remove_file(&pdf);
    match r {
        Err(PrintError::Engine(EngineError::PdfProtected(PdfProtection::PrintingNotAllowed))) => {
            println!("NOPRINT SumatraPDF refused the file itself");
        }
        Ok(o) => {
            if let Some(id) = o.os_job_id {
                let _ = cancel_os_job(queue, &id);
            }
            println!("NOPRINT SumatraPDF spooled the file: the engine's check is the only guard");
        }
        Err(e) => panic!("unexpected failure: {e:?}"),
    }
}

/// A one-page PDF with an owner password and no print permission.
fn no_print_pdf() -> PathBuf {
    use lopdf::{
        dictionary, Document, EncryptionState, EncryptionVersion, Object, Permissions, Stream,
    };
    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let content = doc.add_object(Stream::new(dictionary! {}, b"0 0 m 100 100 l S".to_vec()));
    let page = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
        "Contents" => content,
    });
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => vec![page.into()],
            "Count" => 1,
        }),
    );
    let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    doc.trailer.set("Root", catalog);
    // The Standard security handler keys the encryption on the file ID.
    let id = Object::String(
        b"pluryprint-noprt".to_vec(),
        lopdf::StringFormat::Hexadecimal,
    );
    doc.trailer.set("ID", vec![id.clone(), id]);
    let version = EncryptionVersion::V2 {
        document: &doc,
        owner_password: "owner-secret",
        user_password: "",
        key_length: 128,
        permissions: Permissions::COPYABLE,
    };
    let state = EncryptionState::try_from(version).expect("encryption state");
    doc.encrypt(&state).expect("encrypt");
    let path =
        std::env::temp_dir().join(format!("pluryprint-qt-{}-noprint.pdf", std::process::id()));
    doc.save(&path).expect("save");
    path
}

/// This test binary, re-run as a single child test with `role`.
fn child(role: &str, queue: &str) -> Command {
    let mut c = Command::new(std::env::current_exe().unwrap());
    c.args([
        "print_quality_child_process",
        "--exact",
        "--ignored",
        "--nocapture",
        "--test-threads=1",
    ])
    .env(ROLE_ENV, role)
    .env(QUEUE_ENV, queue);
    if role == "abort" {
        c.env(swap::ABORT_AFTER_SWAP_ENV, "1");
    }
    c
}

trait SpawnPiped {
    fn output_spawn(self) -> std::process::Child;
}

impl SpawnPiped for Command {
    fn output_spawn(mut self) -> std::process::Child {
        self.stdout(std::process::Stdio::piped())
            .spawn()
            .expect("spawn child")
    }
}

/// The child half of the multi-process tests; does nothing when run alone.
#[test]
#[ignore = "child process of the multi-process quality tests"]
fn print_quality_child_process() {
    let (Ok(role), Ok(queue)) = (std::env::var(ROLE_ENV), std::env::var(QUEUE_ENV)) else {
        return;
    };
    let quality = match role.as_str() {
        "high" => Quality::High,
        _ => Quality::Draft,
    };
    // "abort" exits inside the swap (ABORT_AFTER_SWAP_ENV) and never returns.
    let id = spool(&queue, &settings(quality), &role);
    println!("PLURYPRINT_JOB={id}");
}
