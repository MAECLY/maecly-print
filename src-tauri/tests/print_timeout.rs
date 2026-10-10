//! The print stage's bounded wait, against a real child process.
//!
//! The dispatcher's own tests fake the engine seam, so they pin the *policy* (a
//! timed-out job fails terminally and the queue keeps going). This suite pins
//! the other half — that the wait really is bounded and that the child really
//! dies — without touching a printer: it spawns a command that does nothing but
//! sleep, which is exactly the shape of the failure being guarded against (a
//! wedged `lp` / SumatraPDF).

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use pluryprint_lib::engine::print::{wait_with_timeout, PrintError};

/// A child that sleeps far longer than any timeout used here, with its pipes
/// silenced so the test output stays clean.
fn sleeper() -> std::process::Child {
    let mut cmd = if cfg!(windows) {
        // `timeout` needs a console; `ping` to the loopback is the portable
        // "sleep" on Windows and needs none.
        let mut c = Command::new("cmd");
        c.args(["/C", "ping", "-n", "60", "127.0.0.1"]);
        c
    } else {
        let mut c = Command::new("sleep");
        c.arg("60");
        c
    };
    cmd.stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawning the sleeping stand-in must work")
}

#[test]
fn fr_034_print_timeout_kills_a_hung_child_within_its_budget() {
    let mut child = sleeper();
    let pid = child.id();

    let started = Instant::now();
    let err = wait_with_timeout(&mut child, Some(Duration::from_millis(250)))
        .expect_err("a 60s child must not beat a 250ms budget");
    let elapsed = started.elapsed();

    match err {
        PrintError::TimedOut { seconds } => assert_eq!(seconds, 0, "sub-second budget"),
        other => panic!("expected a timeout, got {other:?}"),
    }

    // NFR-012: the wait is bounded and the orphan is killed, not leaked. The
    // ceiling is generous (a loaded CI box is still nowhere near 5 s).
    assert!(
        elapsed < Duration::from_secs(5),
        "the wait must be bounded, took {elapsed:?}"
    );
    // The child has already been reaped by `wait_with_timeout`, so a second
    // wait returns immediately instead of blocking for the remaining 60s.
    let reaped = Instant::now();
    let _ = child.try_wait();
    assert!(reaped.elapsed() < Duration::from_secs(1));
    assert!(!is_alive(pid), "the killed child must not linger");
}

#[test]
fn fr_034_a_child_that_finishes_in_time_reports_its_status() {
    let mut child = Command::new(if cfg!(windows) { "cmd" } else { "true" })
        .args(if cfg!(windows) {
            vec!["/C", "exit", "0"]
        } else {
            vec![]
        })
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawning a trivial child must work");

    let status = wait_with_timeout(&mut child, Some(Duration::from_secs(30)))
        .expect("a child that exits immediately is not a timeout");
    assert!(status.success());
}

#[test]
fn fr_034_no_budget_waits_for_the_child() {
    // `None` disables the bound (the engine timeout set to 0), matching the
    // pre-timeout behaviour for users who opt out.
    let mut child = Command::new(if cfg!(windows) { "cmd" } else { "true" })
        .args(if cfg!(windows) {
            vec!["/C", "exit", "0"]
        } else {
            vec![]
        })
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawning a trivial child must work");

    assert!(wait_with_timeout(&mut child, None)
        .expect("an unbounded wait on a finished child succeeds")
        .success());
}

/// Whether `pid` still names a live process.
///
/// Implemented on both supported platforms on purpose: a version that answers a
/// constant `false` off Unix would make the leak assertion above vacuously true
/// on Windows, which is the *target* platform — the one where a leaked
/// SumatraPDF is a real support call.
#[cfg(unix)]
fn is_alive(pid: u32) -> bool {
    // `kill -0` probes without signalling. A killed-and-reaped child is gone,
    // so this is false; a leaked one would still answer.
    Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[cfg(windows)]
fn is_alive(pid: u32) -> bool {
    // `tasklist` filtered by pid prints one row per match; `/NH` drops the
    // header, so the pid can only appear in the output when the process is
    // really there. With no match it prints the "INFO: No tasks…" banner (on
    // stdout) and still exits 0, which is why the *text* is what decides.
    let out = Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/NH"])
        .stderr(Stdio::null())
        .output()
        .expect("tasklist ships with every supported Windows version");
    String::from_utf8_lossy(&out.stdout).contains(&pid.to_string())
}
