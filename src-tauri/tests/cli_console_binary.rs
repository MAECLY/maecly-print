//! FR-009 — the console front end (`pluryprint-cli`) returns the documented
//! exit codes to whoever started it.
//!
//! On Windows this is the binary a script runs, because shells do not wait for
//! the GUI-subsystem `pluryprint.exe`; the contract is that it runs the same
//! CLI in-process and its exit status *is* the CLI's exit code. Only argv-level
//! paths are exercised here (help, version, a typo), so the tests never touch
//! the user's configuration, printers or queue.

use std::process::Command;

fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_pluryprint-cli"))
}

#[test]
fn fr_009_console_binary_reports_success() {
    let out = cli().arg("--version").output().expect("run pluryprint-cli");
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains(env!("CARGO_PKG_VERSION")), "{stdout}");
}

#[test]
fn fr_009_console_binary_reports_usage_errors_as_2() {
    // A typo is routed to the CLI (never to a window) and is invalid usage.
    let out = cli()
        .arg("frobnicate")
        .output()
        .expect("run pluryprint-cli");
    assert_eq!(out.status.code(), Some(2), "{out:?}");
    assert!(
        !out.stderr.is_empty(),
        "the usage error is explained on stderr"
    );
}
