//! `shell_ext_win.dll` — the "Print with MaeclyPrint" Explorer context menu.
//!
//! # What this crate is
//!
//! An in-process COM server implementing [`IExplorerCommand`] (plus
//! `IObjectWithSelection` and `IEnumExplorerCommand`), which is the only
//! interface the **modern Windows 11 context menu** understands (FR-002).
//! The same object is reachable on Windows 10 through a registry verb with
//! `ExplorerCommandHandler`, so one implementation covers both (see
//! `docs/` and the report in the PR description for the exact registry keys).
//!
//! # What it does when clicked
//!
//! It does **not** print. It serializes the selection to a JSON handoff file
//! and launches
//! `maeclyprint.exe --invoke=context-menu --files-from <path>`; the app
//! validates the file and takes over. A Windows command line is capped at
//! 32,767 characters, so passing paths in argv would break somewhere around
//! 380 files — the handoff file is what makes "any number of files" (FR-003)
//! true. There is no size-based shortcut back to argv: one transport is one
//! transport to test.
//!
//! # Rules this code must not break
//!
//! * **A panic must never cross back into Explorer.** Every COM entry point
//!   goes through [`guard::guard`], and the release profile pins
//!   `panic = "unwind"` so `catch_unwind` actually catches.
//! * **No disk I/O in `GetTitle`.** Explorer calls it on the thread that is
//!   drawing the menu. The menu labels come from a cache loaded earlier, in
//!   `SetSelection` or in a `GetState(fOkToBeSlow = TRUE)` — see
//!   [`strings`] and [`command`].
//! * **The DLL must carry the side-by-side `<msix>` manifest** (`res/`,
//!   embedded by `build.rs`). Without it Explorer will not load the extension
//!   out of the sparse package, and no amount of correct code helps.
//!
//! # Portability of this crate
//!
//! [`handoff`], [`strings`] and [`supported`] are plain Rust and compile — and
//! are tested — on any host, which is what lets the schema and the menu model
//! be verified from a macOS development machine. Everything under
//! `#[cfg(windows)]` needs a Windows target to type-check.
//!
//! [`IExplorerCommand`]: https://learn.microsoft.com/en-us/windows/win32/api/shobjidl_core/nn-shobjidl_core-iexplorercommand

#![deny(missing_docs)]

pub mod handoff;
pub mod strings;
pub mod supported;

#[cfg(windows)]
mod clsid;
#[cfg(windows)]
mod command;
#[cfg(windows)]
mod comstr;
#[cfg(windows)]
mod factory;
#[cfg(windows)]
mod guard;
#[cfg(windows)]
mod launch;
#[cfg(windows)]
mod logging;
#[cfg(windows)]
mod module;
#[cfg(windows)]
mod selection;
#[cfg(windows)]
mod subcommands;

#[cfg(windows)]
pub use clsid::CLSID_MAECLYPRINT_COMMAND;

#[cfg(windows)]
mod exports {
    use std::ffi::c_void;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use windows::Win32::Foundation::{
        BOOL, CLASS_E_CLASSNOTAVAILABLE, E_POINTER, HMODULE, S_FALSE, S_OK, TRUE,
    };
    use windows::Win32::System::Com::IClassFactory;
    use windows::Win32::System::LibraryLoader::DisableThreadLibraryCalls;
    use windows_core::{Interface, GUID, HRESULT};

    /// Live COM objects handed out by this DLL. `DllCanUnloadNow` reports
    /// "unloadable" only at zero, otherwise Explorer could unload the module
    /// out from under an interface it still holds.
    pub(crate) static SERVER_REFS: AtomicUsize = AtomicUsize::new(0);

    /// RAII counter held by every object this server creates.
    pub(crate) struct ServerRef;

    impl ServerRef {
        pub(crate) fn new() -> Self {
            SERVER_REFS.fetch_add(1, Ordering::SeqCst);
            Self
        }
    }

    impl Drop for ServerRef {
        fn drop(&mut self) {
            SERVER_REFS.fetch_sub(1, Ordering::SeqCst);
        }
    }

    const DLL_PROCESS_ATTACH: u32 = 1;

    /// # Safety
    /// Called by the loader. Does nothing that could take the loader lock: it
    /// records the module handle (resolved to a path lazily, later) and opts
    /// out of thread notifications.
    #[no_mangle]
    pub unsafe extern "system" fn DllMain(
        module: HMODULE,
        reason: u32,
        _reserved: *mut c_void,
    ) -> BOOL {
        if reason == DLL_PROCESS_ATTACH {
            crate::module::remember(module);
            // Explorer creates threads constantly; we have no per-thread state.
            let _ = DisableThreadLibraryCalls(module);
        }
        TRUE
    }

    /// # Safety
    /// COM entry point; `rclsid`/`riid`/`ppv` come from the caller.
    #[no_mangle]
    pub unsafe extern "system" fn DllGetClassObject(
        rclsid: *const GUID,
        riid: *const GUID,
        ppv: *mut *mut c_void,
    ) -> HRESULT {
        if ppv.is_null() {
            return E_POINTER;
        }
        *ppv = core::ptr::null_mut();
        if rclsid.is_null() || riid.is_null() {
            return E_POINTER;
        }
        if *rclsid != crate::clsid::CLSID_MAECLYPRINT_COMMAND {
            return CLASS_E_CLASSNOTAVAILABLE;
        }

        // `query` is what makes this correct: it AddRefs and writes through
        // `ppv` itself, so the factory's own reference is released on scope
        // exit and the caller holds the only one. Handing out `as_raw()` from a
        // temporary is the classic windows-rs in-proc-server bug (#1819) —
        // Explorer then gets a pointer to a freed object, or the object is
        // released before `CreateInstance` is ever called.
        let factory: IClassFactory = crate::factory::CommandFactory::new().into();
        factory.query(&*riid, ppv)
    }

    /// # Safety
    /// COM entry point.
    #[no_mangle]
    pub unsafe extern "system" fn DllCanUnloadNow() -> HRESULT {
        if SERVER_REFS.load(Ordering::SeqCst) == 0 {
            S_OK
        } else {
            S_FALSE
        }
    }
}

#[cfg(windows)]
pub(crate) use exports::{ServerRef, SERVER_REFS};
