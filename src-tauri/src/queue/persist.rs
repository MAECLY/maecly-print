//! On-disk persistence for the print queue (`queue.json`).
//!
//! The queue is machine-managed JSON rewritten on every state transition, so
//! writes are atomic (temp file + rename) exactly like [`crate::settings`], and
//! reads never partial-load: a missing, corrupt, or too-new file falls back to
//! an empty queue rather than crashing (FR-030, FR-101,
//! `docs/04-architecture/data-model.md` §4).
//!
//! This module is pure persistence plus the shutdown-recovery transform
//! ([`recover`]); it does not run jobs.

use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::Context;

use crate::queue::job::{Job, JobError, QueueSnapshot, SCHEMA_VERSION};
use crate::util::paths;

/// Terminal (history) jobs kept in `queue.json`. Older completed/failed/
/// cancelled jobs beyond this are pruned on save so the file cannot grow
/// without bound.
pub const TERMINAL_HISTORY_CAP: usize = 200;

/// Absolute path of `queue.json` in the app data dir
/// (`<data_dir>/queue.json`), reusing the crate's [`paths`] helper so the
/// directory name lives in exactly one place.
pub fn queue_file_path() -> anyhow::Result<PathBuf> {
    Ok(paths::data_dir().join("queue.json"))
}

/// Atomically write `snap` to `path` as pretty JSON.
///
/// Creates the parent directory if missing, prunes history first
/// ([`prune_history`] with [`TERMINAL_HISTORY_CAP`]), then writes to a temp
/// file in the same directory and `persist`s (atomic rename on the same
/// volume). The `snap` argument is not mutated; pruning applies only to the
/// bytes on disk.
pub fn save(path: &Path, snap: &QueueSnapshot) -> anyhow::Result<()> {
    let parent = path
        .parent()
        .context("queue file path has no parent directory")?;
    paths::ensure_dir(parent).context("failed to create queue data directory")?;

    let pruned = QueueSnapshot {
        schema_version: snap.schema_version,
        queue_paused: snap.queue_paused,
        jobs: prune_history(snap.jobs.clone(), TERMINAL_HISTORY_CAP),
    };

    let json = serde_json::to_string_pretty(&pruned).context("failed to serialize queue")?;

    let mut tmp =
        tempfile::NamedTempFile::new_in(parent).context("failed to create queue temp file")?;
    tmp.write_all(json.as_bytes())
        .context("failed to write queue temp file")?;
    tmp.flush().context("failed to flush queue temp file")?;
    tmp.persist(path)
        .map_err(|e| anyhow::anyhow!("failed to persist queue file: {e}"))?;
    Ok(())
}

/// Load the queue from `path`, never failing.
///
/// - Missing file → default (empty) snapshot, no side effects.
/// - Unreadable, corrupt JSON, or `schema_version` greater than
///   [`SCHEMA_VERSION`] → the offending file is renamed to `queue.json.bak`
///   (best-effort), a warning is logged, and a default snapshot is returned.
///
/// Never partial-loads: a file is taken whole or discarded whole.
///
/// The returned snapshot has NOT been through shutdown recovery — call
/// [`recover`] on it before use.
pub fn load(path: &Path) -> QueueSnapshot {
    let raw = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return QueueSnapshot::default();
        }
        Err(e) => {
            tracing::warn!(
                event = "queue_read_failed",
                error = %e,
                "could not read queue.json; starting with an empty queue"
            );
            return QueueSnapshot::default();
        }
    };

    match serde_json::from_str::<QueueSnapshot>(&raw) {
        Ok(snap) if snap.schema_version <= SCHEMA_VERSION => snap,
        Ok(snap) => {
            tracing::warn!(
                event = "queue_schema_too_new",
                found = snap.schema_version,
                supported = SCHEMA_VERSION,
                "queue.json was written by a newer Pluryprint; backing up and ignoring"
            );
            backup_corrupt(path);
            QueueSnapshot::default()
        }
        Err(e) => {
            tracing::warn!(
                event = "queue_parse_failed",
                error = %e,
                "queue.json is corrupt; backing up and starting with an empty queue"
            );
            backup_corrupt(path);
            QueueSnapshot::default()
        }
    }
}

