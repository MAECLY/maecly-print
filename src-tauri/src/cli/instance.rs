//! "Is the app open right now?" — answered by an OS file lock, not a guess.
//!
//! The GUI owns the live queue in memory and rewrites `queue.json` on every
//! transition. A headless `pluryprint queue cancel` that edited the file
//! behind its back would report success while the job kept printing, and its
//! write of a stale snapshot could drop jobs the GUI had persisted in between
//! (NFR-010). So the GUI holds an exclusive lock on `instance.lock` (next to
//! `queue.json`) for as long as it runs, and the CLI's mutating queue commands
//! check it first and refuse honestly instead of pretending.
//!
//! A lock rather than a PID file because the OS releases it when the process
//! dies, however it dies: a crashed app can never leave the CLI locked out,
//! and there is no PID-reuse race to get wrong.

use std::fs::{File, OpenOptions, TryLockError};
use std::io;
use std::path::{Path, PathBuf};

/// File name of the lock, in the same directory as `queue.json`.
pub const LOCK_FILE: &str = "instance.lock";

/// The lock next to a given `queue.json`, so the CLI (which is handed the
/// queue path) and the GUI (which uses the data dir) agree on one file.
pub fn lock_path_for(queue_json: &Path) -> PathBuf {
    queue_json.with_file_name(LOCK_FILE)
}

/// Proof that this process is the running app. Hold it for the process'
/// lifetime; dropping it (or the process ending) releases the lock.
#[derive(Debug)]
pub struct InstanceLock {
    _file: File,
}

fn open(path: &Path) -> io::Result<File> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path)
}

/// How many times [`claim`] tries before concluding another process owns the
/// lock, and how long it waits between tries.
///
/// The CLI's probe ([`state`]) takes the same exclusive lock for an instant.
/// An app starting inside that instant would otherwise run its whole lifetime
/// without the lock, and every later `queue cancel` would edit `queue.json`
/// behind its back. A probe holds it for microseconds; a real owner holds it
/// for good, so a short retry tells the two apart.
const CLAIM_ATTEMPTS: u32 = 20;
const CLAIM_RETRY: std::time::Duration = std::time::Duration::from_millis(25);

/// Take the lock for the running app. `Ok(None)` when another process holds
/// it for longer than a probe would — which the single-instance plugin should
/// make impossible, so the caller logs it rather than refusing to start.
pub fn claim(path: &Path) -> io::Result<Option<InstanceLock>> {
    let file = open(path)?;
    for attempt in 1..=CLAIM_ATTEMPTS {
        match file.try_lock() {
            Ok(()) => return Ok(Some(InstanceLock { _file: file })),
            Err(TryLockError::WouldBlock) if attempt < CLAIM_ATTEMPTS => {
                std::thread::sleep(CLAIM_RETRY);
            }
            Err(TryLockError::WouldBlock) => return Ok(None),
            Err(TryLockError::Error(e)) => return Err(e),
        }
    }
    Ok(None)
}

/// What the lock says about a running app.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LockState {
    /// A running app holds the lock.
    Held,
    /// Nobody holds it: no app is running.
    Free,
    /// The lock could not be checked (the file cannot be opened, or this
    /// filesystem does not support locking — some network or redirected
    /// profile folders). An app may well be running unlocked there, so this
    /// is never read as "not running". Carries the reason.
    Unknown(String),
}

/// Whether a running app currently holds the lock.
pub fn state(path: &Path) -> LockState {
    let file = match open(path) {
        Ok(file) => file,
        Err(e) => return LockState::Unknown(e.to_string()),
    };
    match file.try_lock() {
        // We got it, so nobody else had it; dropping `file` releases it.
        Ok(()) => LockState::Free,
        Err(TryLockError::WouldBlock) => LockState::Held,
        Err(TryLockError::Error(e)) => LockState::Unknown(e.to_string()),
    }
}

/// `true` only when a running app is known to hold the lock. Callers that
/// are about to edit the app's files must use [`state`] and treat
/// [`LockState::Unknown`] as a refusal too.
pub fn is_held(path: &Path) -> bool {
    state(path) == LockState::Held
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fr_009_a_held_lock_is_seen_and_released_on_drop() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(LOCK_FILE);

        assert!(!is_held(&path), "nobody holds it yet");
        let lock = claim(&path).unwrap().expect("first claim wins");
        assert!(is_held(&path), "the running app is visible to the CLI");
        assert!(
            claim(&path).unwrap().is_none(),
            "a second claimant is told so"
        );

        drop(lock);
        assert!(!is_held(&path), "the lock dies with its holder");
    }

    /// A probe that takes the lock for an instant must not make a starting
    /// app run without it: `claim` waits it out.
    #[test]
    fn nfr_010_a_probe_in_flight_does_not_cost_the_app_its_lock() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(LOCK_FILE);
        let probe = open(&path).unwrap();
        probe.try_lock().unwrap();
        let releaser = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(60));
            drop(probe);
        });

        let lock = claim(&path).unwrap();
        releaser.join().unwrap();
        assert!(
            lock.is_some(),
            "the app gets the lock once the probe lets go"
        );
        assert_eq!(state(&path), LockState::Held);
    }

    /// A lock that cannot be checked is not evidence the app is closed.
    #[test]
    fn nfr_010_an_unopenable_lock_is_unknown_not_free() {
        let dir = tempfile::tempdir().unwrap();
        // A directory where the lock file should be cannot be opened as one.
        let path = dir.path().join(LOCK_FILE);
        std::fs::create_dir(&path).unwrap();
        assert!(matches!(state(&path), LockState::Unknown(_)));
        assert!(!is_held(&path));
    }

    #[test]
    fn lock_lives_next_to_queue_json() {
        assert_eq!(
            lock_path_for(Path::new("/data/Pluryprint/queue.json")),
            Path::new("/data/Pluryprint/instance.lock")
        );
    }
}
