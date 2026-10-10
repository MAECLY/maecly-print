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
/// on disk. The hook retries, then has it deleted at the next logon.
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
    let fallback = pos(r#"!insertmacro PLURY_DELETE_AT_LOGON "$R1\uninstall.exe" "$R1""#);
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
    let rmdir = pos(r#"RMDir "$R1""#);
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
        .position(|l| *l == r#"RMDir "$R1""#)
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
/// it. The takeover parks it with the same macro the uninstaller uses, which
/// works unelevated (PLURY_REMOVE_OR_PARK, checked below).
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
        .position(|l| *l == r#"RMDir "$R1""#)
        .expect("the old folder is removed");
    assert!(ran < park && park < rmdir);

    let body = nsh
        .split("!macro PLURY_PARK_LEGACY_DLL DIR")
        .nth(1)
        .and_then(|rest| rest.split("!macroend").next())
        .expect("macro body");
    assert_eq!(
        nsis_code(body),
        [r#"!insertmacro PLURY_REMOVE_OR_PARK "${DIR}" "${PLURYPRINT_SHELLEXT_DLL}""#]
    );
}

/// NFR-020: a per-user setup cannot schedule a delete for the next restart
/// (`Delete /REBOOTOK` writes HKLM `PendingFileRenameOperations`), so the DLL
/// Explorer holds is renamed away (which works while it is loaded) and an
/// HKCU `RunOnce` value deletes it at the next logon.
#[test]
fn nfr_020_a_loaded_dll_is_parked_and_deleted_at_the_next_logon() {
    let nsh = read("windows/installer-hooks.nsh");
    let code = nsis_code(&nsh);
    assert!(
        !code.iter().any(|l| l.contains("/REBOOTOK")),
        "nothing may rely on /REBOOTOK in a per-user setup"
    );
    let body = nsh
        .split("!macro PLURY_REMOVE_OR_PARK DIR NAME")
        .nth(1)
        .and_then(|rest| rest.split("!macroend").next())
        .expect("macro body");
    let body = nsis_code(body);
    let pos = |needle: &str| {
        body.iter()
            .position(|l| *l == needle)
            .unwrap_or_else(|| panic!("missing `{needle}`"))
    };
    let delete = pos(r#"Delete "${DIR}\${NAME}""#);
    let to_temp = pos(r#"Rename "${DIR}\${NAME}" "$TEMP\pluryprint-shellext-$R9.dll""#);
    let in_place = pos(r#"Rename "${DIR}\${NAME}" "${DIR}\${NAME}.old$R9""#);
    let runonce = pos(r#"WriteRegStr HKCU "${PLURY_RUNONCE_KEY}" "Pluryprint cleanup $R7" $R8"#);
    assert!(delete < to_temp && to_temp < in_place && in_place < runonce);
    // A parked file from an earlier run is never overwritten, and the search
    // for a free name is bounded.
    for (free, rename) in [
        (
            r#"${IfNot} ${FileExists} "$TEMP\pluryprint-shellext-$R9.dll""#,
            to_temp,
        ),
        (
            r#"${IfNot} ${FileExists} "${DIR}\${NAME}.old$R9""#,
            in_place,
        ),
    ] {
        assert!(pos(free) < rename, "{free}");
    }
    assert_eq!(body.iter().filter(|l| **l == "${For} $R9 1 99").count(), 2);
    // The logon command: the system cmd.exe by full path, a plain delete,
    // and an rmdir (empty folders only) when the copy stayed in DIR.
    assert!(body.contains(&r#"StrCpy $R8 '"$SYSDIR\cmd.exe" /d /c del /f /q "$R7"'"#));
    assert!(
        body.contains(&r#"StrCpy $R8 '"$SYSDIR\cmd.exe" /d /c del /f /q "$R7" & rmdir "${DIR}"'"#)
    );
    assert!(!body
        .iter()
        .any(|l| l.contains("rmdir /s") || l.contains("RMDir /r")));
    assert!(code.contains(
        &r#"!define PLURY_RUNONCE_KEY "Software\Microsoft\Windows\CurrentVersion\RunOnce""#
    ));
    // The registers it uses are its own.
    for r in ["$R7", "$R8", "$R9"] {
        assert!(body.contains(&format!("Push {r}").as_str()), "{r}");
        assert!(body.contains(&format!("Pop {r}").as_str()), "{r}");
    }
    // Update, uninstall and takeover all go through it.
    for user in [
        r#"!insertmacro PLURY_REMOVE_OR_PARK "$INSTDIR" "${PLURYPRINT_SHELLEXT_DLL}""#,
        r#"!insertmacro PLURY_REMOVE_OR_PARK "${DIR}" "${PLURYPRINT_SHELLEXT_DLL}""#,
    ] {
        assert!(code.contains(&user), "{user}");
    }
    // Copies parked next to the DLL by an older version are cleared by the
    // next install and by the uninstaller.
    let cleanup = r#"Delete "$INSTDIR\${PLURYPRINT_SHELLEXT_DLL}.old*""#;
    assert_eq!(code.iter().filter(|l| **l == cleanup).count(), 2);
}

/// The in-place legacy uninstaller that stays locked is deleted at the next
/// logon the same way, with its folder if that is then empty.
#[test]
fn nfr_020_delete_at_logon_is_per_user() {
    let nsh = read("windows/installer-hooks.nsh");
    let body = nsh
        .split("!macro PLURY_DELETE_AT_LOGON FILE DIR")
        .nth(1)
        .and_then(|rest| rest.split("!macroend").next())
        .expect("macro body");
    assert_eq!(
        nsis_code(body),
        [
            r#"WriteRegStr HKCU "${PLURY_RUNONCE_KEY}" "Pluryprint cleanup ${FILE}" '"$SYSDIR\cmd.exe" /d /c del /f /q "${FILE}" & rmdir "${DIR}"'"#
        ]
    );
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
/// post-uninstall hook reads are declared the way Tauri's template does, and
/// so are the MUI language-registry names (after the hooks, as there).
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
             !include StrFunc.nsh\n${{StrLoc}}\nVar UpdateMode\nVar DeleteAppDataCheckboxState\n!include \"{}\"\n\
             !define MUI_LANGDLL_REGISTRY_ROOT \"HKCU\"\n\
             !define MUI_LANGDLL_REGISTRY_KEY \"Software\\Maecly\\Pluryprint\"\n\
             !define MUI_LANGDLL_REGISTRY_VALUENAME \"Installer Language\"\nOutFile \"{}\"\n\
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
