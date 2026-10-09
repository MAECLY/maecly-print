//! Bounded execution of an external converter (LibreOffice, Chromium, Pandoc,
//! `sips`).
//!
//! Every converter that shells out goes through [`run_bounded`], which is where
//! the three promises of `docs/03-requirements/supported-formats.md` live:
//!
//! - **No shell.** The program and its arguments are a [`Command`] argument
//!   vector; nothing is ever interpolated into a shell string, so a file called
//!   `$(rm -rf ~).docx` is just an odd file name.
//! - **Bounded.** The child is polled against a deadline *and* the job's cancel
//!   flag. The dispatcher's `conversion_timeout` sets that flag, so a hung
//!   `soffice` is killed rather than left to hold a conversion slot forever.
//! - **Killed for real.** Office suites and browsers fork helpers
//!   (`soffice.bin`, renderer processes). On Unix the child leads its own
//!   process group and the whole group is killed; on Windows `taskkill /T`
//!   takes the tree. `Child::kill` is always issued as well, as the floor.
//!
//! stderr goes to a file in the work directory rather than a pipe: a chatty
//! child can never fill a pipe buffer and deadlock against our polling loop,
//! and the tail of it becomes the user-visible failure detail.

use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use wait_timeout::ChildExt;

use crate::engine::EngineError;

/// How often the cancel flag and the deadline are checked while a child runs.
const POLL: Duration = Duration::from_millis(100);

/// At most this many bytes of the child's stderr are quoted in an error.
const STDERR_TAIL: usize = 600;

/// Why [`run_bounded`] stopped a child early.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stopped {
    /// The job's cancel flag was raised (user cancel, pause, or the
    /// dispatcher's conversion timeout).
    Cancelled,
    /// The converter's own deadline passed.
    TimedOut,
}

/// Run `cmd` to completion, killing it when `cancel` is raised or `timeout`
/// elapses.
///
/// `label` names the program in error messages ("LibreOffice"). `stderr_log`
/// is where the child's stderr is captured; it should live inside the job's
/// work directory so it disappears with it.
///
/// Returns `Ok(())` only for a zero exit status. A non-zero exit is
/// [`EngineError::ConversionFailed`] carrying the tail of stderr; a spawn
/// failure of a program we already located is the same (a broken install, not
/// a missing one); a cancel is [`EngineError::Cancelled`]; a timeout is
/// `ConversionFailed` naming the limit.
pub fn run_bounded(
    cmd: Command,
    label: &str,
    cancel: &AtomicBool,
    timeout: Option<Duration>,
    stderr_log: &Path,
) -> Result<(), EngineError> {
    run_until(cmd, label, cancel, timeout, stderr_log, None)
}

/// [`run_bounded`], plus a `finished` predicate checked on every poll: once it
/// answers `true` the child (and its helpers) are killed and the run counts as
/// a success.
///
/// This exists for headless Chromium, which on some versions writes the
/// `--print-to-pdf` file in about a second and then never exits (seen with
/// Chrome 156 on macOS). Waiting for the exit would turn every HTML job into a
/// conversion timeout; waiting for the finished output does not.
pub fn run_until(
    mut cmd: Command,
    label: &str,
    cancel: &AtomicBool,
    timeout: Option<Duration>,
    stderr_log: &Path,
    finished: Option<&dyn Fn() -> bool>,
) -> Result<(), EngineError> {
    let stderr = std::fs::File::create(stderr_log).map_err(|e| {
        EngineError::ConversionFailed(format!("cannot create a log for {label}: {e}"))
    })?;
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::from(stderr));
    isolate_process_tree(&mut cmd);

    let mut child = cmd.spawn().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            EngineError::DependencyMissing(format!("{label} could not be started: {e}"))
        } else {
            EngineError::ConversionFailed(format!("{label} could not be started: {e}"))
        }
    })?;

    match wait_bounded(&mut child, cancel, timeout, finished) {
        Ok(Exit::Finished) => Ok(()),
        Ok(Exit::Status(status)) if status.success() => Ok(()),
        Ok(Exit::Status(status)) => Err(EngineError::ConversionFailed(format!(
            "{label} failed ({status}){}",
            stderr_suffix(stderr_log)
        ))),
        Err(Stopped::Cancelled) => Err(EngineError::Cancelled),
        Err(Stopped::TimedOut) => Err(EngineError::ConversionFailed(format!(
            "{label} did not finish within {}s and was stopped",
            timeout.map(|t| t.as_secs()).unwrap_or(0)
        ))),
    }
}

