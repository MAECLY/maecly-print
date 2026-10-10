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
    // Explorer may still hold the DLL: parked and deleted at the next logon.
    assert!(un
        .contains("!insertmacro PLURY_REMOVE_OR_PARK \"$INSTDIR\" \"${PLURYPRINT_SHELLEXT_DLL}\""));

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
        .find("RMDir \"$INSTDIR\"")
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

/// The uninstaller's "delete the application data" box removes the real data
/// folders (`ProjectDirs::from("com", "Maecly", "Pluryprint")`), not only the
/// `%APPDATA%\<identifier>` Tauri knows about. Never on an update.
#[test]
fn fr_083_delete_app_data_removes_the_real_data_folders() {
    let nsh = read("src-tauri/windows/installer-hooks.nsh");
    let paths = read("src-tauri/src/util/paths.rs");
    assert!(paths.contains("const ORG: &str = \"Maecly\";"));
    assert!(paths.contains("const APP: &str = \"Pluryprint\";"));
    assert!(nsh.contains("!define PLURYPRINT_DATA_ORG \"Maecly\""));
    assert!(nsh.contains("!define PLURYPRINT_DATA_APP \"Pluryprint\""));

    let post = nsh
        .split("!macro NSIS_HOOK_POSTUNINSTALL")
        .nth(1)
        .and_then(|rest| rest.split("!macroend").next())
        .expect("post-uninstall hook");
    let guard = post
        .find("${If} $DeleteAppDataCheckboxState = 1")
        .expect("only when the user ticked the box");
    let update = post
        .find("${AndIf} $UpdateMode <> 1")
        .expect("never on an update");
    for dir in [
        r"RMDir /r $APPDATA\${PLURYPRINT_DATA_ORG}\${PLURYPRINT_DATA_APP}",
        r"RMDir /r $LOCALAPPDATA\${PLURYPRINT_DATA_ORG}\${PLURYPRINT_DATA_APP}",
        r"RMDir /r $LOCALAPPDATA\${PLURY_LEGACY_IDENTIFIER}",
    ] {
        let dir = dir.replace("/r $", "/r \"$") + "\"";
        let at = post.find(&dir).unwrap_or_else(|| panic!("missing {dir}"));
        assert!(guard < at && update < at, "{dir} must be inside the guard");
    }
    // The shared organization folder goes only when empty (no /r).
    assert!(post.contains(r#"RMDir "$APPDATA\${PLURYPRINT_DATA_ORG}""#));
    assert!(post.contains(r#"RMDir "$LOCALAPPDATA\${PLURYPRINT_DATA_ORG}""#));
}

/// The takeover deletes MaeclyPrint's web view profile, which nothing reads
/// any more, but only when no MaeclyPrint install is left to use it.
#[test]
fn adr_0009_takeover_removes_the_stale_web_view_profile_when_safe() {
    let nsh = read("src-tauri/windows/installer-hooks.nsh");
    assert!(nsh.contains("!define PLURY_LEGACY_IDENTIFIER \"com.maecly.print\""));
    assert_ne!(
        conf()["identifier"],
        "com.maecly.print",
        "never delete the current identifier's profile"
    );
    let pre = nsh
        .split("!macro NSIS_HOOK_PREINSTALL")
        .nth(1)
        .and_then(|rest| rest.split("!macroend").next())
        .expect("pre-install hook");
    let guard = pre
        .find("${If} $R6 = 0")
        .expect("guarded by 'nothing left'");
    let rm = pre
        .find(r#"RMDir /r "$LOCALAPPDATA\${PLURY_LEGACY_IDENTIFIER}""#)
        .expect("the profile is removed");
    assert!(guard < rm);
    // Every path that leaves an old install in place sets the flag.
    assert!(
        pre.matches("StrCpy $R6 1").count() >= 5,
        "per-user kept (2 ways), non-MSI, MSI declined and MSI failed"
    );
    assert!(pre.contains("Push $R6") && pre.contains("Pop $R6"));
}

#[test]
fn nfr_070_installer_scripts_are_ascii() {
    for rel in [
        "src-tauri/windows/installer-hooks.nsh",
        "scripts/build-shell-ext.ps1",
        "scripts/check-failed-logons.ps1",
        "scripts/vm/r3-contextmenu-e2e.ps1",
    ] {
        let text = read(rel);
        let bad: Vec<char> = text.chars().filter(|c| !c.is_ascii()).collect();
        assert!(bad.is_empty(), "{rel} has non-ASCII characters: {bad:?}");
    }
}

/// The post-uninstall hook with comments and blank lines dropped.
fn post_uninstall_code(nsh: &str) -> Vec<String> {
    nsh.split("!macro NSIS_HOOK_POSTUNINSTALL")
        .nth(1)
        .and_then(|rest| rest.split("!macroend").next())
        .expect("post-uninstall hook")
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with(';'))
        .map(str::to_owned)
        .collect()
}

/// A real uninstall leaves nothing of the installer's own behind, ticked box
/// or not: Tauri's `HKCU\Software\<publisher>\<product>` key (the install path
/// and "Installer Language") and the main window's WebView2 profile,
/// `%LOCALAPPDATA%\<identifier>\EBWebView`, both found after `uninstall.exe
/// /S` on the 0.0.1 build. An update must keep both: `tauri-plugin-updater`
/// runs the setup with `/UPDATE`, and the template passes it on to the
/// uninstaller, which sets `$UpdateMode` to 1.
#[test]
fn fr_083_a_real_uninstall_removes_the_product_key_and_web_view_profile() {
    let nsh = read("src-tauri/windows/installer-hooks.nsh");
    let code = post_uninstall_code(&nsh);
    let pos = |needle: &str| {
        code.iter()
            .position(|l| l == needle)
            .unwrap_or_else(|| panic!("missing `{needle}`"))
    };
    let names = pos("!insertmacro PLURY_REQUIRE_TAURI_NAMES");
    let context = pos("SetShellVarContext current");
    let guard = pos("${If} $UpdateMode <> 1");
    let key = pos(r#"DeleteRegKey HKCU "${MANUPRODUCTKEY}""#);
    let publisher = pos(r#"DeleteRegKey /ifempty HKCU "${MANUKEY}""#);
    let profile = pos(r#"RMDir /r "$LOCALAPPDATA\${BUNDLEID}\EBWebView""#);
    let folder = pos(r#"RMDir "$LOCALAPPDATA\${BUNDLEID}""#);
    let end = code[guard..]
        .iter()
        .position(|l| l == "${EndIf}")
        .map(|i| guard + i)
        .expect("guard closes");
    assert!(names < guard && context < guard);
    for at in [key, publisher, profile, folder] {
        assert!(
            guard < at && at < end,
            "only outside an update: {}",
            code[at]
        );
    }
    assert!(
        profile < folder,
        "the identifier folder goes once it is empty"
    );
    // Not tied to "Delete the application data": a silent uninstall never
    // ticks it, and this is not user data.
    let app_data = pos("${If} $DeleteAppDataCheckboxState = 1");
    assert!(end < app_data);
    assert!(
        !code[guard..end].iter().any(|l| l.contains("Checkbox")),
        "the cleanup runs whether or not the box is ticked"
    );
    // Only the web view profile: never the whole identifier folder
    // recursively, and the publisher key only when empty.
    assert!(!code
        .iter()
        .any(|l| l == r#"RMDir /r "$LOCALAPPDATA\${BUNDLEID}""#));
    assert!(!code
        .iter()
        .any(|l| l == r#"DeleteRegKey HKCU "${MANUKEY}""#));
    // The profile folder is named after the bundle identifier.
    assert_eq!(conf()["identifier"], "com.maecly.pluryprint");
}

/// The uninstall entry carries a silent uninstall command (winget and
/// "Apps & features" tooling read it) and the product's home page. Tauri
/// writes `URLInfoAbout`, `URLUpdateInfo` and `HelpLink` from
/// `bundle.homepage`; `QuietUninstallString` only the hook writes.
#[test]
fn fr_083_the_uninstall_entry_has_a_quiet_command_and_a_home_page() {
    assert_eq!(
        conf()["bundle"]["homepage"],
        "https://pluryprint.maecly.com"
    );
    let nsh = read("src-tauri/windows/installer-hooks.nsh");
    let post = nsh
        .split("!macro NSIS_HOOK_POSTINSTALL")
        .nth(1)
        .and_then(|rest| rest.split("!macroend").next())
        .expect("post-install hook");
    let names = post
        .find("!insertmacro PLURY_REQUIRE_TAURI_NAMES")
        .expect("the template's names are required");
    let quiet = post
        .find(r#"WriteRegStr HKCU "${UNINSTKEY}" "QuietUninstallString" '"$INSTDIR\uninstall.exe" /S'"#)
        .expect("QuietUninstallString");
    assert!(names < quiet);
    // The guard: every name the hooks borrow from the template.
    let body = nsh
        .split("!macro PLURY_REQUIRE_TAURI_NAMES")
        .nth(1)
        .and_then(|rest| rest.split("!macroend").next())
        .expect("PLURY_REQUIRE_TAURI_NAMES");
    assert!(body.contains("!ifndef UNINSTKEY | MANUKEY | MANUPRODUCTKEY | BUNDLEID"));
    assert!(body.contains("!error "));
}

/// The setup's version resource names the company. Tauri's template sets no
/// `CompanyName`, and it defines `MANUFACTURER` only after it includes the
/// hooks, so the hooks spell the publisher out; it must stay
/// `bundle.publisher`. It follows the language strings so it lands in the
/// same version block as the template's keys (see the hooks file).
#[test]
fn fr_083_the_setup_names_its_company() {
    let nsh = read("src-tauri/windows/installer-hooks.nsh");
    let publisher = conf()["bundle"]["publisher"]
        .as_str()
        .expect("bundle.publisher")
        .to_owned();
    assert!(nsh.contains(&format!("!define PLURYPRINT_COMPANY \"{publisher}\"")));
    let strings = nsh
        .find(r#"!include "${__FILEDIR__}\installer-strings.nsh""#)
        .expect("language strings");
    let company = nsh
        .find(r#"VIAddVersionKey "CompanyName" "${PLURYPRINT_COMPANY}""#)
        .expect("CompanyName");
    assert!(strings < company);
    assert!(
        !nsh[company..].contains("LangString"),
        "no language string may follow the version key"
    );
}
