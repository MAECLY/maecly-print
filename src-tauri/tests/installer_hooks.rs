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
        .find(|l| l.contains("CLSID_PLURYPRINT_COMMAND: GUID"))
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
        "!define PLURYPRINT_SHELLEXT_RES \"${__FILEDIR__}\\..\\resources\\shellext\"",
        "CopyFiles /SILENT \"$INSTDIR\\resources\\shellext\\$R8\\${PLURYPRINT_SHELLEXT_DLL}\" \"$INSTDIR\\${PLURYPRINT_SHELLEXT_DLL}\"",
        "\\InprocServer32\" \"\" \"$INSTDIR\\${PLURYPRINT_SHELLEXT_DLL}\"",
        "\"ThreadingModel\" \"Apartment\"",
        "\"ExplorerCommandHandler\" \"${PLURYPRINT_SHELLEXT_CLSID}\"",
        "\"MultiSelectModel\" \"Player\"",
        "PLURYPRINT_REGISTER_VERB \"*\"",
        "PLURYPRINT_REGISTER_VERB \"Directory\"",
        "!error",
    ] {
        assert!(nsh.contains(needle), "missing {needle}");
    }
    // HKCU only: nothing that needs elevation (NFR-020). Only writes count:
    // the MaeclyPrint takeover (ADR-0009) reads HKLM to find a per-machine
    // MSI, which msiexec then removes with its own elevation prompt, and
    // deletes a legacy `ContextMenuHandlers` key an old README told
    // developers to create.
    let writes = nsh
        .lines()
        .map(str::trim_start)
        .filter(|l| l.starts_with("WriteReg"));
    for line in writes {
        assert!(
            line.starts_with("WriteRegStr HKCU ") || line.starts_with("WriteRegDWORD HKCU "),
            "per-user installs write HKCU only: {line}"
        );
        assert!(
            !line.contains("ContextMenuHandlers"),
            "the DLL implements IExplorerCommand, not IContextMenu: {line}"
        );
    }
    for line in nsh.lines().filter(|l| !l.trim_start().starts_with(';')) {
        assert!(
            !line.contains("HKCR") && !line.contains("SHCTX"),
            "per-user installs write HKCU only: {line}"
        );
        if line.contains("HKLM") {
            let op = line.trim_start();
            assert!(
                op.starts_with("EnumRegKey ") || op.starts_with("ReadRegStr "),
                "HKLM is only read: {line}"
            );
        }
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
        "Software\\Classes\\*\\shell\\${PLURYPRINT_SHELLEXT_VERB}",
        "Software\\Classes\\Directory\\shell\\${PLURYPRINT_SHELLEXT_VERB}",
        "Software\\Classes\\CLSID\\${PLURYPRINT_SHELLEXT_CLSID}",
    ] {
        assert!(
            un.contains(&format!("DeleteRegKey HKCU \"{key}\"")),
            "uninstall must delete {key}"
        );
    }
    assert!(un.contains("Delete /REBOOTOK \"$INSTDIR\\${PLURYPRINT_SHELLEXT_DLL}\""));

    // SumatraPDF writes SumatraPDF-settings.txt next to itself when it
    // prints; Tauri's uninstaller only removes what it installed, so without
    // this the install folder stayed behind.
    let post = nsh
        .split("!macro NSIS_HOOK_POSTUNINSTALL")
        .nth(1)
        .expect("post-uninstall hook");
    let settings = post
        .find("!insertmacro PLURY_REMOVE_SUMATRA_SETTINGS \"$INSTDIR\"")
        .expect("uninstall removes the SumatraPDF settings");
    let rmdir = post
        .find("RMDir /REBOOTOK \"$INSTDIR\"")
        .expect("uninstall removes the install folder");
    assert!(settings < rmdir, "the settings go before the folder");
}

/// D5 / plan L1: the DLL registered is the one for the operating system's
/// native architecture, not the installer's. Explorer on Windows on ARM is
/// a native ARM64 process and cannot load the x64 DLL the x64 installer would
/// otherwise pick when it runs there emulated.
#[test]
fn nfr_070_the_registered_dll_matches_the_native_architecture() {
    let nsh = read("src-tauri/windows/installer-hooks.nsh");
    let post = nsh
        .split("!macro NSIS_HOOK_POSTINSTALL")
        .nth(1)
        .and_then(|rest| rest.split("!macroend").next())
        .expect("post-install hook");
    let arch = post
        .find("${If} ${IsNativeARM64}")
        .expect("native-arch probe");
    let copy = post
        .find("CopyFiles /SILENT")
        .expect("the DLL is copied into place");
    let register = post
        .find("InprocServer32\" \"\" \"$INSTDIR\\${PLURYPRINT_SHELLEXT_DLL}\"")
        .expect("the copied DLL is what gets registered");
    assert!(arch < copy && copy < register);
    assert!(post.contains("StrCpy $R8 \"arm64\"") && post.contains("StrCpy $R8 \"x64\""));
    assert!(
        post.contains("Goto pluryprint_register_done"),
        "no DLL for this OS: nothing is registered (a CLSID pointing at nothing)"
    );
    assert!(post.contains("Push $R8") && post.contains("Pop $R8"));
    // Both architectures are looked for at build time; CI must carry both.
    for arch in ["x64", "arm64"] {
        assert!(nsh.contains(&format!(
            "!if /FileExists \"${{PLURYPRINT_SHELLEXT_RES}}\\{arch}\\${{PLURYPRINT_SHELLEXT_DLL}}\""
        )));
    }
    assert!(nsh.contains("!if \"$%CI%\" == \"true\""));
    // The bundle installs the resource folder those paths name.
    assert_eq!(
        conf()["bundle"]["resources"]["resources/shellext/"],
        "resources/shellext/"
    );
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