/// Shutdown recovery (NFR-010): any job left [`Converting`] or [`Printing`]
/// when the app closed is moved to [`Paused`] with the
/// `"interrupted_by_shutdown"` error and its transient progress cleared. All
/// other states — including [`Converted`], which keeps its cached PDF — are
/// left untouched. Pure; run this on the result of [`load`] before exposing the
/// queue.
///
/// One exception: a [`Printing`] job the spooler had already accepted (it
/// carries its OS job id and the printer it went to) is **not** paused. Its
/// paper may be coming out right now, and resuming it would spool a second
/// copy; it stays `Printing` and the dispatcher picks the OS job up again
/// when it starts ([`crate::queue::dispatcher`]'s bootstrap), so it ends as
/// whatever the spooler says (done, cancelled, aborted). There is one print
/// slot, so at most one such job can exist; any other is paused as usual.
///
/// [`Converting`]: crate::queue::job::JobState::Converting
/// [`Printing`]: crate::queue::job::JobState::Printing
/// [`Paused`]: crate::queue::job::JobState::Paused
/// [`Converted`]: crate::queue::job::JobState::Converted
pub fn recover(snap: QueueSnapshot) -> QueueSnapshot {
    let QueueSnapshot {
        schema_version,
        queue_paused,
        jobs,
    } = snap;

    let mut tracking = false;
    let jobs = jobs
        .into_iter()
        .map(|mut job| {
            if !tracking && is_handed_off(&job) {
                tracking = true;
                job.progress = Some(crate::queue::job::JobProgress::indeterminate("printing"));
                return job;
            }
            if job.state.is_active() {
                job.state = crate::queue::job::JobState::Paused;
                job.error = Some(JobError::new(
                    "interrupted_by_shutdown",
                    "Pluryprint was closed while this job was in progress.",
                ));
                job.progress = None;
            }
            job
        })
        .collect();

    QueueSnapshot {
        schema_version,
        queue_paused,
        jobs,
    }
}

/// Whether `job` was spooled and is waiting on its OS job: `Printing`, with
/// the spooler's id and the printer it went to.
pub fn is_handed_off(job: &Job) -> bool {
    job.state == crate::queue::job::JobState::Printing
        && job.os_job_id.is_some()
        && job.printer_resolved.is_some()
}

/// Trim job history: keep every non-terminal job, plus the `cap` most recent
/// terminal jobs (ranked by `finished_at`, falling back to `submitted_at`).
/// The relative order of the kept jobs is preserved.
///
/// Pure and independent of I/O so it can be unit-tested directly; [`save`]
/// applies it before writing.
pub fn prune_history(jobs: Vec<Job>, cap: usize) -> Vec<Job> {
    let terminal_count = jobs.iter().filter(|j| j.state.is_terminal()).count();
    if terminal_count <= cap {
        return jobs;
    }

    // Rank terminal jobs by recency. Parse the RFC 3339 timestamps rather than
    // comparing strings: `time`'s formatter emits fractional seconds only when
    // non-zero, and "…:00Z" compares lexicographically *after* "…:00.5Z".
    // Unparseable timestamps rank oldest, so they are pruned first.
    fn recency_key(j: &Job) -> time::OffsetDateTime {
        let raw = j.finished_at.as_deref().unwrap_or(j.submitted_at.as_str());
        time::OffsetDateTime::parse(raw, &time::format_description::well_known::Rfc3339)
            .unwrap_or(time::OffsetDateTime::UNIX_EPOCH)
    }
    let mut terminal: Vec<(usize, time::OffsetDateTime)> = jobs
        .iter()
        .enumerate()
        .filter(|(_, j)| j.state.is_terminal())
        .map(|(i, j)| (i, recency_key(j)))
        .collect();
    terminal.sort_by_key(|&(_, key)| std::cmp::Reverse(key)); // most recent first

    let keep: std::collections::HashSet<usize> =
        terminal.iter().take(cap).map(|(i, _)| *i).collect();

    jobs.into_iter()
        .enumerate()
        .filter(|(i, j)| !j.state.is_terminal() || keep.contains(i))
        .map(|(_, j)| j)
        .collect()
}

