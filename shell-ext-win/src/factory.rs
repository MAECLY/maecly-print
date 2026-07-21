//! `IClassFactory` — the only thing `DllGetClassObject` hands out.

use windows::core::implement;
use windows::Win32::Foundation::{BOOL, CLASS_E_NOAGGREGATION, E_POINTER};
use windows::Win32::System::Com::{IClassFactory, IClassFactory_Impl};
use windows_core::{IUnknown, Interface, GUID};

use crate::command::RootCommand;
use crate::guard::guard;
use crate::ServerRef;

#[implement(IClassFactory)]
pub struct CommandFactory {
    _server: ServerRef,
}

impl CommandFactory {
    pub fn new() -> Self {
        Self {
            _server: ServerRef::new(),
        }
    }
}

impl Default for CommandFactory {
    fn default() -> Self {
        Self::new()
    }
}

impl IClassFactory_Impl for CommandFactory_Impl {
    fn CreateInstance(
        &self,
        outer: Option<&IUnknown>,
        riid: *const GUID,
        ppv: *mut *mut core::ffi::c_void,
    ) -> windows_core::Result<()> {
        guard("CreateInstance", || {
            if ppv.is_null() || riid.is_null() {
                return Err(E_POINTER.into());
            }
            unsafe { *ppv = core::ptr::null_mut() };
            // We do not support being aggregated, and saying so is required:
            // silently ignoring `outer` would hand back an object whose
            // lifetime the aggregator thinks it controls.
            if outer.is_some() {
                return Err(CLASS_E_NOAGGREGATION.into());
            }

            // `query` AddRefs and writes through `ppv`; the local `command`
            // reference is dropped on return, leaving the caller with exactly
            // one. Returning `as_raw()` instead is the windows-rs in-proc
            // server footgun (#1819).
            let command: IUnknown = RootCommand::new().into();
            unsafe { command.query(riid, ppv) }.ok()
        })
    }

    fn LockServer(&self, lock: BOOL) -> windows_core::Result<()> {
        // The RAII `ServerRef` on every object already keeps the module alive;
        // an explicit lock adds one more count on the same counter.
        use std::sync::atomic::Ordering;
        if lock.as_bool() {
            crate::SERVER_REFS.fetch_add(1, Ordering::SeqCst);
        } else {
            crate::SERVER_REFS.fetch_sub(1, Ordering::SeqCst);
        }
        Ok(())
    }
}
