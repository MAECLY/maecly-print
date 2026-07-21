//! The root command: what the user sees before opening the sub-menu.
//!
//! # Call orders this has to survive
//!
//! Explorer drives the two menus differently, and the difference is not
//! cosmetic:
//!
//! | Windows 11 top-level menu | "Show more options" |
//! |---|---|
//! | `GetState` → `GetTitle` → `GetIcon` → `GetFlags` → `EnumSubCommands` | `SetSelection` → `GetFlags` → `GetCanonicalName` → `GetToolTip` → `GetTitle` → `GetIcon` → `GetState` → `EnumSubCommands` |
//! | selection arrives as a parameter, `SetSelection` is never called | selection arrives through `SetSelection`, parameters may be null |
//!
//! Both orders are covered by (a) remembering the selection wherever it turns
//! up and (b) loading the label cache at the *first* point in each order that
//! is allowed to be slow: `SetSelection` in the second column, and
//! `GetState(fOkToBeSlow = TRUE)` in the first. `GetTitle` therefore never
//! reads a file in either order.

use std::cell::OnceCell;
use std::rc::Rc;
use std::sync::OnceLock;

use windows::core::implement;
use windows::Win32::Foundation::{BOOL, E_FAIL, E_POINTER};
use windows::Win32::System::Com::IBindCtx;
use windows::Win32::UI::Shell::{
    IEnumExplorerCommand, IExplorerCommand, IExplorerCommand_Impl, IObjectWithSelection,
    IObjectWithSelection_Impl, IShellItemArray, ECF_HASSUBCOMMANDS, ECS_ENABLED, ECS_HIDDEN,
};
use windows_core::{Interface, GUID, PWSTR};

use crate::clsid::CLSID_MAECLYPRINT_COMMAND;
use crate::guard::guard;
use crate::handoff::Action;
use crate::selection::SelectionCell;
use crate::strings::Strings;
use crate::supported::{should_show, PROBE_LIMIT};
use crate::ServerRef;

/// State the root command and every sub-command it hands out look at.
///
/// `Rc`, not `Arc`: the class is registered `ThreadingModel="Apartment"`, so
/// COM guarantees all calls arrive on the one apartment thread that created the
/// object. Nothing here is reachable from another thread.
pub(crate) struct Shared {
    pub(crate) selection: SelectionCell,
    strings: OnceCell<Strings>,
}

impl Shared {
    pub(crate) fn new() -> Rc<Self> {
        Rc::new(Self {
            selection: SelectionCell::default(),
            strings: OnceCell::new(),
        })
    }

    /// Read the label cache, once per object, from a call site that may block.
    /// Never called from `GetTitle`.
    pub(crate) fn load_strings(&self) {
        let _ = self.strings.get_or_init(crate::strings::load);
    }

    /// The labels to draw with: the cache if it has been read, otherwise the
    /// compiled-in English defaults. Deliberately does **not** load: this is
    /// the accessor the drawing path uses.
    pub(crate) fn strings(&self) -> &Strings {
        static DEFAULTS: OnceLock<Strings> = OnceLock::new();
        self.strings
            .get()
            .unwrap_or_else(|| DEFAULTS.get_or_init(Strings::defaults))
    }
}

/// The `IExplorerCommand` registered under [`CLSID_MAECLYPRINT_COMMAND`].
#[implement(IExplorerCommand, IObjectWithSelection)]
pub struct RootCommand {
    _server: ServerRef,
    shared: Rc<Shared>,
}

impl RootCommand {
    pub fn new() -> Self {
        Self {
            _server: ServerRef::new(),
            shared: Shared::new(),
        }
    }
}

impl Default for RootCommand {
    fn default() -> Self {
        Self::new()
    }
}

impl IExplorerCommand_Impl for RootCommand_Impl {
    fn GetTitle(&self, items: Option<&IShellItemArray>) -> windows_core::Result<PWSTR> {
        guard("GetTitle", || {
            self.shared.selection.remember(items);
            // Pure string work. No file is read here, ever.
            crate::comstr::alloc(&self.shared.strings().labels.root)
        })
    }

