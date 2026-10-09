//! Windows spooler job tracking: finding the job SumatraPDF just spooled and
//! following it until it leaves the queue.
//!
//! SumatraPDF reports no job id — it hands the document to GDI and exits — so
//! the id is recovered from the spooler itself: the job ids on the printer are
//! listed before spooling ([`job_ids`]) and again after, and the new job whose
//! document name matches the file is ours ([`pick_new_job`]). From then on
//! [`job_state`] reads it with `GetJobW` and maps the `JOB_STATUS_*` flags onto
//! [`OsJobState`] ([`map_status`]).
//!
//! Every call opens the printer, asks once and closes it again; nothing here
//! waits on the device, so a stalled printer can never wedge the dispatcher —
//! the poll cadence and the overall budget live in the dispatcher, bounded by
//! `print_timeout_seconds`.
//!
//! The mapping and the matching are pure and compiled on every platform so
//! they are tested on every CI lane; only the FFI is Windows-only.

use super::OsJobState;

// `JOB_STATUS_*` (winspool.h). Spelled out here so the pure mapping compiles —
// and is tested — off Windows too; the values are part of the Win32 ABI.
const PAUSED: u32 = 0x0001;
const ERROR: u32 = 0x0002;
const DELETING: u32 = 0x0004;
const OFFLINE: u32 = 0x0020;
const PAPEROUT: u32 = 0x0040;
const PRINTED: u32 = 0x0080;
const DELETED: u32 = 0x0100;
const BLOCKED_DEVQ: u32 = 0x0200;
const USER_INTERVENTION: u32 = 0x0400;
const COMPLETE: u32 = 0x1000;
const PRINTING: u32 = 0x0010;

/// Map a job's `JOB_STATUS_*` bit set onto [`OsJobState`].
///
/// Precedence matters because the spooler sets several bits at once:
///
/// - a printed or complete job is done, **even while it is being deleted**:
///   the spooler removes every job it finished printing (unless *Keep printed
///   documents* is on), so `PRINTED | DELETING` is the normal end of a
///   successful job, not a cancellation;
/// - a job being deleted that never printed was cancelled;
/// - a device problem (error, offline, paper out, a user prompt) means the
///   printer needs attention, which outranks "paused";
/// - and a job the user paused in the Windows queue is held.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn map_status(status: u32) -> OsJobState {
    if status & (PRINTED | COMPLETE) != 0 {
        OsJobState::Completed
    } else if status & (DELETING | DELETED) != 0 {
        OsJobState::Canceled
    } else if status & (ERROR | OFFLINE | PAPEROUT | USER_INTERVENTION | BLOCKED_DEVQ) != 0 {
        OsJobState::Stopped
    } else if status & PAUSED != 0 {
        OsJobState::Held
    } else if status & PRINTING != 0 {
        OsJobState::Processing
    } else {
        OsJobState::Pending
    }
}

/// One job on a spooler queue, as `EnumJobs` level 1 reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueuedJob {
    /// The spooler's job id.
    pub id: u32,
    /// `pDocument`: SumatraPDF names the job after the full path of the file it
    /// printed (`C:\cache\…\report.pdf`).
    pub document: String,
    /// `pUserName`: who submitted it. On a `\\server\queue` connection the
    /// list holds every user's jobs.
    pub user: String,
}

/// Whether `document` (a job's `pDocument`) names `pdf`: the same full path,
/// or a path whose last component is exactly `pdf`'s file name. Compared
/// case-insensitively, as Windows paths are. A substring is not enough —
/// `a.pdf` must not claim somebody's `data.pdf`.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn document_names(document: &str, pdf: &str) -> bool {
    let doc = document.trim();
    if doc.is_empty() || pdf.is_empty() {
        return false;
    }
    if doc.to_lowercase() == pdf.to_lowercase() {
        return true;
    }
    let last = |s: &str| s.rsplit(['\\', '/']).next().unwrap_or(s).to_lowercase();
    let file = last(pdf);
    !file.is_empty() && last(doc) == file
}

/// Pick the job SumatraPDF created: among the jobs that were not on the
/// printer before spooling (`before`), the one submitted by `user` whose
/// document is `pdf` ([`document_names`]).
///
/// There is no "it is the only new job" fallback, and no guess between two
/// candidates: on a shared print-server queue a new job can be a colleague's,
/// and following it would report (and on cancel, delete) somebody else's
/// print. `None` sends the dispatcher down its untracked path — the print is
/// treated as finished on hand-off, as it was before tracking existed.
///
/// A job with no user name, or a caller with none, is not filtered on it.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn pick_new_job(before: &[u32], after: &[QueuedJob], pdf: &str, user: &str) -> Option<u32> {
    let mut ours = after.iter().filter(|j| {
        !before.contains(&j.id)
            && (user.is_empty() || j.user.is_empty() || j.user.eq_ignore_ascii_case(user))
            && document_names(&j.document, pdf)
    });
    match (ours.next(), ours.next()) {
        (Some(job), None) => Some(job.id),
        _ => None,
    }
}

