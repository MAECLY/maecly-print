//! "Is the app open right now?" — answered by an OS file lock, not a guess.
//!
//! The GUI owns the live queue in memory and rewrites `queue.json` on every
//! transition. A headless `maeclyprint queue cancel` that edited the file
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

/// Take the lock for the running app. `Ok(None)` when another process already
/// holds it — which the single-instance plugin should make impossible, so the
/// caller logs it rather than refusing to start.
pub fn claim(path: &Path) -> io::Result<Option<InstanceLock>> {
    let file = open(path)?;
    match file.try_lock() {
        Ok(()) => Ok(Some(InstanceLock { _file: file })),
        Err(TryLockError::WouldBlock) => Ok(None),
        Err(TryLockError::Error(e)) => Err(e),
    }
}

/// Whether a running app currently holds the lock.
///
/// Errs on the side of "not running" only when the lock file cannot even be
/// opened (no data dir yet means no app has ever run here); any lock state we
/// can read is reported as it is.
pub fn is_held(path: &Path) -> bool {
    let Ok(file) = open(path) else {
        return false;
    };
    match file.try_lock() {
        // We got it, so nobody else had it; dropping `file` releases it.
        Ok(()) => false,
        Err(TryLockError::WouldBlock) => true,
        Err(TryLockError::Error(_)) => false,
    }
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

    #[test]
    fn lock_lives_next_to_queue_json() {
        assert_eq!(
            lock_path_for(Path::new("/data/MaeclyPrint/queue.json")),
            Path::new("/data/MaeclyPrint/instance.lock")
        );
    }
}
