// Hide the Windows console window in release builds. The headless CLI attaches
// to the parent console instead (see `cli::console`), so this costs no output;
// scripts that need cmd/PowerShell to *wait* for the exit code use the
// console-subsystem `maeclyprint-cli.exe` (src/bin/maeclyprint-cli.rs).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use maeclyprint_lib::cli::{self, Startup};

fn main() {
    // GUI or headless is decided here, from argv alone, before any Tauri
    // machinery exists: `maeclyprint print …` must never create a window.
    match cli::startup(std::env::args_os()) {
        Startup::Exit(code) => cli::exit_process(code),
        Startup::Gui { selection, handoff } => maeclyprint_lib::run_with(selection, handoff),
    }
}