#[cfg(windows)]
mod ffi {
    use windows::core::{HSTRING, PCWSTR};
    use windows::Win32::Foundation::{
        GetLastError, ERROR_INSUFFICIENT_BUFFER, ERROR_INVALID_PARAMETER, HANDLE,
    };
    use windows::Win32::Graphics::Printing::{
        ClosePrinter, EnumJobsW, GetJobW, OpenPrinterW, SetJobW, JOB_CONTROL_DELETE, JOB_INFO_1W,
    };

    use super::super::TrackError;

    /// An open printer handle, closed on drop.
    struct Printer(HANDLE);

    impl Printer {
        fn open(name: &str) -> Result<Self, TrackError> {
            let wide = HSTRING::from(name);
            let mut handle = HANDLE::default();
            // SAFETY: `wide` outlives the call; `handle` is a valid out pointer.
            unsafe { OpenPrinterW(PCWSTR(wide.as_ptr()), &mut handle, None) }
                .map_err(|e| TrackError::Transient(format!("OpenPrinter('{name}') failed: {e}")))?;
            Ok(Self(handle))
        }
    }

    impl Drop for Printer {
        fn drop(&mut self) {
            // SAFETY: the handle came from a successful OpenPrinterW.
            let _ = unsafe { ClosePrinter(self.0) };
        }
    }

    /// Read a `PWSTR` field of a spooler struct (null → empty).
    fn wstr(p: windows::core::PWSTR) -> String {
        if p.is_null() {
            return String::new();
        }
        // SAFETY: the spooler returns NUL-terminated strings inside the buffer
        // that is still alive while this runs.
        unsafe { p.to_string() }.unwrap_or_default()
    }

    /// Every job on `printer`.
    ///
    /// The sizing call is expected to fail with `ERROR_INSUFFICIENT_BUFFER`
    /// (or succeed with nothing to report); any other failure is returned, not
    /// read as an empty queue — an empty "before" snapshot would make every
    /// job already on the printer look new.
    pub fn jobs(printer: &str) -> Result<Vec<super::QueuedJob>, TrackError> {
        let p = Printer::open(printer)?;
        let insufficient = ERROR_INSUFFICIENT_BUFFER.to_hresult();
        // A job arriving between the two calls makes the second one short of
        // room again, so the pair is retried a few times.
        for _ in 0..4 {
            let mut needed = 0u32;
            let mut returned = 0u32;
            // SAFETY: out pointers are valid for the call.
            match unsafe { EnumJobsW(p.0, 0, u32::MAX, 1, None, &mut needed, &mut returned) } {
                Ok(()) => return Ok(Vec::new()),
                Err(e) if e.code() == insufficient => {}
                Err(e) => return Err(TrackError::Transient(format!("EnumJobs failed: {e}"))),
            }
            if needed == 0 {
                return Ok(Vec::new());
            }
            // u64-aligned storage: JOB_INFO_1W holds pointers and a SYSTEMTIME.
            let mut storage = vec![0u64; (needed as usize).div_ceil(8)];
            // SAFETY: `storage` is at least `needed` bytes and lives across the call.
            let buf = unsafe {
                std::slice::from_raw_parts_mut(storage.as_mut_ptr().cast::<u8>(), storage.len() * 8)
            };
            match unsafe { EnumJobsW(p.0, 0, u32::MAX, 1, Some(buf), &mut needed, &mut returned) } {
                Ok(()) => {}
                Err(e) if e.code() == insufficient => continue,
                Err(e) => return Err(TrackError::Transient(format!("EnumJobs failed: {e}"))),
            }
            // SAFETY: the spooler wrote `returned` JOB_INFO_1W records at the
            // start of the (suitably aligned) buffer.
            let infos = unsafe {
                std::slice::from_raw_parts(
                    storage.as_ptr().cast::<JOB_INFO_1W>(),
                    returned as usize,
                )
            };
            return Ok(infos
                .iter()
                .map(|j| super::QueuedJob {
                    id: j.JobId,
                    document: wstr(j.pDocument),
                    user: wstr(j.pUserName),
                })
                .collect());
        }
        Err(TrackError::Transient(
            "the job list kept changing while it was being read".to_string(),
        ))
    }

