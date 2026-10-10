//! The sub-menu: leaf commands and the enumerator that hands them out.
//!
//! The shape is constrained by two documented platform limits (see
//! `docs/05-features/context-menu-actions.md`):
//!
//! * `IExplorerCommand::EnumSubCommands` — *"Subcommands which themselves have
//!   subcommands are not supported by Windows Explorer."* So the per-profile
//!   entries are flat, not a nested "Print with profile ▸".
//! * A handler contributes at most 16 items. [`crate::strings::Strings::menu`]
//!   owns that budget.
//!
//! Every leaf does the same thing on click: resolve the selection to real
//! paths, write a handoff file, launch the app. Which entry was clicked only
//! changes the `action`/`profile_id` written into that file.

use std::cell::Cell;
use std::rc::Rc;

use windows::core::implement;
use windows::Win32::Foundation::{BOOL, E_FAIL, E_NOTIMPL, E_POINTER, S_FALSE, S_OK};
use windows::Win32::System::Com::IBindCtx;
use windows::Win32::UI::Shell::{
    IEnumExplorerCommand, IEnumExplorerCommand_Impl, IExplorerCommand, IExplorerCommand_Impl,
    IShellItemArray, ECF_DEFAULT, ECF_ISSEPARATOR, ECS_ENABLED,
};
use windows_core::{GUID, HRESULT, PWSTR};

use crate::clsid::GUID_NULL;
use crate::command::Shared;
use crate::guard::{guard, guard_hresult};
use crate::handoff::Action;
use crate::strings::{MenuCommand, MenuItem};
use crate::ServerRef;

/// Build the enumerator Explorer asks for in `EnumSubCommands`.
pub(crate) fn enumerator(
    shared: &Rc<Shared>,
    items: Vec<MenuItem>,
) -> windows_core::Result<IEnumExplorerCommand> {
    let commands: Vec<IExplorerCommand> = items
        .into_iter()
        .map(|item| SubCommand::new(shared.clone(), item).into())
        .collect();
    Ok(SubCommandEnum::new(commands).into())
}

/// Shared body of every leaf `Invoke`, and of the root's fallback.
pub(crate) fn run(
    shared: &Rc<Shared>,
    items: Option<&IShellItemArray>,
    action: Action,
    profile_id: Option<String>,
) -> windows_core::Result<()> {
    let Some(items) = shared.selection.resolve(items) else {
        crate::logging::failed("invoke", "no selection was available");
        return Err(E_FAIL.into());
    };

    // Only now — after a click — is it acceptable to walk the whole selection.
    let paths = crate::selection::paths(&items);
    let count = paths.len();

    match crate::launch::deliver(&paths, action, profile_id) {
        Ok(handoff) => {
            crate::logging::invoked(action.as_str(), count, &handoff);
            Ok(())
        }
        Err(err) => {
            crate::logging::failed("deliver", &err.to_string());
            Err(E_FAIL.into())
        }
    }
}

/// One entry in the sub-menu: either a separator or a command.
#[implement(IExplorerCommand)]
pub struct SubCommand {
    _server: ServerRef,
    shared: Rc<Shared>,
    item: MenuItem,
}

impl SubCommand {
    fn new(shared: Rc<Shared>, item: MenuItem) -> Self {
        Self {
            _server: ServerRef::new(),
            shared,
            item,
        }
    }

    fn command(&self) -> Option<&MenuCommand> {
        match &self.item {
            MenuItem::Command(command) => Some(command),
            MenuItem::Separator => None,
        }
    }
}

impl IExplorerCommand_Impl for SubCommand_Impl {
    fn GetTitle(&self, items: Option<&IShellItemArray>) -> windows_core::Result<PWSTR> {
        guard("SubCommand::GetTitle", || {
            self.shared.selection.remember(items);
            match self.command() {
                // A separator has no title; E_NOTIMPL is how the shell is told
                // so, and it is paired with ECF_ISSEPARATOR below.
                None => Err(E_NOTIMPL.into()),
                Some(command) => crate::comstr::alloc(&command.title),
            }
        })
    }

    fn GetIcon(&self, _items: Option<&IShellItemArray>) -> windows_core::Result<PWSTR> {
        // Leaf entries inherit the parent's icon; drawing eight copies of the
        // app icon in one sub-menu is noise.
        Err(E_NOTIMPL.into())
    }

