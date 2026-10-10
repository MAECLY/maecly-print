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

/// A per-machine MSI install from before c1466b4 is found by name and offered
/// for removal (msiexec asks for elevation itself).
#[test]
fn nsis_hook_offers_to_remove_a_per_machine_msi_install() {
    let nsh = read("windows/installer-hooks.nsh");
    let code = nsis_code(&nsh);
    assert!(code
        .iter()
        .any(|l| l.starts_with(r#"EnumRegKey $R5 HKLM "${PLURY_UNINSTROOT}""#)));
    assert!(code
        .iter()
        .any(|l| l.starts_with("ExecWait 'msiexec.exe /x $R5")));
    assert!(
        code.iter()
            .any(|l| l.starts_with("MessageBox") && l.contains("/SD IDNO")),
        "a silent run never triggers a UAC prompt"
    );
}

/// Compile the hooks file in a stub script with the includes Tauri's template
/// makes before it, every hook in its section, warnings as errors. Skipped
/// where `makensis` is missing. The file refuses to compile without the
/// context-menu DLL next to it, so it is copied into a temporary folder with
/// a placeholder DLL.
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
    let hooks = dir.path().join("installer-hooks.nsh");
    std::fs::write(&hooks, read("windows/installer-hooks.nsh")).unwrap();
    std::fs::write(dir.path().join("shell_ext_win.dll"), b"placeholder").unwrap();
    let script = dir.path().join("stub.nsi");
    std::fs::write(
        &script,
        format!(
            "Unicode true\n!include LogicLib.nsh\n!include FileFunc.nsh\n!include x64.nsh\n\
             !include StrFunc.nsh\n${{StrLoc}}\n!include \"{}\"\nOutFile \"{}\"\n\
             RequestExecutionLevel user\nInstallDir \"$LOCALAPPDATA\\Stub\"\n\
             Section Install\n!insertmacro NSIS_HOOK_PREINSTALL\n\
             !insertmacro NSIS_HOOK_POSTINSTALL\nWriteUninstaller \"$INSTDIR\\uninstall.exe\"\n\
             SectionEnd\nSection Uninstall\n!insertmacro NSIS_HOOK_PREUNINSTALL\n\
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
