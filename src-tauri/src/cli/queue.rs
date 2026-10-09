//! `maeclyprint queue …`
//!
//! The queue the CLI talks to is the **persisted** one (`queue.json`), read and
//! written through [`crate::queue::persist`] and mutated only through the same
//! [`can_transition`] guard the dispatcher uses.
//!
//! What this deliberately does *not* do is reach into a running app. A live
//! instance owns its queue in memory and rewrites `queue.json` on every
//! transition, so a cancel written here while the app is open would be
//! overwritten by the next transition — and reported as a success while the
//! job kept printing. So `cancel` / `cancel-all` first check the app's
//! instance lock ([`crate::cli::instance`]) and, when the app is running,
//! **fail** (exit 1, `InvalidState`) without touching the file. The offline
//! path is what makes `queue status` useful in a script (it reads what the app
//! persists) and `queue cancel` useful on a queue left behind by a closed app;
//! cancelling inside a *running* app is the UI's job.

use std::path::Path;

use crate::cli::args::QueueCmd;
use crate::cli::exit::CliError;
use crate::cli::output::{table, Emitter};
use crate::cli::Paths;
use crate::queue::job::{can_transition, now_rfc3339, JobState, QueueSnapshot};
use crate::queue::persist;
use crate::util::error::IpcError;

/// How often `queue status --watch` re-reads `queue.json`. Frequent enough to
/// feel live, rare enough to be free.
const WATCH_INTERVAL: std::time::Duration = std::time::Duration::from_millis(750);

/// Run a `queue` subcommand.
pub fn run(paths: &Paths, emitter: Emitter, cmd: QueueCmd) -> Result<(), CliError> {
    match cmd {
        QueueCmd::Status { watch } => {
            let snapshot = persist::load(&paths.queue);
            emitter.emit(&snapshot, || render(&snapshot));
            if watch {
                watch_queue(&paths.queue, emitter, snapshot);
            }
            Ok(())
        }
        QueueCmd::Cancel { id } => {
            refuse_while_app_runs(&paths.queue)?;
            let mut snapshot = persist::load(&paths.queue);
            let job = snapshot
                .jobs
                .iter_mut()
                .find(|j| j.id == id)
                .ok_or_else(|| CliError::from(IpcError::JobNotFound(id.clone())))?;
            if !can_transition(job.state, JobState::Cancelled) {
                return Err(IpcError::InvalidState(format!(
                    "job {id} is already {}",
                    state_name(job.state)
                ))
                .into());
            }
            let os_job = os_job_of(job);
            job.state = JobState::Cancelled;
            job.progress = None;
            job.finished_at = Some(now_rfc3339());

            // Again right before the write: an app that started while we were
            // reading now owns the file, and our snapshot is already stale.
            refuse_while_app_runs(&paths.queue)?;
            save(&paths.queue, &snapshot)?;
            cancel_os_jobs(os_job);
            emitter.emit(&serde_json::json!({ "cancelled": [id.clone()] }), || {
                format!("Cancelled job {id}")
            });
            Ok(())
        }
        QueueCmd::CancelAll => {
            refuse_while_app_runs(&paths.queue)?;
            let mut snapshot = persist::load(&paths.queue);
            let finished_at = now_rfc3339();
            let mut cancelled = Vec::new();
            let mut os_jobs = Vec::new();
            for job in &mut snapshot.jobs {
                if can_transition(job.state, JobState::Cancelled) {
                    os_jobs.extend(os_job_of(job));
                    job.state = JobState::Cancelled;
                    job.progress = None;
                    job.finished_at = Some(finished_at.clone());
                    cancelled.push(job.id.clone());
                }
            }
            refuse_while_app_runs(&paths.queue)?;
            save(&paths.queue, &snapshot)?;
            cancel_os_jobs(os_jobs);
            let count = cancelled.len();
            emitter.emit(&serde_json::json!({ "cancelled": cancelled }), || {
                format!("Cancelled {count} job(s)")
            });
            Ok(())
        }
    }
}