/// How a child that was not stopped early ended.
#[derive(Debug)]
pub enum Exit {
    /// It exited on its own with this status.
    Status(std::process::ExitStatus),
    /// The `finished` predicate fired; the child was killed after that.
    Finished,
}

/// Poll `child` until it exits (or `finished` says its work is done), or kill
/// it (tree and all) when `cancel` is set or `timeout` elapses. Separate from
/// [`run_bounded`] so the kill policy is testable with any child.
pub fn wait_bounded(
    child: &mut Child,
    cancel: &AtomicBool,
    timeout: Option<Duration>,
    finished: Option<&dyn Fn() -> bool>,
) -> Result<Exit, Stopped> {
    let started = Instant::now();
    loop {
        if cancel.load(Ordering::Relaxed) {
            kill_tree(child);
            return Err(Stopped::Cancelled);
        }
        if finished.is_some_and(|done| done()) {
            kill_tree(child);
            return Ok(Exit::Finished);
        }
        if let Some(limit) = timeout {
            if started.elapsed() >= limit {
                kill_tree(child);
                return Err(Stopped::TimedOut);
            }
        }
        match child.wait_timeout(POLL) {
            Ok(Some(status)) => return Ok(Exit::Status(status)),
            Ok(None) => continue,
            // The OS refused to tell us; treat it like a hang we must end.
            Err(_) => {
                kill_tree(child);
                return Err(Stopped::TimedOut);
            }
        }
    }
}

/// Put the child in its own process group (Unix) so [`kill_tree`] can take its
/// helpers with it. Windows needs nothing up front: `taskkill /T` walks the
/// tree by parent pid. The Windows console window is suppressed so a
/// conversion does not flash a terminal in the user's face.
fn isolate_process_tree(cmd: &mut Command) {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
}

