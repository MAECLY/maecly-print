//! `maeclyprint queue …`
//!
//! The queue the CLI talks to is the **persisted** one (`queue.json`), read and
//! written through [`crate::queue::persist`] and mutated only through the same
//! [`can_transition`] guard the dispatcher uses.
//!
//! What this deliberately does *not* do is reach into a running app. A live
//! instance owns its queue in memory and rewrites `queue.json` on every
//! transition, so a cancel issued here while the app is open would simply be
//! overwritten by the next transition — the writes are atomic, so nothing is
//! corrupted, but nothing is achieved either. The offline path is what makes
//! `queue status` useful in a script and `queue cancel` useful on a queue left
//! behind by a closed app; cancelling inside a *running* app is the UI's job
//! (and, for a forwarded invocation, the single-instance path in
//! [`crate::cli::single_instance`]).

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
            job.state = JobState::Cancelled;
            job.progress = None;
            job.finished_at = Some(now_rfc3339());

            save(&paths.queue, &snapshot)?;
            emitter.emit(&serde_json::json!({ "cancelled": [id.clone()] }), || {
                format!("Cancelled job {id}")
            });
            Ok(())
        }
        QueueCmd::CancelAll => {
            let mut snapshot = persist::load(&paths.queue);
            let finished_at = now_rfc3339();
            let mut cancelled = Vec::new();
            for job in &mut snapshot.jobs {
                if can_transition(job.state, JobState::Cancelled) {
                    job.state = JobState::Cancelled;
                    job.progress = None;
                    job.finished_at = Some(finished_at.clone());
                    cancelled.push(job.id.clone());
                }
            }
            save(&paths.queue, &snapshot)?;
            let count = cancelled.len();
            emitter.emit(&serde_json::json!({ "cancelled": cancelled }), || {
                format!("Cancelled {count} job(s)")
            });
            Ok(())
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
