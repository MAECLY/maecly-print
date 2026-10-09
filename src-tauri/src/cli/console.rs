//! Making stdout reachable on Windows.
//!
//! The release `maeclyprint.exe` is linked as a GUI subsystem app
//! (`windows_subsystem = "windows"` in `main.rs`) so double-clicking it never
//! flashes a console window. The side effect is that a process started from
//! `cmd.exe` or PowerShell inherits *no* console at all, and everything the
//! headless CLI prints would vanish.
//!
//! [`attach_parent`] fixes the output half: it attaches to the console of the
//! process that launched us and points stdout/stderr at it. It is called once,
//! before any output, and only on the headless path — the GUI path must not
//! steal the parent's console.
//!
//! It cannot fix the *waiting* half. `cmd.exe` and PowerShell do not wait for a
//! GUI-subsystem program, so the prompt comes back at once and `%ERRORLEVEL%` /
//! `$LASTEXITCODE` never see our exit code. That is what the console-subsystem
//! `maeclyprint-cli.exe` (`src/bin/maeclyprint-cli.rs`) is for; scripts use it,
//! or `start /wait` / `Start-Process -Wait` around the GUI exe. See
//! `docs/04-architecture/ipc-and-cli.md` §3.

/// Attach to the parent process' console so `println!` reaches the terminal.
///
/// Handles the parent already gave us — `maeclyprint print … > out.txt`, a
/// pipe, or the console-subsystem shim's inherited handles — are kept: output
/// must go where the caller redirected it. Only a missing stdout/stderr is
/// pointed at the attached console (`CONOUT$`), because attaching alone does
/// not reliably populate the standard handles of a GUI-subsystem process.
///
/// No-op when there is no parent console (double-clicked, or launched by
/// Explorer) and on every non-Windows platform.
pub fn attach_parent() {
    #[cfg(windows)]
    imp::attach_parent();
}

#[cfg(windows)]
mod imp {
    use windows::core::w;
    use windows::Win32::Foundation::{GENERIC_READ, GENERIC_WRITE, HANDLE};
    use windows::Win32::Storage::FileSystem::{
        CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
    };
    use windows::Win32::System::Console::{
        AttachConsole, GetStdHandle, SetStdHandle, ATTACH_PARENT_PROCESS, STD_ERROR_HANDLE,
        STD_HANDLE, STD_OUTPUT_HANDLE,
    };

    /// A standard handle that points somewhere (a console, file or pipe).
    fn present(which: STD_HANDLE) -> bool {
        match unsafe { GetStdHandle(which) } {
            Ok(h) => !h.is_invalid() && !h.0.is_null(),
            Err(_) => false,
        }
    }

    fn conout() -> Option<HANDLE> {
        unsafe {
            CreateFileW(
                w!("CONOUT$"),
                (GENERIC_READ | GENERIC_WRITE).0,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                None,
                OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL,
                None,
            )
        }
        .ok()
    }

    pub(super) fn attach_parent() {
        let has_out = present(STD_OUTPUT_HANDLE);
        let has_err = present(STD_ERROR_HANDLE);
        if has_out && has_err {
            // Both already go somewhere the caller chose.
            return;
        }
        // A failure means "no parent console" (Explorer, a double-click, a
        // scheduled task): normal, and nothing to report it to.
        if unsafe { AttachConsole(ATTACH_PARENT_PROCESS) }.is_err() {
            return;
        }
        for (which, has) in [(STD_OUTPUT_HANDLE, has_out), (STD_ERROR_HANDLE, has_err)] {
            if has || present(which) {
                continue;
            }
            if let Some(h) = conout() {
                let _ = unsafe { SetStdHandle(which, h) };
            }
        }
    }
}
