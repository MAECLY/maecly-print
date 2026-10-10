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
        r"Software\Maecly\${PLURY_LEGACY_PRODUCT}",
        r"Software\Classes\*\shell\${PLURY_LEGACY_PRODUCT}",
        r"Software\Classes\Directory\shell\${PLURY_LEGACY_PRODUCT}",
    ] {
        assert!(nsh.contains(key), "hooks.nsh must clean up {key}");
    }
}
