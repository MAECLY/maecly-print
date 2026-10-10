//! FR-001 / FR-083 / ADR-0008 — what the Windows installer ships and registers.
//!
//! The per-user NSIS installer is configured in `tauri.conf.json` and extended
//! by `src-tauri/windows/installer-hooks.nsh`, which installs the Explorer
//! context-menu DLL and registers it. None of that is exercised by `cargo
//! test` (it only runs when an installer is built and run), so the contract
//! is pinned here:
//!
//! - the installer is NSIS only, per user, and uses the hooks;
//! - every registry write is under HKCU (no elevation, NFR-020);
//! - the CLSID is the one the DLL serves (`shell-ext-win/src/clsid.rs`), on
//!   both files and folders, as an `ExplorerCommandHandler` verb (the DLL
//!   implements `IExplorerCommand` only, not `IContextMenu`);
//! - uninstall removes what install wrote;
//! - the scripts stay ASCII for Windows PowerShell 5.1 and NSIS.

use std::path::PathBuf;

fn repo(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join(rel)
}

fn read(rel: &str) -> String {
    let p = repo(rel);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

fn conf() -> serde_json::Value {
    serde_json::from_str(&read("src-tauri/tauri.conf.json")).expect("tauri.conf.json parses")
}

/// The CLSID as `{XXXXXXXX-XXXX-XXXX-XXXX-XXXXXXXXXXXX}`, read from the DLL's
/// own source of truth.
fn dll_clsid() -> String {
    let src = read("shell-ext-win/src/clsid.rs");
    let line = src
        .lines()
        .find(|l| l.contains("CLSID_MAECLYPRINT_COMMAND: GUID"))
        .expect("CLSID constant");
    let hex = line
        .split("0x")
        .nth(1)
        .expect("hex literal")
        .split(')')
        .next()
        .unwrap()
        .replace('_', "");
    assert_eq!(hex.len(), 32, "{hex}");
    format!(
        "{{{}-{}-{}-{}-{}}}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

#[test]
fn fr_083_the_windows_installer_is_per_user_nsis_with_the_hooks() {
    let conf = conf();
    let targets: Vec<&str> = conf["bundle"]["targets"]
        .as_array()
        .expect("explicit targets, not \"all\"")
        .iter()
        .filter_map(|t| t.as_str())
        .collect();
    assert!(targets.contains(&"nsis"), "{targets:?}");
    assert!(
        !targets.contains(&"msi"),
        "ADR-0008 ships the per-user NSIS installer only; a WiX MSI is per-machine: {targets:?}"
    );
    let nsis = &conf["bundle"]["windows"]["nsis"];
    assert_eq!(nsis["installMode"], "currentUser");
    let hooks = nsis["installerHooks"].as_str().expect("installerHooks");
    assert!(
        repo("src-tauri").join(hooks).is_file(),
        "{hooks} must exist next to tauri.conf.json"
    );
}

#[test]
fn fr_001_the_hooks_register_the_context_menu_dll_per_user() {
    let nsh = read("src-tauri/windows/installer-hooks.nsh");
    let clsid = dll_clsid();
    assert!(
        nsh.contains(&format!("\"{clsid}\"")),
        "the hooks must register {clsid}, the CLSID shell_ext_win.dll serves"
    );
    for needle in [
        "!define MAECLYPRINT_SHELLEXT_SRC \"${__FILEDIR__}\\${MAECLYPRINT_SHELLEXT_DLL}\"",
        "File \"${MAECLYPRINT_SHELLEXT_SRC}\"",
        "\\InprocServer32\" \"\" \"$INSTDIR\\${MAECLYPRINT_SHELLEXT_DLL}\"",
        "\"ThreadingModel\" \"Apartment\"",
        "\"ExplorerCommandHandler\" \"${MAECLYPRINT_SHELLEXT_CLSID}\"",
        "\"MultiSelectModel\" \"Player\"",
        "MAECLYPRINT_REGISTER_VERB \"*\"",
        "MAECLYPRINT_REGISTER_VERB \"Directory\"",
        "!error",
    ] {
        assert!(nsh.contains(needle), "missing {needle}");
    }
    // HKCU only: nothing that needs elevation (NFR-020).
    for line in nsh.lines().filter(|l| !l.trim_start().starts_with(';')) {
        assert!(
            !line.contains("HKLM") && !line.contains("HKCR") && !line.contains("SHCTX"),
            "per-user installs write HKCU only: {line}"
        );
        assert!(
            !line.contains("ContextMenuHandlers"),
            "the DLL implements IExplorerCommand, not IContextMenu: {line}"
        );
    }
}

#[test]
fn fr_001_uninstall_removes_what_install_wrote() {
    let nsh = read("src-tauri/windows/installer-hooks.nsh");
    let un = nsh
        .split("!macro NSIS_HOOK_PREUNINSTALL")
        .nth(1)
        .expect("uninstall hook");
    for key in [
        "Software\\Classes\\*\\shell\\${MAECLYPRINT_SHELLEXT_VERB}",
        "Software\\Classes\\Directory\\shell\\${MAECLYPRINT_SHELLEXT_VERB}",
        "Software\\Classes\\CLSID\\${MAECLYPRINT_SHELLEXT_CLSID}",
    ] {
        assert!(
            un.contains(&format!("DeleteRegKey HKCU \"{key}\"")),
            "uninstall must delete {key}"
        );
    }
    assert!(un.contains("Delete /REBOOTOK \"$INSTDIR\\${MAECLYPRINT_SHELLEXT_DLL}\""));
}

#[test]
fn nfr_070_installer_scripts_are_ascii() {
    for rel in [
        "src-tauri/windows/installer-hooks.nsh",
        "scripts/build-shell-ext.ps1",
        "scripts/check-failed-logons.ps1",
    ] {
        let text = read(rel);
        let bad: Vec<char> = text.chars().filter(|c| !c.is_ascii()).collect();
        assert!(bad.is_empty(), "{rel} has non-ASCII characters: {bad:?}");
    }
}
