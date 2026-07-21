//! Making stdout reachable on Windows.
//!
//! The release binary is linked as a GUI subsystem app (`windows_subsystem =
//! "windows"` in `main.rs`) so double-clicking it never flashes a console
//! window. The side effect is that a process started from `cmd.exe` or
//! PowerShell inherits *no* console at all, and everything the headless CLI
//! prints would vanish.
//!
//! [`attach_parent`] fixes that by attaching to the console of the process that
//! launched us, which is what every dual-mode Windows app does. It is called
//! once, before any output, and only on the headless path — the GUI path must
//! not steal the parent's console.
//!
//! `AttachConsole` is declared here rather than pulled from the `windows`
//! crate because the crate's `Win32_System_Console` feature is not enabled for
//! this package (and adding features is not this module's call); the symbol
//! lives in `kernel32`, which every MSVC target already links.

/// Attach to the parent process' console so `println!` reaches the terminal.
/// No-op when there is no parent console (double-clicked, or launched by
/// Explorer) and on every non-Windows platform.
pub fn attach_parent() {
    #[cfg(windows)]
    {
        // -1 = ATTACH_PARENT_PROCESS. A failure means "no console to attach
        // to", which is normal and not worth reporting.
        const ATTACH_PARENT_PROCESS: u32 = u32::MAX;
        unsafe {
            AttachConsole(ATTACH_PARENT_PROCESS);
        }
    }
}

#[cfg(windows)]
extern "system" {
    fn AttachConsole(process_id: u32) -> i32;
}