    /// The `JOB_STATUS_*` bits of one job, or `None` when the spooler no longer
    /// knows it (it printed and was removed, or someone deleted it).
    pub fn status(printer: &str, job_id: u32) -> Result<Option<u32>, TrackError> {
        let p = Printer::open(printer)?;
        let mut needed = 0u32;
        // SAFETY: sizing call with no buffer; out pointer is valid.
        let ok = unsafe { GetJobW(p.0, job_id, 1, None, &mut needed) };
        if !ok.as_bool() {
            // SAFETY: plain thread-local read right after the failing call.
            let err = unsafe { GetLastError() };
            if err == ERROR_INVALID_PARAMETER {
                return Ok(None);
            }
            if err != ERROR_INSUFFICIENT_BUFFER {
                return Err(TrackError::Transient(format!("GetJob failed: {err:?}")));
            }
        }
        let mut storage = vec![0u64; (needed as usize).div_ceil(8).max(1)];
        // SAFETY: as in `jobs`.
        let buf = unsafe {
            std::slice::from_raw_parts_mut(storage.as_mut_ptr().cast::<u8>(), storage.len() * 8)
        };
        let ok = unsafe { GetJobW(p.0, job_id, 1, Some(buf), &mut needed) };
        if !ok.as_bool() {
            let err = unsafe { GetLastError() };
            if err == ERROR_INVALID_PARAMETER {
                return Ok(None);
            }
            return Err(TrackError::Transient(format!("GetJob failed: {err:?}")));
        }
        // SAFETY: the spooler wrote one JOB_INFO_1W at the buffer's start.
        let info = unsafe { &*storage.as_ptr().cast::<JOB_INFO_1W>() };
        Ok(Some(info.Status))
    }

    /// Delete a job from the queue (`JOB_CONTROL_DELETE`).
    pub fn delete(printer: &str, job_id: u32) -> anyhow::Result<()> {
        let p = Printer::open(printer).map_err(|e| anyhow::anyhow!("{e}"))?;
        // SAFETY: valid handle; level 0 with no struct is the documented form
        // for a pure control command.
        let ok = unsafe { SetJobW(p.0, job_id, 0, None, JOB_CONTROL_DELETE) };
        if ok.as_bool() {
            Ok(())
        } else {
            let err = unsafe { GetLastError() };
            anyhow::bail!("SetJob(DELETE) on job {job_id} failed: {err:?}")
        }
    }
}

/// The ids of the jobs currently on `printer` (the "before" snapshot). An
/// error means the snapshot is unknown, and the caller must not guess the new
/// job from the "after" list alone.
#[cfg(windows)]
pub fn job_ids(printer: &str) -> Result<Vec<u32>, super::TrackError> {
    ffi::jobs(printer).map(|v| v.into_iter().map(|j| j.id).collect())
}

/// The job SumatraPDF just created on `printer` for `pdf`, given the ids that
/// were there before (see [`pick_new_job`]). `None` — logged with the reason —
/// when the spooler cannot be read or the job cannot be told apart.
#[cfg(windows)]
pub fn find_spooled_job(printer: &str, before: &[u32], pdf: &str) -> Option<u32> {
    let user = std::env::var("USERNAME").unwrap_or_default();
    match ffi::jobs(printer) {
        Ok(after) => {
            let found = pick_new_job(before, &after, pdf, &user);
            if found.is_none() {
                tracing::info!(
                    event = "os_job_untracked",
                    printer = %printer,
                    reason = "no new job of ours with this document name",
                );
            }
            found
        }
        Err(e) => {
            tracing::warn!(
                event = "os_job_untracked",
                printer = %printer,
                reason = "the job list could not be read after spooling",
                error = %e,
            );
            None
        }
    }
}

/// Where the spooler job `os_job_id` stands on `printer`.
#[cfg(windows)]
pub fn job_state(printer: &str, os_job_id: &str) -> Result<OsJobState, super::TrackError> {
    let id: u32 = os_job_id.parse().map_err(|_| {
        super::TrackError::Unavailable(format!("'{os_job_id}' is not a spooler job id"))
    })?;
    Ok(match ffi::status(printer, id)? {
        None => OsJobState::Gone,
        Some(bits) => map_status(bits),
    })
}

