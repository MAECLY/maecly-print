//! Writing the handoff file and starting the app.
//!
//! This is the whole "producer" side of the contract in
//! `src-tauri/src/cli/handoff.rs`: write the selection into the handoff
//! directory, then run
//!
//! ```text
//! maeclyprint.exe --invoke=context-menu --files-from <path> [--action A] [--profile P]
//! ```
//!
//! `--action` and `--profile` are passed as well as being inside the file. The
//! file wins where it has an opinion, but a payload the app decides to ignore
//! still produces the right behaviour from argv, and the argv form is what a
//! human sees in Process Explorer when debugging.
//!
//! The command line's only variable part is one path, so its length is bounded
//! by `MAX_PATH` no matter how large the selection is — which is the entire
//! reason FR-003 is satisfiable.
//!
//! A second launch while the app is already running is not a problem here: the
//! app registers `tauri-plugin-single-instance`, so the running process
//! receives this argv and merges the selection.

use std::path::{Path, PathBuf};

use windows::core::{w, HSTRING, PCWSTR};
use windows::Win32::UI::Shell::{
    ShellExecuteExW, SEE_MASK_FLAG_NO_UI, SEE_MASK_NOASYNC, SHELLEXECUTEINFOW,
};
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

use crate::handoff::{self, Action};

/// Write the handoff file for `paths` and launch the app against it.
///
/// Returns the handoff path so the caller can log it. An empty selection is
/// refused before anything is written: the app rejects a payload with no paths,
/// and launching a window for nothing is worse than doing nothing.
pub fn deliver(
    paths: &[PathBuf],
    action: Action,
    profile_id: Option<String>,
) -> std::io::Result<PathBuf> {
    if paths.is_empty() {
        return Err(std::io::Error::other(
            "the selection contained no filesystem paths",
        ));
    }

    let exe = crate::module::app_exe().ok_or_else(|| {
        std::io::Error::other("could not locate maeclyprint.exe next to the shell extension")
    })?;

    let handoff_path = handoff::write(paths, action, profile_id.clone())?;

    match launch(&exe, &handoff_path, action, profile_id.as_deref()) {
        Ok(()) => Ok(handoff_path),
        Err(err) => {
            // The app will never read it, and the app's own orphan sweep only
            // runs on startup — which is exactly what just failed to happen.
            let _ = std::fs::remove_file(&handoff_path);
            Err(std::io::Error::other(err.to_string()))
        }
    }
}

fn launch(
    exe: &Path,
    handoff_path: &Path,
    action: Action,
    profile_id: Option<&str>,
) -> windows_core::Result<()> {
    let mut args = format!(
        "--invoke=context-menu --files-from {} --action {}",
        quote_arg(&handoff_path.display().to_string()),
        action.as_str()
    );
    if let Some(profile) = profile_id {
        // A profile id is user-controlled text — it comes out of a JSON file on
        // the user's disk — so it is quoted by the rules the receiving process
        // actually parses with, not by hand.
        args.push_str(&format!(" --profile {}", quote_arg(profile)));
    }

    let exe = HSTRING::from(exe.as_os_str());
    let args = HSTRING::from(args);
    let working_dir = HSTRING::from(
        exe.to_string()
            .rsplit_once('\\')
            .map(|(dir, _)| dir.to_string())
            .unwrap_or_default(),
    );

    let mut info = SHELLEXECUTEINFOW {
        cbSize: core::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        // NOASYNC: this DLL can be unloaded the moment `Invoke` returns, and an
        // asynchronous ShellExecuteEx would still be reading our memory.
        // NO_UI: Explorer must never end up owning an error dialog we raised.
        fMask: SEE_MASK_NOASYNC | SEE_MASK_FLAG_NO_UI,
        lpVerb: w!("open"),
        lpFile: PCWSTR(exe.as_ptr()),
        lpParameters: PCWSTR(args.as_ptr()),
        lpDirectory: PCWSTR(working_dir.as_ptr()),
        nShow: SW_SHOWNORMAL.0,
        ..Default::default()
    };

    unsafe { ShellExecuteExW(&mut info) }
}

