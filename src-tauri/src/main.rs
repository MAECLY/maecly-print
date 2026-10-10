// Hide the Windows console window in release builds. The headless CLI attaches
// to the parent console instead (see `cli::console`), so this costs no output;
// scripts that need cmd/PowerShell to *wait* for the exit code use the
// console-subsystem `pluryprint-cli.exe` (src/bin/pluryprint-cli.rs).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use pluryprint_lib::cli::{self, Startup};

fn main() {
    let argv: Vec<std::ffi::OsString> = std::env::args_os().collect();
    // A hidden worker (`pluryprint --worker <kind> …`, D10) is decided first,
    // before single-instance, the updater, the tray or any menu: it is never a
    // second launch to forward and never shows anything.
    if let Some(code) = pluryprint_lib::worker::dispatch(&argv) {
        cli::exit_process(code);
    }
    // GUI or headless is decided here, from argv alone, before any Tauri
    // machinery exists: `pluryprint print …` must never create a window.
    match cli::startup(argv) {
        Startup::Exit(code) => cli::exit_process(code),
        Startup::Gui { selection, handoff } => pluryprint_lib::run_with(selection, handoff),
    }
}
