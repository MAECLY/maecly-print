//! Headless CLI and the argv contract (FR-008/009/010/112, NFR-014).
//!
//! The binary has three possible starts, decided by [`startup`] from argv
//! **before** anything else happens — no Tauri builder, no webview, no queue:
//!
//! | argv                                        | mode         | window? |
//! | ------------------------------------------- | ------------ | ------- |
//! | `--invoke=context-menu --files-from <PATH>`  | context menu | yes, pre-loaded |
//! | `print …` / `printers …` / … / `--cli` / `--help` | headless | **no** |
//! | anything else                                | GUI          | yes     |
//!
//! Order matters and matches `docs/04-architecture/ipc-and-cli.md` §3.1:
//! `--invoke=context-menu` wins over everything (a shell extension must never
//! be reinterpreted as a subcommand), then a known subcommand or `--cli`, then
//! the GUI.
//!
//! ## Why the decision lives here and not in `run()`
//!
//! `maeclyprint print --files a.pdf` must not create a window, so the split
//! has to happen in `main` before [`crate::run_with`] is even called.
//! [`startup`] therefore returns either "the work is done, exit with this
//! code" or "boot the GUI, and here is the selection to pre-load".
//!
//! ## What the CLI is allowed to be
//!
//! A facade. Every subcommand calls the same path-injected core the matching
//! Tauri command calls (`ipc::profiles::*_at`, `ipc::settings::*_at`,
//! `platform::*`, `update::*`, `queue::persist`), so there is exactly one
//! implementation of every rule. The only logic that lives here is what only
//! a command line needs: argument vocabulary, name→id lookup, file expansion,
//! output formatting and exit codes.

pub mod args;
pub mod console;
pub mod exit;
pub mod files;
pub mod handoff;
pub mod output;
pub mod pending;
pub mod print;
pub mod printers;
pub mod profiles;
pub mod queue;
pub mod single_instance;
pub mod update;

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use clap::Parser;
use serde::{Deserialize, Serialize};

use crate::cli::args::{Cli, Command, ContextAction, InvokeSource, SUBCOMMANDS};
use crate::cli::exit::{CliError, ExitCode};
use crate::cli::output::Emitter;

/// A file selection handed to the app by a shell extension, a forwarded second
/// launch, or a context-menu-style command line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct Selection {
    /// Absolute paths, exactly as the integration reported them.
    pub paths: Vec<String>,
    /// What the user asked for (open the dialog, print with the default
    /// profile, just queue it).
    pub action: ContextAction,
    /// Profile id/name to apply, when the integration specified one.
    pub profile_id: Option<String>,
}

/// What `main` should do next.
#[derive(Debug)]
pub enum Startup {
    /// The run already happened headlessly; exit with this code.
    Exit(i32),
    /// Boot the GUI, pre-loading `selection` when there is one.
    Gui { selection: Option<Selection> },
}

/// The on-disk files every command reads through. Injected rather than resolved
/// inside each command so tests can point a whole subcommand at a tempdir
/// instead of the developer's real configuration.
#[derive(Debug, Clone)]
pub struct Paths {
    /// `config.toml`.
    pub config: PathBuf,
    /// `profiles.toml`.
    pub profiles: PathBuf,
    /// `queue.json`.
    pub queue: PathBuf,
}

impl Paths {
    /// The real per-user locations (see [`crate::util::paths`]).
    pub fn real() -> Self {
        Self {
            config: crate::settings::Config::config_path(),
            profiles: crate::profiles::profiles_path(),
            queue: crate::util::paths::data_dir().join("queue.json"),
        }
    }
}

/// Which of the three starts an argv asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Intent {
    /// Open the app with a selection from a shell extension.
    ContextMenu,
    /// Run a subcommand and exit; no window.
    Headless,
    /// Plain launch.
    Gui,
}

/// Classify an argv without parsing it.
///
/// Deliberately cheap and total: it must reach a verdict for *any* argv,
/// including the platform noise a GUI launch carries (macOS passes
/// `-psn_0_12345` to bundled apps). The rule is the documented one — the first
/// **non-flag** argument decides — so an unknown leading flag can never turn a
/// double-click into a failed CLI run.
pub fn classify<S: AsRef<str>>(args: &[S]) -> Intent {
    let args: Vec<&str> = args.iter().map(AsRef::as_ref).collect();

    // 1. A context-menu invocation outranks everything else.
    if args.contains(&"--invoke=context-menu")
        || args
            .windows(2)
            .any(|w| w[0] == "--invoke" && w[1] == "context-menu")
    {
        return Intent::ContextMenu;
    }

    // 2. An explicit `--cli`, a help/version request, or a known subcommand.
    for arg in &args {
        match *arg {
            "--cli" | "--help" | "-h" | "--version" | "-V" => return Intent::Headless,
            a if a.starts_with('-') => continue,
            a if SUBCOMMANDS.contains(&a) => return Intent::Headless,
            // A bare path is an "open with" launch: the shell handed us a file
            // to show, not a command to run.
            a if Path::new(a).exists() => return Intent::Gui,
            // Anything else is a typo. Routing it to the CLI lets clap answer
            // with "unrecognized subcommand" and a non-zero code; routing it to
            // the GUI would silently open a window for `maeclyprint frobnicate`.
            _ => return Intent::Headless,
        }
    }

    // 3. Anything else is a plain launch.
    Intent::Gui
}

