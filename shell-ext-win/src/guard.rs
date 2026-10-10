//! The panic barrier at the COM boundary.
//!
//! Every method Explorer can call goes through here. Unwinding across an
//! `extern "system"` boundary is undefined behaviour, and in practice it takes
//! Explorer down with it — a shell extension that crashes the desktop is worse
//! than one that does not exist. `catch_unwind` turns any panic into
//! `E_UNEXPECTED`, which Explorer handles by quietly dropping the menu entry.
//!
//! This only works because `Cargo.toml` pins `panic = "unwind"` in both
//! profiles. With `panic = "abort"` (which `src-tauri` uses, and which this
//! crate must therefore never inherit) `catch_unwind` catches nothing and the
//! process dies.

use std::panic::{catch_unwind, AssertUnwindSafe};

use windows::Win32::Foundation::E_UNEXPECTED;
use windows_core::HRESULT;

/// Run `f`, converting a panic into `E_UNEXPECTED`.
///
/// `AssertUnwindSafe` is justified rather than assumed: the only state shared
/// across a call is the selection cell and the string cache. Both are rebuilt
/// or re-read on the next invocation, and neither has an invariant that a
/// half-finished method could break — the worst case is a menu drawn with
/// default labels.
pub fn guard<T>(
    what: &'static str,
    f: impl FnOnce() -> windows_core::Result<T>,
) -> windows_core::Result<T> {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(result) => result,
        Err(_) => {
            crate::logging::panicked(what);
            Err(windows_core::Error::from(E_UNEXPECTED))
        }
    }
}

/// Same, for the handful of methods whose signature is a raw `HRESULT`
/// (`IEnumExplorerCommand::Next`).
pub fn guard_hresult(what: &'static str, f: impl FnOnce() -> HRESULT) -> HRESULT {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(hr) => hr,
        Err(_) => {
            crate::logging::panicked(what);
            E_UNEXPECTED
        }
    }
}
