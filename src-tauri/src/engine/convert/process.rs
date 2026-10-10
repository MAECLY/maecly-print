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

use crate::engine::{EngineError, Failure};

/// How often the cancel flag and the deadline are checked while a child runs.
const POLL: Duration = Duration::from_millis(100);

/// At most this many bytes of the child's stderr are quoted in an error.
const STDERR_TAIL: usize = 600;

/// Why [`run_bounded`] stopped a child early.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stopped {
    /// The job's cancel flag was raised (user cancel, pause, or the
    /// dispatcher's conversion timeout).
    Cancelled,
    /// The converter's own deadline passed.
    TimedOut,
    /// The OS could not tell us whether the child was still running, so it
    /// was killed rather than waited on blindly. Carries the OS error text:
    /// this is not a timeout and must not be reported as one.
    WaitFailed(String),
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
/// [`EngineError::ConversionTimedOut`] naming the limit (terminal: the queue
/// does not retry a converter that already ran out of time); a failed wait is
/// `ConversionFailed` carrying the OS error.
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
        EngineError::ConversionFailed(Failure::new(
            "workdir_failed",
            format!("cannot create a log for {label}: {e}"),
        ))
    })?;
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::from(stderr));
    isolate_process_tree(&mut cmd);

    let mut child = cmd.spawn().map_err(|e| spawn_error(label, &e))?;

    match wait_bounded(&mut child, cancel, timeout, finished) {
        Ok(Exit::Finished) => Ok(()),
        Ok(Exit::Status(status)) if status.success() => Ok(()),
        Ok(Exit::Status(status)) => Err(EngineError::ConversionFailed(
            Failure::new(
                "converter_failed",
                format!("{label} failed ({status}){}", stderr_suffix(stderr_log)),
            )
            .param("tool", label),
        )),
        Err(stopped) => Err(stopped_error(stopped, label, timeout)),
    }
}

/// The dependency id (`common:deps.<id>`) of a converter known by its
/// [`run_bounded`] label, so a converter that vanished between being located
/// and being started pauses its job with the same FR-058 remediation the
/// resolver would have given. Pure.
pub fn dependency_id_of(label: &str) -> Option<&'static str> {
    Some(match label {
        "LibreOffice" => "libre-office",
        "Pandoc" => "pandoc",
        "sips" => "sips",
        "The browser" => "chromium",
        _ => return None,
    })
}

/// The error a failed spawn becomes: a program that is not there is a
/// missing dependency (FR-058); one that is there but will not start is a
/// conversion failure naming it. Pure.
pub fn spawn_error(label: &str, e: &std::io::Error) -> EngineError {
    let detail = format!("{label} could not be started: {e}");
    if e.kind() == std::io::ErrorKind::NotFound {
        EngineError::DependencyMissing(match dependency_id_of(label) {
            Some(id) => Failure::new(id, detail),
            None => detail.into(),
        })
    } else {
        EngineError::ConversionFailed(
            Failure::new("converter_start_failed", detail).param("tool", label),
        )
    }
}

/// The error a converter that ran past `limit` becomes: the terminal
/// [`EngineError::ConversionTimedOut`], naming the tool and the limit. Shared
/// with the in-process decoders (`heif::run_abandonable`). Pure.
pub fn timed_out_error(label: &str, limit: Option<Duration>) -> EngineError {
    let secs = limit.map(|t| t.as_secs()).unwrap_or(0);
    EngineError::ConversionTimedOut(
        Failure::new(
            "tool_timed_out",
            format!("{label} did not finish within {secs}s and was stopped"),
        )
        .param("tool", label)
        .param("secs", secs),
    )
}

/// The error a child stopped early becomes. Pure.
pub fn stopped_error(stopped: Stopped, label: &str, timeout: Option<Duration>) -> EngineError {
    match stopped {
        Stopped::Cancelled => EngineError::Cancelled,
        Stopped::TimedOut => timed_out_error(label, timeout),
        Stopped::WaitFailed(e) => EngineError::ConversionFailed(
            Failure::new(
                "converter_crashed",
                format!("could not wait for {label} to finish ({e}); it was stopped"),
            )
            .param("tool", label),
        ),
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
            // The OS refused to tell us; end the child rather than wait
            // blindly, and say why — this is not a timeout.
            Err(e) => {
                kill_tree(child);
                return Err(Stopped::WaitFailed(e.to_string()));
            }
        }
    }
}