/// Decide what this process does, and — in headless mode — do it.
///
/// `argv` includes the program name, exactly as [`std::env::args_os`] yields
/// it. This is the single entry point `main` calls.
pub fn startup<I, T>(argv: I) -> Startup
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let argv: Vec<OsString> = argv.into_iter().map(Into::into).collect();
    let tail: Vec<String> = argv
        .iter()
        .skip(1)
        .map(|a| a.to_string_lossy().into_owned())
        .collect();

    match classify(&tail) {
        Intent::Gui => Startup::Gui { selection: None },

        Intent::ContextMenu => {
            // A malformed shell-extension invocation must not leave the user
            // staring at nothing: report it and open the app empty, which is
            // still a usable outcome (they can pick files themselves).
            let selection = match Cli::try_parse_from(&argv).map_err(context_menu_parse_error) {
                Ok(cli) => selection_from(&cli).map_err(|e| e.error.to_string()),
                Err(message) => Err(message),
            };
            match selection {
                Ok(selection) => Startup::Gui {
                    selection: Some(selection),
                },
                Err(message) => {
                    eprintln!("maeclyprint: ignoring the context-menu selection: {message}");
                    Startup::Gui { selection: None }
                }
            }
        }

        Intent::Headless => {
            console::attach_parent();
            match Cli::try_parse_from(&argv) {
                Ok(cli) => Startup::Exit(execute(cli)),
                Err(err) => {
                    // clap prints help/version to stdout and errors to stderr
                    // itself; we only own the exit code.
                    let _ = err.print();
                    Startup::Exit(clap_exit_code(err.kind()).code())
                }
            }
        }
    }
}

/// Run a parsed headless command line and return its exit code. Every failure
/// is reported through the emitter, so `--json` errors land on stderr as JSON
/// and human errors as one `error: …` line.
fn execute(cli: Cli) -> i32 {
    let emitter = Emitter::new(cli.json);
    let paths = Paths::real();

    let result = match cli.command {
        Some(Command::Print(args)) => print::run(&paths, emitter, args),
        Some(Command::Printers { cmd }) => printers::run(emitter, cmd).map(|_| ExitCode::Success),
        Some(Command::Profiles { cmd }) => {
            profiles::run(&paths, emitter, cmd).map(|_| ExitCode::Success)
        }
        Some(Command::Queue { cmd }) => queue::run(&paths, emitter, cmd).map(|_| ExitCode::Success),
        Some(Command::Update { cmd }) => {
            update::run(&paths, emitter, cmd).map(|_| ExitCode::Success)
        }
        // `--cli` (or a bare `--invoke`) with nothing to do: show the help and
        // report invalid usage, the same as any other incomplete command line.
        None => {
            use clap::CommandFactory;
            let _ = Cli::command().print_help();
            Err(CliError::usage("no subcommand was given"))
        }
    };

    match result {
        Ok(code) => code.code(),
        Err(err) => {
            emitter.fail(&err);
            err.exit.code()
        }
    }
}

/// Build the [`Selection`] a context-menu invocation carries.
///
/// The handoff file is the only transport (see [`handoff`]), and it is treated
/// as untrusted input: it must sit inside the handoff directory and be recent,
/// or it is refused. The payload is authoritative for `action`/`profile_id`;
/// the `--action`/`--profile` flags are the fallback for a payload that omits
/// them (or for a hand-typed invocation).
pub fn selection_from(cli: &Cli) -> Result<Selection, CliError> {
    if cli.invoke != Some(InvokeSource::ContextMenu) {
        return Err(CliError::usage("not a context-menu invocation"));
    }
    let path = cli
        .files_from
        .as_ref()
        .ok_or_else(|| CliError::usage("--invoke=context-menu requires --files-from <PATH>"))?;

    if !handoff::is_within_dir(&handoff::handoff_dir(), path) {
        return Err(CliError::usage(format!(
            "{} is not inside the handoff directory",
            path.display()
        )));
    }

    let payload = handoff::consume(path)?;
    if !handoff::is_fresh(
        &payload.invoked_at,
        time::OffsetDateTime::now_utc(),
        handoff::MAX_HANDOFF_AGE,
    ) {
        return Err(CliError::usage(
            "handoff file is stale; the selection was not honoured",
        ));
    }

    Ok(selection_from_payload(
        payload,
        cli.action,
        cli.profile.clone(),
    ))
}

