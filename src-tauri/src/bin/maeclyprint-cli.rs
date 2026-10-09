//! `maeclyprint-cli` — the console-subsystem front end of the headless CLI.
//!
//! `maeclyprint.exe` is a GUI-subsystem program on Windows (no console window
//! on a double-click). `cmd.exe` and PowerShell do not wait for such a program:
//! the prompt returns at once, the output lands after it, and `%ERRORLEVEL%` /
//! `$LASTEXITCODE` never see the FR-009 exit code. This binary is linked as a
//! normal console program, so shells wait for it and read its exit code, and it
//! runs exactly the same CLI (`cli::startup`) in-process.
//!
//! When the command line asks for the GUI instead (no subcommand, a
//! context-menu invocation, an "open with" path) it starts the sibling
//! `maeclyprint` executable with the same arguments, detached, and exits 0 —
//! a terminal must not stay blocked for as long as a window is open.
//!
//! On macOS and Linux both binaries are ordinary console programs; this one is
//! still built (and bundled) so scripts can use the same name everywhere.

use std::process::Command;

use maeclyprint_lib::cli::{self, Startup};

/// The GUI executable this one sits next to.
#[cfg(windows)]
const GUI_EXE: &str = "maeclyprint.exe";
#[cfg(not(windows))]
const GUI_EXE: &str = "maeclyprint";

fn main() {
    match cli::startup(std::env::args_os()) {
        Startup::Exit(code) => cli::exit_process(code),
        Startup::Gui { .. } => cli::exit_process(launch_gui()),
    }
}

/// Start the GUI with our own arguments and return the exit code to report.
fn launch_gui() -> i32 {
    let exe = match std::env::current_exe() {
        Ok(me) => me.with_file_name(GUI_EXE),
        Err(e) => {
            eprintln!("maeclyprint: cannot locate the MaeclyPrint app: {e}");
            return cli::exit::ExitCode::Generic.code();
        }
    };
    let mut command = Command::new(&exe);
    command.args(std::env::args_os().skip(1));
    detach(&mut command);
    match command.spawn() {
        Ok(_) => 0,
        Err(e) => {
            eprintln!("maeclyprint: cannot start {}: {e}", exe.display());
            cli::exit::ExitCode::Generic.code()
        }
    }
}

/// Do not tie the GUI to this console: it must survive the terminal closing
/// and must not receive its Ctrl+C.
#[cfg(windows)]
fn detach(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    command.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
}

#[cfg(not(windows))]
fn detach(command: &mut Command) {
    use std::process::Stdio;
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
}