/// The `(printer, OS job id)` a job left in the system print queue, if any. A
/// job the app had already spooled when it closed stays `Printing` with its OS
/// job id ([`persist::recover`]), so cancelling it here has to reach the
/// spooler too, or the paper keeps coming out of a job reported cancelled.
fn os_job_of(job: &crate::queue::job::Job) -> Option<(String, String)> {
    persist::is_handed_off(job)
        .then(|| job.printer_resolved.clone().zip(job.os_job_id.clone()))
        .flatten()
}

/// Best-effort cancellation at the spooler. A job that already left the OS
/// queue cannot be cancelled there, which is not a failure of this command.
fn cancel_os_jobs(jobs: impl IntoIterator<Item = (String, String)>) {
    for (printer, oid) in jobs {
        if let Err(e) = crate::engine::print::cancel_os_job(&printer, &oid) {
            tracing::warn!(event = "os_job_cancel_failed", os_job_id = %oid, error = %e);
        }
    }
}

/// Re-print the queue whenever the file changes. Runs until the user
/// interrupts the process, which is why it has no exit path of its own.
fn watch_queue(path: &Path, emitter: Emitter, mut last: QueueSnapshot) {
    loop {
        std::thread::sleep(WATCH_INTERVAL);
        let snapshot = persist::load(path);
        if snapshot != last {
            emitter.emit(&snapshot, || render(&snapshot));
            last = snapshot;
        }
    }
}

/// Refuse to edit `queue.json` while the app is open (see
/// [`crate::cli::instance`]). The running app owns the live queue: a cancel
/// written to the file would be reported as done while the job kept printing,
/// and the stale snapshot we would write could drop jobs the app saved in the
/// meantime. Failing loudly, with nothing changed, is the honest answer.
fn refuse_while_app_runs(queue_json: &Path) -> Result<(), CliError> {
    if crate::cli::instance::is_held(&crate::cli::instance::lock_path_for(queue_json)) {
        return Err(IpcError::InvalidState(
            "MaeclyPrint is open and owns the live queue, so nothing was changed — \
             cancel from its Queue screen, or close the app and run this again"
                .to_string(),
        )
        .into());
    }
    Ok(())
}

fn save(path: &Path, snapshot: &QueueSnapshot) -> Result<(), CliError> {
    persist::save(path, snapshot)
        .map_err(|e| CliError::from(IpcError::Internal(format!("could not save the queue: {e}"))))
}

/// Human-mode rendering: the pause flag, then one row per job.
fn render(snapshot: &QueueSnapshot) -> String {
    let rows: Vec<Vec<String>> = snapshot
        .jobs
        .iter()
        .map(|j| {
            vec![
                j.id.clone(),
                state_name(j.state).to_string(),
                j.file_name.clone(),
                j.printer_resolved.clone().unwrap_or_default(),
                j.error
                    .as_ref()
                    .map(|e| e.message.clone())
                    .unwrap_or_default(),
            ]
        })
        .collect();

    let header = if snapshot.queue_paused {
        format!("Queue: paused ({} job(s))", snapshot.jobs.len())
    } else {
        format!("Queue: running ({} job(s))", snapshot.jobs.len())
    };
    format!(
        "{header}\n{}",
        table(&["ID", "STATE", "FILE", "PRINTER", "ERROR"], &rows)
    )
}

