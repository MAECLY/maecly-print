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

/// Pick the job SumatraPDF created: among the jobs that were not on the
/// printer before spooling (`before`), the one whose document name mentions
/// `file_name`; failing that, the only new job, if there is exactly one.
/// `None` when it cannot be told apart from somebody else's job — the
/// dispatcher then treats the print as finished on hand-off, as it did before
/// tracking existed, rather than follow a stranger's job.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn pick_new_job(before: &[u32], after: &[(u32, String)], file_name: &str) -> Option<u32> {
    let fresh: Vec<&(u32, String)> = after
        .iter()
        .filter(|(id, _)| !before.contains(id))
        .collect();
    let needle = file_name.to_lowercase();
    if !needle.is_empty() {
        if let Some((id, _)) = fresh
            .iter()
            .find(|(_, doc)| doc.to_lowercase().contains(&needle))
        {
            return Some(*id);
        }
    }
    match fresh.as_slice() {
        [(id, _)] => Some(*id),
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

    /// Every job on `printer` as `(job id, document name)`.
    pub fn jobs(printer: &str) -> Result<Vec<(u32, String)>, TrackError> {
        let p = Printer::open(printer)?;
        let mut needed = 0u32;
        let mut returned = 0u32;
        // First call sizes the buffer; it "fails" with ERROR_INSUFFICIENT_BUFFER.
        // SAFETY: out pointers are valid for the call.
        let _ = unsafe { EnumJobsW(p.0, 0, u32::MAX, 1, None, &mut needed, &mut returned) };
        if needed == 0 {
            return Ok(Vec::new());
        }
        // u64-aligned storage: JOB_INFO_1W holds pointers and a SYSTEMTIME.
        let mut storage = vec![0u64; (needed as usize).div_ceil(8)];
        // SAFETY: `storage` is at least `needed` bytes and lives across the call.
        let buf = unsafe {
            std::slice::from_raw_parts_mut(storage.as_mut_ptr().cast::<u8>(), storage.len() * 8)
        };
        unsafe { EnumJobsW(p.0, 0, u32::MAX, 1, Some(buf), &mut needed, &mut returned) }
            .map_err(|e| TrackError::Transient(format!("EnumJobs failed: {e}")))?;
        // SAFETY: the spooler wrote `returned` JOB_INFO_1W records at the start
        // of the (suitably aligned) buffer.
        let infos = unsafe {
            std::slice::from_raw_parts(storage.as_ptr().cast::<JOB_INFO_1W>(), returned as usize)
        };
        Ok(infos.iter().map(|j| (j.JobId, wstr(j.pDocument))).collect())
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

/// The ids of the jobs currently on `printer` (the "before" snapshot).
#[cfg(windows)]
pub fn job_ids(printer: &str) -> Vec<u32> {
    ffi::jobs(printer)
        .map(|v| v.into_iter().map(|(id, _)| id).collect())
        .unwrap_or_default()
}

/// The job SumatraPDF just created on `printer`, given the ids that were there
/// before (see [`pick_new_job`]).
#[cfg(windows)]
pub fn find_spooled_job(printer: &str, before: &[u32], file_name: &str) -> Option<u32> {
    let after = ffi::jobs(printer).ok()?;
    pick_new_job(before, &after, file_name)
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

    #[test]
    fn the_new_job_is_found_by_document_name() {
        let before = [3, 4];
        let after = vec![
            (3, "old.pdf".to_string()),
            (5, "someone-else.docx".to_string()),
            (6, "C:\\cache\\Report.PDF".to_string()),
        ];
        assert_eq!(pick_new_job(&before, &after, "report.pdf"), Some(6));
    }

    #[test]
    fn a_single_new_job_is_ours_but_an_ambiguous_set_is_not_guessed() {
        let only = vec![(1, "a".to_string()), (9, "Microsoft Word".to_string())];
        assert_eq!(pick_new_job(&[1], &only, "x.pdf"), Some(9));

        let two = vec![(8, "one".to_string()), (9, "two".to_string())];
        assert_eq!(pick_new_job(&[], &two, "x.pdf"), None);
        assert_eq!(pick_new_job(&[8, 9], &two, "x.pdf"), None, "nothing new");
    }
}
