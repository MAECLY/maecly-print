//! Where this DLL lives, and therefore where `maeclyprint.exe` lives.
//!
//! The installer puts the executable and the DLL in the same directory, and the
//! sparse package's `com:Class/@Path` is a bare file name resolved against the
//! same external location. Deriving the exe path from our own module path is
//! what keeps the extension working for a per-user install in
//! `%LOCALAPPDATA%\Programs\MaeclyPrint` without reading the registry.
//!
//! `GetModuleFileNameW` is a cheap in-memory lookup, but the result is cached
//! anyway so `GetIcon` — which Explorer calls while drawing — costs nothing
//! after the first menu.

use std::path::PathBuf;
use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::OnceLock;

use windows::Win32::Foundation::HMODULE;
use windows::Win32::System::LibraryLoader::GetModuleFileNameW;

/// File name of the GUI binary, next to this DLL.
pub const APP_EXE: &str = "maeclyprint.exe";

/// Stored as an `isize` because `HMODULE` is a raw pointer newtype and is not
/// `Sync`; the value is only ever handed straight back to Win32.
static MODULE: AtomicIsize = AtomicIsize::new(0);
static MODULE_DIR: OnceLock<Option<PathBuf>> = OnceLock::new();

/// Record the module handle. Called from `DllMain` on `DLL_PROCESS_ATTACH`.
pub fn remember(module: HMODULE) {
    MODULE.store(module.0 as isize, Ordering::SeqCst);
}

/// Directory this DLL was loaded from, or `None` if the handle was never set
/// or the path could not be read.
pub fn dir() -> Option<PathBuf> {
    MODULE_DIR
        .get_or_init(|| {
            let handle = MODULE.load(Ordering::SeqCst);
            if handle == 0 {
                return None;
            }
            let module = HMODULE(handle as *mut core::ffi::c_void);
            // MAX_PATH is not the ceiling for a long path, so use a buffer that
            // covers the extended limit and treat a full buffer as failure
            // (GetModuleFileNameW truncates rather than failing).
            let mut buffer = [0u16; 32_768];
            let len = unsafe { GetModuleFileNameW(module, &mut buffer) } as usize;
            if len == 0 || len >= buffer.len() {
                return None;
            }
            let path = PathBuf::from(String::from_utf16_lossy(&buffer[..len]));
            path.parent().map(PathBuf::from)
        })
        .clone()
}

/// Full path to the GUI binary this extension launches.
pub fn app_exe() -> Option<PathBuf> {
    dir().map(|d| d.join(APP_EXE))
}

/// `"<dir>\maeclyprint.exe,0"` — the `IExplorerCommand::GetIcon` format
/// (module path, comma, icon index).
pub fn icon_spec() -> Option<String> {
    app_exe().map(|exe| format!("{},0", exe.display()))
}