/// Fold a validated handoff payload and the argv fallbacks into a [`Selection`].
///
/// Split out from [`selection_from`] because it is the whole decision — what
/// the app will actually do — with none of the I/O, so it can be asserted
/// directly. The payload wins wherever it has an opinion; `--action`/`--profile`
/// cover a payload that omits one (or a hand-typed invocation).
pub fn selection_from_payload(
    payload: handoff::Handoff,
    action: ContextAction,
    profile: Option<String>,
) -> Selection {
    Selection {
        paths: payload.paths,
        action: payload.action.unwrap_or(action),
        profile_id: payload.profile_id.or(profile),
    }
}

/// Turn a clap failure on the context-menu path into a message. Help/version
/// requests are impossible here (they would have classified as headless), so
/// every kind is a real error.
fn context_menu_parse_error(err: clap::Error) -> String {
    err.kind()
        .as_str()
        .unwrap_or("invalid arguments")
        .to_string()
}

/// Exit code for a clap outcome: printing `--help`/`--version` succeeded;
/// everything else is invalid usage (FR-009's code 2).
fn clap_exit_code(kind: clap::error::ErrorKind) -> ExitCode {
    use clap::error::ErrorKind;
    match kind {
        ErrorKind::DisplayHelp | ErrorKind::DisplayVersion => ExitCode::Success,
        _ => ExitCode::Usage,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fr_008_subcommands_switch_to_headless() {
        for sub in SUBCOMMANDS {
            assert_eq!(classify(&[*sub]), Intent::Headless, "{sub} is headless");
        }
        assert_eq!(classify(&["print", "--files", "a.pdf"]), Intent::Headless);
        assert_eq!(classify(&["--cli"]), Intent::Headless);
        assert_eq!(classify(&["--json", "queue", "status"]), Intent::Headless);
    }

    #[test]
    fn fr_112_help_and_version_are_headless() {
        for arg in ["--help", "-h", "--version", "-V"] {
            assert_eq!(classify(&[arg]), Intent::Headless);
        }
        assert_eq!(classify(&["print", "--help"]), Intent::Headless);
    }

    #[test]
    fn nfr_014_context_menu_outranks_everything() {
        assert_eq!(
            classify(&["--invoke=context-menu", "--files-from", "/tmp/s.json"]),
            Intent::ContextMenu
        );
        assert_eq!(
            classify(&["--invoke", "context-menu", "--files-from", "/tmp/s.json"]),
            Intent::ContextMenu
        );
        // Even paired with a subcommand: the shell extension always wins.
        assert_eq!(
            classify(&["print", "--invoke=context-menu"]),
            Intent::ContextMenu
        );
    }

    #[test]
    fn fr_008_anything_else_opens_the_gui() {
        let empty: [&str; 0] = [];
        assert_eq!(classify(&empty), Intent::Gui);
        // macOS hands a bundled app its process serial number.
        assert_eq!(classify(&["-psn_0_1234"]), Intent::Gui);
    }

    #[test]
    fn fr_008_an_existing_path_is_an_open_with_launch() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join("report.pdf");
        std::fs::write(&file, b"%PDF-1.4\n").expect("write");
        assert_eq!(
            classify(&[file.to_string_lossy().as_ref()]),
            Intent::Gui,
            "the shell handing us a real file must open the app on it"
        );
    }

    /// Regression: `maeclyprint frobnicate` used to open a window instead of
    /// reporting an unknown subcommand, so a typo silently spawned a GUI.
    #[test]
    fn fr_009_an_unknown_word_is_a_cli_error_not_a_window() {
        assert_eq!(classify(&["frobnicate"]), Intent::Headless);
        // A path-looking argument that does not exist is still a typo, not a
        // launch: opening an empty window would hide the mistake.
        assert_eq!(classify(&["Documents/report.pdf"]), Intent::Headless);
    }

    #[test]
    fn fr_009_clap_outcomes_map_to_exit_codes() {
        use clap::error::ErrorKind;
        assert_eq!(clap_exit_code(ErrorKind::DisplayHelp), ExitCode::Success);
        assert_eq!(clap_exit_code(ErrorKind::DisplayVersion), ExitCode::Success);
        assert_eq!(clap_exit_code(ErrorKind::UnknownArgument), ExitCode::Usage);
        assert_eq!(
            clap_exit_code(ErrorKind::MissingRequiredArgument),
            ExitCode::Usage
        );
    }
}