/// The wire spelling of a state, so human output and `--json` agree.
fn state_name(state: JobState) -> &'static str {
    match state {
        JobState::Queued => "queued",
        JobState::Converting => "converting",
        JobState::Converted => "converted",
        JobState::Printing => "printing",
        JobState::Done => "done",
        JobState::Failed => "failed",
        JobState::Paused => "paused",
        JobState::Cancelled => "cancelled",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::queue::job::{Job, JobSettings};

    fn snapshot_with(states: &[JobState]) -> QueueSnapshot {
        let jobs = states
            .iter()
            .enumerate()
            .map(|(i, state)| {
                let mut job = Job::new(
                    "batch",
                    format!("/tmp/f{i}.pdf"),
                    JobSettings::default(),
                    None,
                    now_rfc3339(),
                );
                job.id = format!("job-{i}");
                job.state = *state;
                job
            })
            .collect();
        QueueSnapshot {
            jobs,
            ..Default::default()
        }
    }

    fn queue_paths(dir: &Path) -> Paths {
        Paths {
            config: dir.join("config.toml"),
            profiles: dir.join("profiles.toml"),
            queue: dir.join("queue.json"),
        }
    }

    #[test]
    fn fr_009_cancel_of_an_unknown_job_is_generic() {
        let dir = tempfile::tempdir().unwrap();
        let paths = queue_paths(dir.path());
        persist::save(&paths.queue, &snapshot_with(&[JobState::Queued])).unwrap();

        let err = run(
            &paths,
            Emitter::new(true),
            QueueCmd::Cancel {
                id: "nope".to_string(),
            },
        )
        .unwrap_err();
        assert_eq!(err.exit, crate::cli::exit::ExitCode::Generic);
        assert!(matches!(err.error, IpcError::JobNotFound(_)));
    }

    #[test]
    fn fr_009_cancel_of_a_terminal_job_is_invalid_state() {
        let dir = tempfile::tempdir().unwrap();
        let paths = queue_paths(dir.path());
        persist::save(&paths.queue, &snapshot_with(&[JobState::Done])).unwrap();

        let err = run(
            &paths,
            Emitter::new(true),
            QueueCmd::Cancel {
                id: "job-0".to_string(),
            },
        )
        .unwrap_err();
        assert!(matches!(err.error, IpcError::InvalidState(_)));
    }

    /// With the app open, the live queue lives in its memory: editing
    /// `queue.json` would report a cancel that never happens. The CLI must
    /// refuse — non-zero, and without touching the file.
    #[test]
    fn fr_009_cancel_while_the_app_runs_fails_and_changes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let paths = queue_paths(dir.path());
        persist::save(&paths.queue, &snapshot_with(&[JobState::Queued])).unwrap();
        let _app = crate::cli::instance::claim(&crate::cli::instance::lock_path_for(&paths.queue))
            .unwrap()
            .expect("the test plays the running app");

        let err = run(
            &paths,
            Emitter::new(true),
            QueueCmd::Cancel {
                id: "job-0".to_string(),
            },
        )
        .unwrap_err();
        assert!(matches!(err.error, IpcError::InvalidState(_)));
        assert_ne!(err.exit, crate::cli::exit::ExitCode::Success);

        let err = run(&paths, Emitter::new(true), QueueCmd::CancelAll).unwrap_err();
        assert!(matches!(err.error, IpcError::InvalidState(_)));

        assert_eq!(
            persist::load(&paths.queue).jobs[0].state,
            JobState::Queued,
            "nothing may be written behind the running app's back"
        );
    }

    #[test]
    fn fr_032_only_a_spooled_job_has_an_os_job_to_cancel() {
        let mut snapshot = snapshot_with(&[JobState::Printing, JobState::Printing]);
        snapshot.jobs[0].os_job_id = Some("Office-42".to_string());
        snapshot.jobs[0].printer_resolved = Some("Office".to_string());
        assert_eq!(
            os_job_of(&snapshot.jobs[0]),
            Some(("Office".to_string(), "Office-42".to_string()))
        );
        assert_eq!(os_job_of(&snapshot.jobs[1]), None, "never spooled");
    }

    #[test]
    fn fr_008_cancel_all_spares_terminal_jobs() {
        let dir = tempfile::tempdir().unwrap();
        let paths = queue_paths(dir.path());
        persist::save(
            &paths.queue,
            &snapshot_with(&[JobState::Queued, JobState::Done, JobState::Paused]),
        )
        .unwrap();

        run(&paths, Emitter::new(true), QueueCmd::CancelAll).unwrap();

        let after = persist::load(&paths.queue);
        assert_eq!(after.jobs[0].state, JobState::Cancelled);
        assert_eq!(
            after.jobs[1].state,
            JobState::Done,
            "terminal job untouched"
        );
        assert_eq!(after.jobs[2].state, JobState::Cancelled);
        assert!(after.jobs[0].finished_at.is_some());
    }
}