/// Kill `child` and everything it spawned, then reap it. Best effort: each
/// step ignores failures because the child may already be gone.
fn kill_tree(child: &mut Child) {
    let pid = child.id();
    #[cfg(unix)]
    {
        // `kill -KILL -<pgid>` signals the whole group the child leads. Run
        // through the `kill` utility so no `libc` binding is needed.
        let _ = Command::new("kill")
            .args(["-KILL", &format!("-{pid}")])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let _ = Command::new("taskkill")
            .args(["/T", "/F", "/PID", &pid.to_string()])
            .creation_flags(0x0800_0000)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    #[cfg(not(any(unix, windows)))]
    let _ = pid;
    let _ = child.kill();
    let _ = child.wait();
}

/// `": <last bytes of stderr>"`, or nothing when the log is empty/unreadable.
fn stderr_suffix(log: &Path) -> String {
    let Ok(bytes) = std::fs::read(log) else {
        return String::new();
    };
    let text = String::from_utf8_lossy(&bytes);
    let text = text.trim();
    if text.is_empty() {
        return String::new();
    }
    let tail = if text.len() > STDERR_TAIL {
        // Cut on a char boundary at or after the byte offset.
        let mut start = text.len() - STDERR_TAIL;
        while !text.is_char_boundary(start) {
            start += 1;
        }
        &text[start..]
    } else {
        text
    };
    format!(": {tail}")
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn sh(script: &str) -> Command {
        let mut c = Command::new("/bin/sh");
        c.args(["-c", script]);
        c
    }

    #[test]
    fn zero_exit_is_ok() {
        let dir = tempfile::tempdir().unwrap();
        let cancel = AtomicBool::new(false);
        run_bounded(sh("exit 0"), "t", &cancel, None, &dir.path().join("e")).unwrap();
    }

    #[test]
    fn non_zero_exit_quotes_stderr() {
        let dir = tempfile::tempdir().unwrap();
        let cancel = AtomicBool::new(false);
        let err = run_bounded(
            sh("echo boom-detail >&2; exit 3"),
            "Tool",
            &cancel,
            None,
            &dir.path().join("e"),
        )
        .unwrap_err();
        match err {
            EngineError::ConversionFailed(m) => {
                assert!(m.contains("Tool failed"), "{m}");
                assert!(m.contains("boom-detail"), "{m}");
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn timeout_kills_the_child_and_its_helpers() {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("survived");
        // The background `sleep` is a helper in the same process group; if the
        // group kill missed it, it would create the marker after 2 s.
        let script = format!("(sleep 2; touch '{}') & sleep 30", marker.display());
        let cancel = AtomicBool::new(false);
        let started = Instant::now();
        let err = run_bounded(
            sh(&script),
            "Slow",
            &cancel,
            Some(Duration::from_millis(300)),
            &dir.path().join("e"),
        )
        .unwrap_err();
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "must not wait 30 s"
        );
        assert!(
            matches!(&err, EngineError::ConversionFailed(m) if m.contains("did not finish")),
            "{err:?}"
        );
        std::thread::sleep(Duration::from_millis(2500));
        assert!(!marker.exists(), "a helper outlived the kill");
    }

    #[test]
    fn cancel_flag_kills_the_child() {
        let dir = tempfile::tempdir().unwrap();
        let cancel = std::sync::Arc::new(AtomicBool::new(false));
        let flag = cancel.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(200));
            flag.store(true, Ordering::Relaxed);
        });
        let started = Instant::now();
        let err =
            run_bounded(sh("sleep 30"), "t", &cancel, None, &dir.path().join("e")).unwrap_err();
        assert!(matches!(err, EngineError::Cancelled));
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn fr_055_a_child_that_never_exits_is_done_once_its_output_is_finished() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("out");
        // Writes its output, then lingers like headless Chrome does.
        let script = format!("echo done > '{}'; sleep 30", out.display());
        let cancel = AtomicBool::new(false);
        let started = Instant::now();
        let done = || out.is_file();
        run_until(
            sh(&script),
            "Lingering",
            &cancel,
            Some(Duration::from_secs(20)),
            &dir.path().join("e"),
            Some(&done),
        )
        .unwrap();
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn a_program_that_does_not_exist_is_a_missing_dependency() {
        let dir = tempfile::tempdir().unwrap();
        let cancel = AtomicBool::new(false);
        let err = run_bounded(
            Command::new(dir.path().join("no-such-binary")),
            "Ghost",
            &cancel,
            None,
            &dir.path().join("e"),
        )
        .unwrap_err();
        assert!(matches!(err, EngineError::DependencyMissing(_)), "{err:?}");
    }

    #[test]
    fn arguments_are_never_shell_interpolated() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("argv");
        let hostile = "$(touch pwned); `touch pwned2`";
        let mut cmd = Command::new("/bin/sh");
        // $1 is written verbatim; the hostile text is an argument, not code.
        cmd.current_dir(dir.path()).args([
            "-c",
            "printf '%s' \"$1\" > \"$2\"",
            "sh",
            hostile,
            out.to_str().unwrap(),
        ]);
        let cancel = AtomicBool::new(false);
        run_bounded(cmd, "t", &cancel, None, &dir.path().join("e")).unwrap();
        assert_eq!(std::fs::read_to_string(&out).unwrap(), hostile);
        assert!(!dir.path().join("pwned").exists());
        assert!(!dir.path().join("pwned2").exists());
    }
}