/// Put the child in its own process group (Unix) so [`kill_tree`] can take its
/// helpers with it. Windows needs nothing up front: `taskkill /T` walks the
/// tree by parent pid. The Windows console window is suppressed so a
/// conversion does not flash a terminal in the user's face.
pub(crate) fn isolate_process_tree(cmd: &mut Command) {
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
pub(crate) fn kill_tree(child: &mut Child) {
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

/// [`kill_tree`], then whether the whole tree is **confirmed** gone. `false`
/// when part of it could not be ended: a helper running elevated (`Access
/// denied` to `taskkill`, `Operation not permitted` to `kill`) or one still
/// alive a second later. The caller must then not claim the work stopped.
/// Work handed to a service outside the tree (the Windows Installer service
/// runs an MSI as SYSTEM) is invisible here; callers that start such work
/// know it and say so themselves.
pub(crate) fn kill_tree_confirmed(child: &mut Child) -> bool {
    let pid = child.id();
    #[cfg(unix)]
    {
        let _ = Command::new("kill")
            .args(["-KILL", &format!("-{pid}")])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        let _ = child.kill();
        let _ = child.wait();
        // The group is gone once signal 0 finds nobody (helpers killed a
        // moment ago may still be zombies waiting to be reaped).
        let deadline = Instant::now() + Duration::from_secs(1);
        loop {
            let probe = Command::new("kill")
                .args(["-0", &format!("-{pid}")])
                .env("LC_ALL", "C")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::piped())
                .output();
            match probe {
                Ok(out) if !out.status.success() => {
                    let err = String::from_utf8_lossy(&out.stderr).to_ascii_lowercase();
                    return !err.contains("not permitted");
                }
                Ok(_) => {}
                Err(_) => return false,
            }
            if Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let killed = Command::new("taskkill")
            .args(["/T", "/F", "/PID", &pid.to_string()])
            .creation_flags(0x0800_0000)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        let _ = child.kill();
        let _ = child.wait();
        matches!(killed, Ok(s) if s.success())
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = pid;
        let _ = child.kill();
        let _ = child.wait();
        false
    }
}

/// `": <last bytes of stderr>"`, or nothing when the log is empty/unreadable.
fn stderr_suffix(log: &Path) -> String {
    let tail = log_tail(log, STDERR_TAIL);
    if tail.is_empty() {
        String::new()
    } else {
        format!(": {tail}")
    }
}

/// Lines converters print on every run that say nothing about *this* failure:
/// Fontconfig's cache warnings (six of them on a stock LibreOffice) and GTK /
/// GLib / accessibility-bus chatter. Quoting them would bury the one line the
/// user needs ("Error: source file could not be loaded").
fn is_noise(line: &str) -> bool {
    let line = line.trim();
    line.is_empty()
        || line.starts_with("Fontconfig warning")
        || line.starts_with("Fontconfig error")
        || [
            "Gtk-WARNING",
            "Gdk-WARNING",
            "GLib-GIO-WARNING",
            "dbind-WARNING",
        ]
        .iter()
        .any(|tag| line.contains(tag))
}

/// The last `max` bytes of a converter's log with the known noise removed
/// ([`is_noise`]), lines joined by a space. Empty when the log is missing or
/// holds nothing but noise.
pub(crate) fn log_tail(log: &Path, max: usize) -> String {
    let Ok(bytes) = std::fs::read(log) else {
        return String::new();
    };
    let text = String::from_utf8_lossy(&bytes);
    let text = text
        .lines()
        .filter(|l| !is_noise(l))
        .map(str::trim)
        .collect::<Vec<_>>()
        .join(" ");
    if text.len() > max {
        // Cut on a char boundary at or after the byte offset.
        let mut start = text.len() - max;
        while !text.is_char_boundary(start) {
            start += 1;
        }
        text[start..].to_string()
    } else {
        text
    }
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

    /// A converter's failure message quotes what it said, minus the noise it
    /// prints on every run: a corrupt DOCX used to report six Fontconfig
    /// warnings ahead of LibreOffice's one meaningful line.
    #[test]
    fn fr_053_failure_detail_drops_converter_noise() {
        let dir = tempfile::tempdir().unwrap();
        let cancel = AtomicBool::new(false);
        let script = "for i in 1 2 3 4 5 6; do \
              echo 'Fontconfig warning: no <cachedir> elements found. Check configuration.' >&2; \
            done; \
            echo '(soffice:1): Gtk-WARNING **: cannot open display' >&2; \
            echo 'Error: source file could not be loaded' >&2; exit 1";
        let err = run_bounded(
            sh(script),
            "LibreOffice",
            &cancel,
            None,
            &dir.path().join("e"),
        )
        .unwrap_err();
        let EngineError::ConversionFailed(m) = err else {
            panic!("unexpected {err:?}");
        };
        assert!(
            m.ends_with(": Error: source file could not be loaded"),
            "{m}"
        );
        assert!(!m.contains("Fontconfig"), "{m}");
        assert!(!m.contains("Gtk-WARNING"), "{m}");
    }

    /// An OS error while waiting is not a timeout and must not read like one
    /// ("did not finish within 0s").
    #[test]
    fn a_failed_wait_is_reported_with_its_cause_not_as_a_timeout() {
        let err = stopped_error(
            Stopped::WaitFailed("Interrupted system call".to_string()),
            "Pandoc",
            None,
        );
        match err {
            EngineError::ConversionFailed(m) => {
                assert!(m.contains("Interrupted system call"), "{m}");
                assert!(!m.contains("did not finish"), "{m}");
            }
            other => panic!("unexpected {other:?}"),
        }
        assert!(matches!(
            stopped_error(Stopped::TimedOut, "Pandoc", Some(Duration::from_secs(9))),
            EngineError::ConversionTimedOut(m) if m.contains("9s")
        ));
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
        // A timeout is its own (terminal) error, so the queue does not retry
        // a converter that already ran out of time (FR-034).
        assert!(
            matches!(&err, EngineError::ConversionTimedOut(m) if m.contains("did not finish")),
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

#[cfg(test)]
mod reason_tests {
    use super::*;

    /// FR-092: every way a converter process fails carries a registered
    /// reason and the params its sentence needs, so the queue says it in the
    /// user's language instead of quoting the English detail.
    #[test]
    fn fr_092_engine_failures_carry_reason_for_converter_processes() {
        let gone = std::io::Error::from(std::io::ErrorKind::NotFound);
        match spawn_error("LibreOffice", &gone) {
            EngineError::DependencyMissing(f) => {
                assert_eq!(f.reason, Some("libre-office"));
                assert!(f.contains("LibreOffice could not be started"), "{f}");
            }
            other => panic!("unexpected {other:?}"),
        }
        // An unknown label still pauses as a missing dependency, reasonless.
        assert!(matches!(
            spawn_error("Ghost", &gone),
            EngineError::DependencyMissing(f) if f.reason.is_none()
        ));
        let denied = std::io::Error::from(std::io::ErrorKind::PermissionDenied);
        match spawn_error("Pandoc", &denied) {
            EngineError::ConversionFailed(f) => {
                assert_eq!(f.reason, Some("converter_start_failed"));
                assert_eq!(f.params["tool"], "Pandoc");
            }
            other => panic!("unexpected {other:?}"),
        }
        match stopped_error(Stopped::TimedOut, "sips", Some(Duration::from_secs(9))) {
            EngineError::ConversionTimedOut(f) => {
                assert_eq!(f.reason, Some("tool_timed_out"));
                assert_eq!(f.params["tool"], "sips");
                assert_eq!(f.params["secs"], "9");
            }
            other => panic!("unexpected {other:?}"),
        }
        match stopped_error(Stopped::WaitFailed("EINTR".into()), "sips", None) {
            EngineError::ConversionFailed(f) => {
                assert_eq!(f.reason, Some("converter_crashed"));
                assert!(f.contains("EINTR"));
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    /// Every label a converter is run under maps to a registered dependency.
    #[test]
    fn fr_058_known_converter_labels_map_to_registered_dependency_ids() {
        for label in ["LibreOffice", "Pandoc", "sips", "The browser"] {
            let id = dependency_id_of(label).expect(label);
            assert!(
                crate::util::reasons::is_job_reason("dependency_missing", id),
                "{id}"
            );
        }
    }
}