    fn GetToolTip(&self, _items: Option<&IShellItemArray>) -> windows_core::Result<PWSTR> {
        Err(E_NOTIMPL.into())
    }

    fn GetCanonicalName(&self) -> windows_core::Result<GUID> {
        // Sub-commands are enumerated, never activated by name.
        Ok(GUID_NULL)
    }

    fn GetState(
        &self,
        items: Option<&IShellItemArray>,
        _ok_to_be_slow: BOOL,
    ) -> windows_core::Result<u32> {
        guard("SubCommand::GetState", || {
            self.shared.selection.remember(items);
            // The root already decided whether the whole entry is worth
            // showing; re-probing per leaf would multiply that work by 13.
            Ok(ECS_ENABLED.0 as u32)
        })
    }

    fn Invoke(
        &self,
        items: Option<&IShellItemArray>,
        _bind_ctx: Option<&IBindCtx>,
    ) -> windows_core::Result<()> {
        guard("SubCommand::Invoke", || {
            self.shared.selection.remember(items);
            match self.command() {
                None => Ok(()),
                Some(command) => run(
                    &self.shared,
                    items,
                    command.action,
                    command.profile_id.clone(),
                ),
            }
        })
    }

    fn GetFlags(&self) -> windows_core::Result<u32> {
        // `_EXPCMDFLAGS` has no "this is the default/bold entry" bit — the only
        // flags are structural (separator, sub-commands, split button, …). So
        // `MenuCommand::is_default` cannot be expressed here; it decides
        // *ordering* instead, and the first entry is the one Explorer treats as
        // the default action. Keeping the field is still right: the day a flag
        // for it exists, or the day the order changes, this is where it lands.
        Ok(match self.command() {
            None => ECF_ISSEPARATOR.0 as u32,
            Some(_) => ECF_DEFAULT.0 as u32,
        })
    }

    fn EnumSubCommands(&self) -> windows_core::Result<IEnumExplorerCommand> {
        // Two levels of nesting are not supported by Explorer; saying so
        // plainly is better than returning an empty enumerator.
        Err(E_NOTIMPL.into())
    }
}

/// `IEnumExplorerCommand` over a fixed list.
#[implement(IEnumExplorerCommand)]
pub struct SubCommandEnum {
    _server: ServerRef,
    commands: Vec<IExplorerCommand>,
    /// Cursor. `Cell`, not `AtomicUsize`: apartment-threaded, single thread.
    index: Cell<usize>,
}

impl SubCommandEnum {
    fn new(commands: Vec<IExplorerCommand>) -> Self {
        Self {
            _server: ServerRef::new(),
            commands,
            index: Cell::new(0),
        }
    }
}

impl IEnumExplorerCommand_Impl for SubCommandEnum_Impl {
    fn Next(
        &self,
        celt: u32,
        commands_out: *mut Option<IExplorerCommand>,
        fetched_out: *mut u32,
    ) -> HRESULT {
        guard_hresult("Next", || {
            if commands_out.is_null() {
                return E_POINTER;
            }
            let mut index = self.index.get();
            let mut fetched = 0usize;
            while fetched < celt as usize && index < self.commands.len() {
                // `write`, not assignment: the output slots are uninitialized
                // memory and assigning would drop whatever bit pattern is in
                // them as if it were an interface pointer.
                unsafe {
                    commands_out
                        .add(fetched)
                        .write(Some(self.commands[index].clone()))
                };
                fetched += 1;
                index += 1;
            }
            self.index.set(index);

            // Optional per the interface contract, but Explorer passes it.
            if !fetched_out.is_null() {
                unsafe { fetched_out.write(fetched as u32) };
            }

            if fetched == celt as usize {
                S_OK
            } else {
                S_FALSE
            }
        })
    }

    fn Skip(&self, celt: u32) -> windows_core::Result<()> {
        guard("Skip", || {
            let index = self.index.get().saturating_add(celt as usize);
            self.index.set(index.min(self.commands.len()));
            Ok(())
        })
    }

    fn Reset(&self) -> windows_core::Result<()> {
        self.index.set(0);
        Ok(())
    }

    fn Clone(&self) -> windows_core::Result<IEnumExplorerCommand> {
        guard("Clone", || {
            let copy = SubCommandEnum::new(self.commands.clone());
            copy.index.set(self.index.get());
            Ok(copy.into())
        })
    }
}
