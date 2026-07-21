//! The one identifier three files have to agree on.
//!
//! This GUID appears in exactly three places, and a mismatch is silent:
//!
//! 1. here;
//! 2. `installers/windows/sparse-msix/AppxManifest.xml`, as
//!    `com:Class/@Id` **and** `desktop5:Verb/@Clsid`;
//! 3. the Windows 10 fallback registry verb
//!    (`HKCU\Software\Classes\*\shell\MaeclyPrint\ExplorerCommandHandler`)
//!    and its `CLSID\{…}\InprocServer32` entry.
//!
//! Never regenerate it: an installed machine would keep the old CLSID
//! registered and the menu entry would simply stop appearing.

use windows_core::GUID;

/// `{B269B8DC-044F-4D65-8F1E-8F20A2E653EA}` — the root `IExplorerCommand`.
pub const CLSID_MAECLYPRINT_COMMAND: GUID = GUID::from_u128(0xB269B8DC_044F_4D65_8F1E_8F20A2E653EA);

/// Canonical name reported by the sub-commands. They are enumerated, never
/// activated by CLSID, so a nil GUID is the documented answer.
pub const GUID_NULL: GUID = GUID::from_u128(0);
