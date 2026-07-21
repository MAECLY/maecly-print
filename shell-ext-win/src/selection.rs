//! Reading a selection out of an `IShellItemArray`, and remembering it.
//!
//! # Why the selection has to be cached
//!
//! Explorer hands the selection to the handler in two different ways depending
//! on which menu is being built:
//!
//! * **Top-level (Windows 11) menu** — `SetSelection` is *never* called. The
//!   array arrives as the `psiitemarray` parameter of `GetState`, `GetTitle`,
//!   `GetIcon` and `Invoke`. But `EnumSubCommands` takes no parameters at all,
//!   so by the time the sub-menu is built the array has to have been kept.
//! * **"Show more options"** — `SetSelection` is called first and the
//!   per-method `psiitemarray` may be null.
//!
//! [`SelectionCell`] absorbs the difference: every method that sees a non-null
//! array stores it, and everything else reads the stored one.
//!
//! # Why `IShellItemArray` has no 15-item limit
//!
//! It never had one. The documented caps belong to the *verb selection model*:
//! a legacy command-line verb gets 15 items with `MultiSelectModel="Document"`
//! and 100 with `"Player"`, while a COM-based verb — which this is, and for
//! which `Player` is assumed — has no limit at all (FR-003).

use std::cell::RefCell;
use std::path::PathBuf;

use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::System::SystemServices::SFGAO_FOLDER;
use windows::Win32::UI::Shell::{
    IShellItem, IShellItemArray, SIGDN_FILESYSPATH, SIGDN_PARENTRELATIVEPARSING,
};

use crate::supported::ProbeItem;

/// The selection, as last seen through either delivery route.
#[derive(Default)]
pub struct SelectionCell {
    items: RefCell<Option<IShellItemArray>>,
}

impl SelectionCell {
    /// Store `items` if it is non-null. Called from every method that receives
    /// a `psiitemarray`, including the ones that do not otherwise need it.
    pub fn remember(&self, items: Option<&IShellItemArray>) {
        if let Some(items) = items {
            *self.items.borrow_mut() = Some(items.clone());
        }
    }

    /// The array to work with: the one just handed to us, else the stored one.
    pub fn resolve(&self, items: Option<&IShellItemArray>) -> Option<IShellItemArray> {
        match items {
            Some(items) => Some(items.clone()),
            None => self.items.borrow().clone(),
        }
    }

    /// Number of selected items. `GetCount` reads a count the shell already
    /// has, so this is safe to call while a menu is being drawn.
    pub fn count(&self, items: Option<&IShellItemArray>) -> usize {
        self.resolve(items)
            .and_then(|items| unsafe { items.GetCount() }.ok())
            .unwrap_or(0) as usize
    }
}

/// Inspect at most `limit` items, cheaply, for the show/hide decision.
///
/// `SIGDN_PARENTRELATIVEPARSING` returns the item's own name without resolving
/// a full filesystem path, which is all the extension check needs and is the
/// cheapest display name the shell offers. `SFGAO_FOLDER` comes from the parent
/// folder's cached attributes. Neither touches the disk for a normal
/// filesystem selection.
pub fn probe(items: &IShellItemArray, limit: usize) -> Vec<ProbeItem> {
    let count = unsafe { items.GetCount() }.unwrap_or(0) as usize;
    let mut out = Vec::with_capacity(count.min(limit));
    for index in 0..count.min(limit) {
        let Ok(item) = (unsafe { items.GetItemAt(index as u32) }) else {
            continue;
        };
        let Some(name) = display_name(&item, SIGDN_PARENTRELATIVEPARSING) else {
            continue;
        };
        let is_folder = unsafe { item.GetAttributes(SFGAO_FOLDER) }
            .map(|flags| flags.0 & SFGAO_FOLDER.0 != 0)
            .unwrap_or(false);
        out.push(ProbeItem { name, is_folder });
    }
    out
}

/// Every real filesystem path in the selection.
///
/// Called from `Invoke` only — never while a menu is being drawn — because for
/// a large selection this is thousands of shell calls.
///
/// `SIGDN_FILESYSPATH` fails for virtual items (a search result that is not
/// backed by a file, a device in "This PC", a mail attachment). That failure is
/// the point: such an item is skipped rather than turned into a made-up path
/// the app would then fail to open.
pub fn paths(items: &IShellItemArray) -> Vec<PathBuf> {
    let count = unsafe { items.GetCount() }.unwrap_or(0);
    let mut out = Vec::with_capacity(count as usize);
    for index in 0..count {
        let Ok(item) = (unsafe { items.GetItemAt(index) }) else {
            continue;
        };
        if let Some(path) = display_name(&item, SIGDN_FILESYSPATH) {
            if !path.is_empty() {
                out.push(PathBuf::from(path));
            }
        }
    }
    out
}

/// `IShellItem::GetDisplayName` returns a `CoTaskMemAlloc`'d string that the
/// caller owns; forgetting the free here would leak once per menu item per
/// right-click, inside a process that runs for days.
fn display_name(item: &IShellItem, kind: windows::Win32::UI::Shell::SIGDN) -> Option<String> {
    let raw = unsafe { item.GetDisplayName(kind) }.ok()?;
    if raw.is_null() {
        return None;
    }
    let text = unsafe { raw.to_string() }.ok();
    unsafe { CoTaskMemFree(Some(raw.0 as *const core::ffi::c_void)) };
    text
}