    fn GetIcon(&self, items: Option<&IShellItemArray>) -> windows_core::Result<PWSTR> {
        guard("GetIcon", || {
            self.shared.selection.remember(items);
            match crate::module::icon_spec() {
                Some(spec) => crate::comstr::alloc(&spec),
                // No icon is a fine menu entry; a failure here must not remove
                // the command.
                None => Err(E_FAIL.into()),
            }
        })
    }

    fn GetToolTip(&self, items: Option<&IShellItemArray>) -> windows_core::Result<PWSTR> {
        guard("GetToolTip", || {
            self.shared.selection.remember(items);
            crate::comstr::alloc(&self.shared.strings().labels.tooltip)
        })
    }

    fn GetCanonicalName(&self) -> windows_core::Result<GUID> {
        Ok(CLSID_MAECLYPRINT_COMMAND)
    }

    fn GetState(
        &self,
        items: Option<&IShellItemArray>,
        ok_to_be_slow: BOOL,
    ) -> windows_core::Result<u32> {
        guard("GetState", || {
            self.shared.selection.remember(items);

            // The top-level menu's first call. If we are allowed to take our
            // time, this is where the label cache is read — so that GetTitle,
            // which comes next, does not have to.
            if ok_to_be_slow.as_bool() {
                self.shared.load_strings();
            }

            let Some(items) = self.shared.selection.resolve(items) else {
                return Ok(ECS_HIDDEN.0 as u32);
            };
            let total = unsafe { items.GetCount() }.unwrap_or(0) as usize;
            // Bounded on purpose: a 10,000-item selection must not be walked
            // on the thread that is drawing the menu.
            let probed = crate::selection::probe(&items, PROBE_LIMIT);

            Ok(if should_show(&probed, total) {
                ECS_ENABLED.0 as u32
            } else {
                ECS_HIDDEN.0 as u32
            })
        })
    }

    fn Invoke(
        &self,
        items: Option<&IShellItemArray>,
        _bind_ctx: Option<&IBindCtx>,
    ) -> windows_core::Result<()> {
        // A command with sub-commands is not normally invoked itself, but a
        // keyboard accelerator or a shell that ignores ECF_HASSUBCOMMANDS can
        // still get here. Do the least surprising thing: open the dialog.
        guard("Invoke", || {
            self.shared.selection.remember(items);
            crate::subcommands::run(&self.shared, items, Action::Configure, None)
        })
    }

    fn GetFlags(&self) -> windows_core::Result<u32> {
        Ok(ECF_HASSUBCOMMANDS.0 as u32)
    }

    fn EnumSubCommands(&self) -> windows_core::Result<IEnumExplorerCommand> {
        guard("EnumSubCommands", || {
            // The "Show more options" order reaches here without ever passing
            // fOkToBeSlow = TRUE, so this is the other slow-safe call site.
            self.shared.load_strings();
            let count = self.shared.selection.count(None);
            let items = self.shared.strings().menu(count);
            crate::subcommands::enumerator(&self.shared, items)
        })
    }
}

impl IObjectWithSelection_Impl for RootCommand_Impl {
    fn SetSelection(&self, items: Option<&IShellItemArray>) -> windows_core::Result<()> {
        guard("SetSelection", || {
            self.shared.selection.remember(items);
            // First call of the "Show more options" order and the only one
            // guaranteed to happen before GetTitle.
            self.shared.load_strings();
            Ok(())
        })
    }

    fn GetSelection(
        &self,
        riid: *const GUID,
        ppv: *mut *mut core::ffi::c_void,
    ) -> windows_core::Result<()> {
        guard("GetSelection", || {
            if ppv.is_null() || riid.is_null() {
                return Err(E_POINTER.into());
            }
            unsafe { *ppv = core::ptr::null_mut() };
            match self.shared.selection.resolve(None) {
                Some(items) => unsafe { items.query(riid, ppv) }.ok(),
                None => Err(E_FAIL.into()),
            }
        })
    }
}