/// Delete the spooler job `os_job_id` from `printer`.
#[cfg(windows)]
pub fn cancel(printer: &str, os_job_id: &str) -> anyhow::Result<()> {
    let id: u32 = os_job_id
        .parse()
        .map_err(|_| anyhow::anyhow!("'{os_job_id}' is not a spooler job id"))?;
    ffi::delete(printer, id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_bits_map_with_the_documented_precedence() {
        assert_eq!(map_status(0), OsJobState::Pending);
        assert_eq!(map_status(PRINTING), OsJobState::Processing);
        assert_eq!(map_status(PAUSED), OsJobState::Held);
        assert_eq!(map_status(PRINTING | PAPEROUT), OsJobState::Stopped);
        assert_eq!(map_status(PAUSED | OFFLINE), OsJobState::Stopped);
        assert_eq!(map_status(ERROR), OsJobState::Stopped);
        assert_eq!(map_status(USER_INTERVENTION), OsJobState::Stopped);
        assert_eq!(map_status(PRINTED), OsJobState::Completed);
        assert_eq!(map_status(COMPLETE | PRINTING), OsJobState::Completed);
        assert_eq!(map_status(DELETING | PRINTING), OsJobState::Canceled);
        assert_eq!(map_status(DELETED), OsJobState::Canceled);
    }

    /// FR-031: a job the spooler is removing *because it finished printing*
    /// is done, not cancelled — otherwise a poll that lands in that window
    /// reports a printed job as "cancelled in the system print queue".
    #[test]
    fn fr_031_a_printed_job_being_removed_is_completed_not_cancelled() {
        assert_eq!(map_status(DELETING | PRINTED), OsJobState::Completed);
        assert_eq!(map_status(DELETED | PRINTED), OsJobState::Completed);
        assert_eq!(map_status(DELETING | COMPLETE), OsJobState::Completed);
        assert_eq!(
            map_status(DELETING | PRINTED | PRINTING),
            OsJobState::Completed
        );
        // Deleted before anything came out: cancelled.
        assert_eq!(map_status(DELETING), OsJobState::Canceled);
        assert_eq!(map_status(DELETING | PAUSED), OsJobState::Canceled);
    }

    fn job(id: u32, document: &str, user: &str) -> QueuedJob {
        QueuedJob {
            id,
            document: document.to_string(),
            user: user.to_string(),
        }
    }

    /// FR-031: SumatraPDF names the job after the full path it printed
    /// (verified on a paused Windows 11 queue), so that is what is matched.
    #[test]
    fn fr_031_the_new_job_is_found_by_its_document_path() {
        let pdf = "C:\\cache\\conversions\\Report.pdf";
        let after = vec![
            job(3, "old.pdf", "me"),
            job(5, "someone-else.docx", "me"),
            job(6, "c:\\cache\\conversions\\report.PDF", "me"),
        ];
        assert_eq!(pick_new_job(&[3, 4], &after, pdf, "me"), Some(6));
        // The bare file name (a driver that drops the folder) still matches.
        let after = vec![job(7, "Report.pdf", "ME")];
        assert_eq!(pick_new_job(&[], &after, pdf, "me"), Some(7));
    }

    /// A substring is not a match: `a.pdf` must not claim `data.pdf`.
    #[test]
    fn fr_031_a_document_name_is_matched_whole_not_as_a_substring() {
        assert!(document_names("C:\\x\\a.pdf", "C:\\x\\a.pdf"));
        assert!(document_names("a.pdf", "C:\\y\\A.PDF"));
        assert!(!document_names("C:\\x\\data.pdf", "C:\\x\\a.pdf"));
        assert!(!document_names("data.pdf", "a.pdf"));
        assert!(!document_names("", "a.pdf"));
        assert!(!document_names("Microsoft Word - a", "a.pdf"));
    }

    /// On a shared print-server queue the only new job can be a colleague's:
    /// it is never adopted without a name match, and never when it is another
    /// user's, and two candidates are not guessed between.
    #[test]
    fn fr_031_another_users_or_an_unnamed_job_is_never_adopted() {
        let pdf = "C:\\cache\\a.pdf";
        // The only new job, but not ours by name: not adopted any more.
        let only = vec![job(1, "a", "me"), job(9, "Microsoft Word - b", "me")];
        assert_eq!(pick_new_job(&[1], &only, pdf, "me"), None);
        // Same file name, other user.
        let theirs = vec![job(9, "C:\\cache\\a.pdf", "colleague")];
        assert_eq!(pick_new_job(&[], &theirs, pdf, "me"), None);
        // Two of ours with the same name: ambiguous.
        let two = vec![job(8, "a.pdf", "me"), job(9, "a.pdf", "me")];
        assert_eq!(pick_new_job(&[], &two, pdf, "me"), None);
        // Nothing new.
        assert_eq!(pick_new_job(&[8, 9], &two, pdf, "me"), None);
        // A job with no user name, or no known caller, is not filtered on it.
        let anon = vec![job(4, "a.pdf", "")];
        assert_eq!(pick_new_job(&[], &anon, pdf, "me"), Some(4));
        assert_eq!(pick_new_job(&[], &theirs, pdf, ""), Some(9));
    }
}
