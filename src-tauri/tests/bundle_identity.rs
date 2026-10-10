//! ADR-0009: the bundle carries the Pluryprint identity, and the Windows
//! installer takes over an install made under the former name, MaeclyPrint.

use std::path::Path;

fn read(rel: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(rel);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {rel}: {e}"))
}

fn conf() -> serde_json::Value {
    serde_json::from_str(&read("tauri.conf.json")).unwrap()
}

#[test]
fn bundle_identity_is_pluryprint() {
    let conf = conf();
    assert_eq!(conf["productName"], "Pluryprint");
    assert_eq!(conf["identifier"], "com.maecly.pluryprint");
    assert_eq!(conf["app"]["windows"][0]["title"], "Pluryprint");

    // The Finder service's port must be the bundle name.
    let plist = read("Info.plist");
    assert!(plist.contains("<string>Print with Pluryprint</string>"));
    assert!(plist.contains("<key>NSPortName</key>\n\t\t\t<string>Pluryprint</string>"));
}

#[test]
fn nsis_hooks_take_over_a_maeclyprint_install() {
    let conf = conf();
    let hooks = conf["bundle"]["windows"]["nsis"]["installerHooks"]
        .as_str()
        .expect("installerHooks is set");
    let nsh = read(hooks);

    assert!(nsh.contains("!macro NSIS_HOOK_PREINSTALL"));
    assert!(nsh.contains(r#"!define PLURY_LEGACY_PRODUCT "MaeclyPrint""#));
    for key in [
        r"Software\Microsoft\Windows\CurrentVersion\Uninstall\${PLURY_LEGACY_PRODUCT}",
        r"Software\${PLURY_LEGACY_PUBLISHER}\${PLURY_LEGACY_PRODUCT}",
        r"Software\Classes\*\shell\${PLURY_LEGACY_PRODUCT}",
        r"Software\Classes\Directory\shell\${PLURY_LEGACY_PRODUCT}",
    ] {
        assert!(nsh.contains(key), "hooks.nsh must clean up {key}");
    }
}

/// The NSIS lines that do something (comments and blank lines dropped).
fn nsis_code(nsh: &str) -> Vec<&str> {
    nsh.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with(';'))
        .collect()
}

/// `_?=` must get a bare folder, and only the unquoted default value of
/// `HKCU\Software\Maecly\MaeclyPrint` is one (`InstallLocation` is quoted).
#[test]
fn nsis_hook_runs_the_old_uninstaller_in_place_with_an_unquoted_folder() {
    let nsh = read("windows/installer-hooks.nsh");
    let code = nsis_code(&nsh);

    assert!(
        code.iter()
            .any(|l| l.starts_with(r#"ReadRegStr $R1 HKCU "${PLURY_LEGACY_PRODUCTKEY}" """#)),
        "the folder comes from the unquoted product key"
    );
    assert!(
        code.contains(&"!insertmacro PLURY_UNQUOTE $R1"),
        "InstallLocation is unquoted before use"
    );
    let execs: Vec<_> = code
        .iter()
        .filter(|l| l.starts_with("ExecWait") && l.contains("uninstall"))
        .collect();
    assert!(!execs.is_empty());
    for exec in execs {
        assert!(
            exec.contains("_?=$R1"),
            "without _?= the uninstaller returns at once: {exec}"
        );
    }
}

/// A failed uninstall keeps the old registration, so the install can still be
/// removed from "Apps & features".
#[test]
fn nsis_hook_keeps_the_registration_when_the_old_uninstaller_fails() {
    let nsh = read("windows/installer-hooks.nsh");
    let code = nsis_code(&nsh);
    let gate = code
        .iter()
        .position(|l| *l == "${If} $R3 = 1")
        .expect("the cleanup is gated on $R3");
    let delete = code
        .iter()
        .position(|l| l.starts_with(r#"DeleteRegKey HKCU "${PLURY_LEGACY_UNINSTKEY}""#))
        .expect("the Uninstall key is deleted");
    assert!(
        gate < delete,
        "the Uninstall key is deleted only after the gate"
    );
    assert!(code.contains(&"StrCpy $R3 0"), "a failure clears the gate");
}

/// The old uninstaller runs in place, so it is left behind, and Windows keeps
/// its image locked for a moment after `ExecWait` returns: a single `Delete`
/// failed on Windows 11 and left `%LOCALAPPDATA%\MaeclyPrint\uninstall.exe`
/// on disk. The hook retries, then falls back to deleting it at restart.
#[test]
fn nsis_hook_retries_deleting_the_in_place_uninstaller() {
    let nsh = read("windows/installer-hooks.nsh");
    let code = nsis_code(&nsh);
    let pos = |needle: &str| {
        code.iter()
            .position(|l| *l == needle)
            .unwrap_or_else(|| panic!("missing `{needle}`"))
    };
    let exec = code
        .iter()
        .position(|l| l.starts_with("ExecWait") && l.contains("_?=$R1"))
        .expect("the old uninstaller runs in place");
    let retry = pos("plury_unexe_retry:");
    let delete = pos(r#"Delete "$R1\uninstall.exe""#);
    let check = pos("IfErrors 0 plury_unexe_done");
    let back = pos("Goto plury_unexe_retry");
    let fallback = pos(r#"Delete /REBOOTOK "$R1\uninstall.exe""#);
    let done = pos("plury_unexe_done:");
    assert!(exec < retry && retry < delete && delete < check && check < back);
    assert!(
        code[retry..delete].contains(&"ClearErrors"),
        "each try starts with a clear error flag"
    );
    assert!(
        code[check..back].iter().any(|l| l.starts_with("Sleep ")),
        "the retry waits between tries"
    );
    assert!(
        code[check..back]
            .iter()
            .any(|l| l.starts_with("IntCmp $R4 ") && l.contains("plury_unexe_late")),
        "the retry is bounded"
    );
    assert!(back < fallback && fallback < done);
    let rmdir = pos(r#"RMDir /REBOOTOK "$R1""#);
    assert!(done < rmdir, "the folder goes once the uninstaller is gone");
}

/// SumatraPDF writes its settings next to itself, so the old install folder
/// keeps `resources\sumatrapdf\SumatraPDF-settings.txt` after its uninstaller
/// ran, and with it the folder. The takeover removes it before the folder.
#[test]
fn nsis_hook_removes_sumatra_settings_from_the_old_install_folder() {
    let nsh = read("windows/installer-hooks.nsh");
    let code = nsis_code(&nsh);
    let take = code
        .iter()
        .position(|l| *l == r#"!insertmacro PLURY_REMOVE_SUMATRA_SETTINGS "$R1""#)
        .expect("the takeover removes the SumatraPDF settings");
    let rmdir = code
        .iter()
        .position(|l| *l == r#"RMDir /REBOOTOK "$R1""#)
        .expect("the old folder is removed");
    assert!(take < rmdir);
    let body = nsh
        .split("!macro PLURY_REMOVE_SUMATRA_SETTINGS DIR")
        .nth(1)
        .and_then(|rest| rest.split("!macroend").next())
        .expect("macro body");
    let body = nsis_code(body);
    assert_eq!(
        body,
        [
            r#"Delete "${DIR}\resources\sumatrapdf\SumatraPDF-settings.txt""#,
            r#"RMDir "${DIR}\resources\sumatrapdf""#,
            r#"RMDir "${DIR}\resources""#,
        ],
        "only that file, and only empty folders"
    );
    // The folder the bundle puts SumatraPDF in is the one the macro cleans.
    let resources = &conf()["bundle"]["resources"];
    assert_eq!(resources["resources/sumatrapdf/*"], "resources/sumatrapdf/");
}

/// Explorer keeps the old context-menu DLL loaded once its menu was shown, so
/// the old uninstaller cannot delete it and the old folder stayed behind with
/// it. The takeover moves it next to the new DLL under the `.old<N>` names
/// that the next update and the uninstaller already clean up.
#[test]
fn nsis_hook_parks_a_loaded_legacy_dll_where_it_gets_cleaned_up() {
    let nsh = read("windows/installer-hooks.nsh");
    let code = nsis_code(&nsh);
    let park = code
        .iter()
        .position(|l| *l == r#"!insertmacro PLURY_PARK_LEGACY_DLL "$R1""#)
        .expect("the takeover parks the old DLL");
    let ran = code
        .iter()
        .position(|l| *l == "plury_unexe_done:")
        .expect("after the old uninstaller ran");
    let rmdir = code
        .iter()
        .position(|l| *l == r#"RMDir /REBOOTOK "$R1""#)
        .expect("the old folder is removed");
    assert!(ran < park && park < rmdir);

    let body = nsh
        .split("!macro PLURY_PARK_LEGACY_DLL DIR")
        .nth(1)
        .and_then(|rest| rest.split("!macroend").next())
        .expect("macro body");
    let body = nsis_code(body);
    let rename = body
        .iter()
        .find(|l| l.starts_with("Rename "))
        .expect("the DLL is moved, which works while it is loaded");
    assert_eq!(
        *rename,
        r#"Rename "${DIR}\${PLURYPRINT_SHELLEXT_DLL}" "$INSTDIR\${PLURYPRINT_SHELLEXT_DLL}.old$R5""#
    );
    assert!(
        body.iter()
            .any(|l| l.starts_with("IntCmp $R5 ") && l.contains("plury_park_late")),
        "the search for a free name is bounded"
    );
    assert!(
        body.iter()
            .any(|l| l.starts_with("IfFileExists \"$INSTDIR\\${PLURYPRINT_SHELLEXT_DLL}.old$R5\"")),
        "a parked DLL from an earlier run is never overwritten"
    );
    // The `.old<N>` files are the ones the existing cleanup deletes.
    let cleanup = r#"Delete "$INSTDIR\${PLURYPRINT_SHELLEXT_DLL}.old*""#;
    let un_cleanup = r#"Delete /REBOOTOK "$INSTDIR\${PLURYPRINT_SHELLEXT_DLL}.old*""#;
    assert!(code.contains(&cleanup), "the next install clears them");
    assert!(code.contains(&un_cleanup), "the uninstaller clears them");
    // $R5 is saved and restored around the hook.
    assert!(code.contains(&"Push $R5") && code.contains(&"Pop $R5"));
}

/// A per-machine MSI install from before c1466b4 is found by name and offered
/// for removal (msiexec asks for elevation itself). msiexec is run by its
/// full system path: a bare `msiexec.exe` is looked up in the setup's own
/// folder (Downloads, as a rule) first.
#[test]
fn nsis_hook_offers_to_remove_a_per_machine_msi_install() {
    let nsh = read("windows/installer-hooks.nsh");
    let code = nsis_code(&nsh);
    assert!(code
        .iter()
        .any(|l| l.starts_with(r#"EnumRegKey $R5 HKLM "${PLURY_UNINSTROOT}""#)));
    assert!(code
        .iter()
        .any(|l| l.starts_with(r#"ExecWait '"$SYSDIR\msiexec.exe" /x $R5"#)));
    assert!(
        code.iter()
            .any(|l| l.starts_with("MessageBox") && l.contains("/SD IDNO")),
        "a silent run never triggers a UAC prompt"
    );
}

/// Compile the hooks file in a stub script with the includes Tauri's template
/// makes before it, every hook in its section, warnings as errors. Skipped
/// where `makensis` is missing. The file refuses to compile without the
/// context-menu DLLs it ships from `resources/shellext/<arch>/`, so it is
/// copied into a temporary tree with placeholder DLLs. The two variables the
/// post-uninstall hook reads are declared the way Tauri's template does.
#[test]
fn nsis_hook_compiles() {
    let Ok(out) = std::process::Command::new("makensis")
        .arg("-VERSION")
        .output()
    else {
        eprintln!("makensis not installed; skipping");
        return;
    };
    assert!(out.status.success());
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("windows")).unwrap();
    let hooks = dir.path().join("windows").join("installer-hooks.nsh");
    std::fs::write(&hooks, read("windows/installer-hooks.nsh")).unwrap();
    // The hooks include their EN/ES messages from next to themselves.
    std::fs::write(
        dir.path().join("windows").join("installer-strings.nsh"),
        read("windows/installer-strings.nsh"),
    )
    .unwrap();
    for arch in ["x64", "arm64"] {
        let res = dir.path().join("resources").join("shellext").join(arch);
        std::fs::create_dir_all(&res).unwrap();
        std::fs::write(res.join("shell_ext_win.dll"), b"placeholder").unwrap();
    }
    let script = dir.path().join("stub.nsi");
    std::fs::write(
        &script,
        format!(
            "Unicode true\n!include LogicLib.nsh\n!include FileFunc.nsh\n!include x64.nsh\n\
             !include StrFunc.nsh\n${{StrLoc}}\nVar UpdateMode\nVar DeleteAppDataCheckboxState\n!include \"{}\"\nOutFile \"{}\"\n\
             RequestExecutionLevel user\nInstallDir \"$LOCALAPPDATA\\Stub\"\n\
             Section Install\n!insertmacro NSIS_HOOK_PREINSTALL\n\
             !insertmacro NSIS_HOOK_POSTINSTALL\nWriteUninstaller \"$INSTDIR\\uninstall.exe\"\n\
             SectionEnd\nSection Uninstall\nStrCpy $UpdateMode 0\nStrCpy $DeleteAppDataCheckboxState 0\n\
             !insertmacro NSIS_HOOK_PREUNINSTALL\n\
             !insertmacro NSIS_HOOK_POSTUNINSTALL\nSectionEnd\n",
            hooks.display(),
            dir.path().join("stub.exe").display()
        ),
    )
    .unwrap();
    let out = std::process::Command::new("makensis")
        .args(["-WX", "-V2"])
        .arg(&script)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}