/// One command-line argument, quoted so that `CommandLineToArgvW` — which is
/// what the Rust runtime on the other end parses argv with — hands the callee
/// back exactly `arg`.
///
/// The rule that matters, and that the previous "double the quotes" escaping
/// got wrong: inside a quoted argument, a run of backslashes is only special
/// when a `"` follows it, and then the whole run is halved and the quote is
/// consumed. So every backslash immediately before a `"` must be doubled — and
/// that includes the backslashes at the *end* of the value, because the closing
/// quote we append is a `"` that follows them. Without it a profile id ending in
/// `\` (`C:\reports\`) closed its own quote and everything after it — including
/// text the id itself carried — became further arguments.
///
/// Empty values are quoted as `""`; a value with nothing to escape is passed
/// bare, which keeps the common command line readable in Process Explorer.
///
/// Reference: "Parsing C++ Command-Line Arguments" / `CommandLineToArgvW`.
fn quote_arg(arg: &str) -> String {
    if !arg.is_empty() && !arg.contains([' ', '\t', '"', '\\']) {
        return arg.to_string();
    }

    let mut out = String::with_capacity(arg.len() + 2);
    out.push('"');
    let mut backslashes = 0usize;
    for ch in arg.chars() {
        match ch {
            '\\' => {
                backslashes += 1;
            }
            '"' => {
                // The run before a quote is halved by the parser, so double it,
                // then escape the quote itself with one more backslash.
                out.extend(std::iter::repeat('\\').take(backslashes * 2 + 1));
                backslashes = 0;
                out.push('"');
            }
            other => {
                out.extend(std::iter::repeat('\\').take(backslashes));
                backslashes = 0;
                out.push(other);
            }
        }
    }
    // The closing quote is itself a `"` preceded by that run.
    out.extend(std::iter::repeat('\\').take(backslashes * 2));
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::quote_arg;

    /// `CommandLineToArgvW`'s rules, in Rust, so a quoting assertion can be
    /// stated as "the callee gets back exactly what we meant" instead of as a
    /// literal string nobody can verify by eye. Parses a whole command line
    /// (the non-argv[0] rules, which is what our arguments are).
    fn argv_of(command_line: &str) -> Vec<String> {
        let mut args = Vec::new();
        let mut current = String::new();
        let mut in_quotes = false;
        let mut started = false;
        let mut backslashes = 0usize;

        // Flush a pending backslash run as literal backslashes.
        fn flush(current: &mut String, backslashes: &mut usize) {
            current.extend(std::iter::repeat('\\').take(*backslashes));
            *backslashes = 0;
        }

        for ch in command_line.chars() {
            match ch {
                '\\' => {
                    backslashes += 1;
                    started = true;
                }
                '"' => {
                    current.extend(std::iter::repeat('\\').take(backslashes / 2));
                    if backslashes % 2 == 1 {
                        current.push('"');
                    } else {
                        in_quotes = !in_quotes;
                    }
                    backslashes = 0;
                    started = true;
                }
                c if (c == ' ' || c == '\t') && !in_quotes => {
                    flush(&mut current, &mut backslashes);
                    if started {
                        args.push(std::mem::take(&mut current));
                        started = false;
                    }
                }
                c => {
                    flush(&mut current, &mut backslashes);
                    current.push(c);
                    started = true;
                }
            }
        }
        flush(&mut current, &mut backslashes);
        if started {
            args.push(current);
        }
        args
    }

    /// Every value must survive quote → parse unchanged, and produce exactly
    /// one argument.
    fn round_trips(value: &str) {
        let parsed = argv_of(&quote_arg(value));
        assert_eq!(
            parsed,
            vec![value.to_string()],
            "quoting {value:?} produced {:?}",
            quote_arg(value)
        );
    }

    #[test]
    fn fr_002_plain_values_round_trip() {
        for value in [
            "builtin-quick",
            "Duplex BW",
            "  leading and trailing  ",
            "",
            "\t",
            "acentuación 日本語",
        ] {
            round_trips(value);
        }
    }

    #[test]
    fn fr_002_a_trailing_backslash_cannot_close_the_quote() {
        // The defect: `profile\` became `"profile\"`, whose closing quote the
        // parser swallowed — everything the id carried after it was read as
        // more arguments on our command line.
        round_trips(r"profile\");
        round_trips(r"profile\\");
        round_trips(r"C:\Users\ana\Reports\");
        round_trips(r"\");
        round_trips(r"\\server\share\");

        let injected = r"evil\";
        let line = format!("--profile {} --action queue", quote_arg(injected));
        assert_eq!(
            argv_of(&line),
            vec!["--profile", r"evil\", "--action", "queue"],
            "the id must stay one argument: {line}"
        );
    }

    #[test]
    fn fr_002_embedded_quotes_cannot_inject_arguments() {
        round_trips(r#"a "b" c"#);
        round_trips(r#"""#);
        round_trips(r#"quote-at-end""#);
        round_trips(r#"\"#);
        round_trips(r#"mix\"and\\"more"#);

        // The shape an attacker would try: close the quote, add flags.
        let injected = r#"x" --files "C:\secret.pdf"#;
        let line = format!("--profile {} --action queue", quote_arg(injected));
        assert_eq!(
            argv_of(&line),
            vec![
                "--profile".to_string(),
                injected.to_string(),
                "--action".to_string(),
                "queue".to_string()
            ],
            "no extra argument may appear: {line}"
        );
    }

    #[test]
    fn fr_003_the_handoff_path_is_quoted_the_same_way() {
        // Windows paths are the reason this has to be right: they are full of
        // backslashes, and a directory argument legitimately ends in one.
        round_trips(r"C:\Users\ana\AppData\Roaming\MaeclyPrint\handoff\sel-1.json");
        round_trips(r"C:\Program Files\Maecly Print\sel 1.json");

        let path = r"C:\Users\ana\AppData\Roaming\MaeclyPrint\handoff\sel-1.json";
        let line = format!(
            "--invoke=context-menu --files-from {} --action print-default",
            quote_arg(path)
        );
        assert_eq!(
            argv_of(&line),
            vec![
                "--invoke=context-menu",
                "--files-from",
                path,
                "--action",
                "print-default"
            ]
        );
    }
}