/// Best-effort rename of an unreadable queue file to `queue.json.bak` so the
/// data is preserved for debugging but a fresh file can be written. Failure to
/// rename is logged, not propagated.
fn backup_corrupt(path: &Path) {
    let bak = path.with_extension("json.bak");
    if let Err(e) = std::fs::rename(path, &bak) {
        tracing::warn!(
            event = "queue_backup_failed",
            error = %e,
            "could not back up corrupt queue.json"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::queue::job::{JobSettings, JobState};

    /// Build a job in a given state with an explicit timestamp, for tests.
    fn job(state: JobState, submitted_at: &str) -> Job {
        let mut j = Job::new(
            "batch",
            "/tmp/file.pdf",
            JobSettings::default(),
            None,
            submitted_at.to_string(),
        );
        j.state = state;
        if state.is_terminal() {
            j.finished_at = Some(submitted_at.to_string());
        }
        j
    }

    #[test]
    fn round_trip_save_load() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("queue.json");

        let snap = QueueSnapshot {
            schema_version: SCHEMA_VERSION,
            queue_paused: true,
            jobs: vec![
                job(JobState::Queued, "2026-07-11T10:00:00Z"),
                job(JobState::Done, "2026-07-11T09:00:00Z"),
            ],
        };
        save(&path, &snap).unwrap();
        let loaded = load(&path);
        assert_eq!(loaded, snap);
    }

    #[test]
    fn missing_file_yields_default() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("queue.json");
        let loaded = load(&path);
        assert_eq!(loaded, QueueSnapshot::default());
        // No .bak is created for a simply-absent file.
        assert!(!path.with_extension("json.bak").exists());
    }

    #[test]
    fn corrupt_file_backs_up_and_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("queue.json");
        std::fs::write(&path, b"{ this is not valid json ]").unwrap();

        let loaded = load(&path);
        assert_eq!(loaded, QueueSnapshot::default());
        assert!(
            path.with_extension("json.bak").exists(),
            "corrupt file must be preserved as queue.json.bak"
        );
    }

    #[test]
    fn future_schema_backs_up_and_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("queue.json");
        std::fs::write(
            &path,
            br#"{"schema_version":999,"queue_paused":false,"jobs":[]}"#,
        )
        .unwrap();

        let loaded = load(&path);
        assert_eq!(loaded, QueueSnapshot::default());
        assert!(path.with_extension("json.bak").exists());
    }

    #[test]
    fn recover_pauses_in_flight_jobs_only() {
        let snap = QueueSnapshot {
            schema_version: SCHEMA_VERSION,
            queue_paused: false,
            jobs: vec![
                job(JobState::Converting, "2026-07-11T10:00:00Z"),
                job(JobState::Printing, "2026-07-11T10:01:00Z"),
                job(JobState::Queued, "2026-07-11T10:02:00Z"),
                job(JobState::Converted, "2026-07-11T10:03:00Z"),
                job(JobState::Done, "2026-07-11T10:04:00Z"),
            ],
        };
        let recovered = recover(snap);
        let states: Vec<JobState> = recovered.jobs.iter().map(|j| j.state).collect();
        assert_eq!(
            states,
            vec![
                JobState::Paused,    // was converting
                JobState::Paused,    // was printing
                JobState::Queued,    // untouched
                JobState::Converted, // keeps its cached PDF
                JobState::Done,      // untouched
            ]
        );
        for j in recovered.jobs.iter().take(2) {
            let err = j.error.as_ref().expect("recovered job carries an error");
            assert_eq!(err.code, "interrupted_by_shutdown");
            assert!(j.progress.is_none());
        }
        // The untouched jobs never gained an error.
        assert!(recovered.jobs[2].error.is_none());
        assert!(recovered.jobs[3].error.is_none());
    }

    /// NFR-010 / FR-032: a print the spooler already accepted is not resumable
    /// work. Pausing it as "interrupted" made Resume spool it a second time
    /// while the first copy was still in the printer's queue.
    #[test]
    fn nfr_010_a_spooled_print_keeps_tracking_across_a_restart() {
        let mut spooled = job(JobState::Printing, "2026-07-11T10:00:00Z");
        spooled.os_job_id = Some("Office-42".to_string());
        spooled.printer_resolved = Some("Office".to_string());
        // Spooling had not returned an id yet: nothing to follow.
        let mid_spool = job(JobState::Printing, "2026-07-11T10:01:00Z");

        let recovered = recover(QueueSnapshot {
            schema_version: SCHEMA_VERSION,
            queue_paused: false,
            jobs: vec![spooled, mid_spool],
        });
        let kept = &recovered.jobs[0];
        assert_eq!(kept.state, JobState::Printing);
        assert!(kept.error.is_none());
        assert_eq!(kept.os_job_id.as_deref(), Some("Office-42"));
        assert_eq!(
            kept.progress.as_ref().map(|p| p.phase.as_str()),
            Some("printing")
        );
        assert_eq!(recovered.jobs[1].state, JobState::Paused);
        assert_eq!(
            recovered.jobs[1].error.as_ref().unwrap().code,
            "interrupted_by_shutdown"
        );
    }

    #[test]
    fn prune_keeps_non_terminal_and_caps_terminal() {
        let mut jobs = Vec::new();
        // 5 terminal (done) jobs with increasing timestamps.
        for i in 0..5 {
            jobs.push(job(JobState::Done, &format!("2026-07-11T10:0{i}:00Z")));
        }
        // 2 non-terminal jobs that must always survive.
        jobs.push(job(JobState::Queued, "2026-07-11T11:00:00Z"));
        jobs.push(job(JobState::Printing, "2026-07-11T11:01:00Z"));

        let pruned = prune_history(jobs, 2);
        let terminal: Vec<&Job> = pruned.iter().filter(|j| j.state.is_terminal()).collect();
        let non_terminal = pruned.iter().filter(|j| !j.state.is_terminal()).count();

        assert_eq!(non_terminal, 2, "all non-terminal jobs are retained");
        assert_eq!(terminal.len(), 2, "terminal jobs capped at 2");
        // The two most recent terminals (10:03, 10:04) are the ones kept.
        let kept_times: Vec<&str> = terminal
            .iter()
            .map(|j| j.finished_at.as_deref().unwrap())
            .collect();
        assert!(kept_times.contains(&"2026-07-11T10:04:00Z"));
        assert!(kept_times.contains(&"2026-07-11T10:03:00Z"));
    }

    #[test]
    fn prune_is_noop_under_cap() {
        let jobs = vec![
            job(JobState::Done, "2026-07-11T10:00:00Z"),
            job(JobState::Failed, "2026-07-11T10:01:00Z"),
        ];
        let pruned = prune_history(jobs.clone(), 200);
        assert_eq!(pruned.len(), jobs.len());
    }

    #[test]
    fn save_prunes_history_on_disk() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("queue.json");

        let mut jobs = Vec::new();
        for i in 0..250 {
            jobs.push(job(
                JobState::Done,
                &format!("2026-07-11T10:00:{:02}Z", i % 60),
            ));
        }
        let snap = QueueSnapshot {
            schema_version: SCHEMA_VERSION,
            queue_paused: false,
            jobs,
        };
        save(&path, &snap).unwrap();
        let loaded = load(&path);
        assert_eq!(loaded.jobs.len(), TERMINAL_HISTORY_CAP);
    }
}
