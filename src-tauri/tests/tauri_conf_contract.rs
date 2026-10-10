//! Round-3 contract on `tauri.conf.json` and the main capability (lane L0).
//!
//! - one version source: Cargo.toml (`tauri.conf.json` has no `version`, so
//!   the updater and `CARGO_PKG_VERSION` can never disagree);
//! - no Linux bundles (decision D20: Linux is out of scope);
//! - the NSIS installer speaks English and Spanish (FR-093);
//! - the Explorer extension DLLs ship under `resources/shellext/<arch>/`
//!   (directory form, so the per-architecture folders survive bundling);
//! - IPC is granted to the main window only: the render worker's web view has
//!   none.

use std::path::PathBuf;

fn read(rel: &str) -> String {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(rel);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

fn conf() -> serde_json::Value {
    serde_json::from_str(&read("tauri.conf.json")).expect("tauri.conf.json parses")
}

#[test]
fn release_version_has_a_single_source() {
    assert!(
        conf().get("version").is_none(),
        "tauri.conf.json must not pin a version: Cargo.toml is the only source"
    );
}

#[test]
fn nfr_070_no_linux_bundle_targets() {
    let conf = conf();
    let targets: Vec<&str> = conf["bundle"]["targets"]
        .as_array()
        .expect("explicit targets")
        .iter()
        .map(|t| t.as_str().unwrap())
        .collect();
    for linux in ["deb", "rpm", "appimage"] {
        assert!(!targets.contains(&linux), "{linux} in {targets:?}");
    }
    for wanted in ["nsis", "app", "dmg"] {
        assert!(targets.contains(&wanted), "{wanted} missing: {targets:?}");
    }
}

#[test]
fn fr_093_nsis_installer_is_english_and_spanish() {
    let conf = conf();
    let langs = &conf["bundle"]["windows"]["nsis"]["languages"];
    assert_eq!(langs, &serde_json::json!(["English", "Spanish"]));
}

#[test]
fn fr_001_shell_extension_dlls_ship_per_architecture() {
    let conf = conf();
    assert_eq!(
        conf["bundle"]["resources"]["resources/shellext/"], "resources/shellext/",
        "directory form keeps resources/shellext/<arch>/ intact"
    );
    // The folder exists in a fresh checkout, so a build without the DLLs
    // (every non-Windows build) still finds the resource.
    assert!(PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("resources/shellext/README.md")
        .is_file());
}

#[test]
fn nfr_render_worker_gets_no_ipc() {
    let cap: serde_json::Value =
        serde_json::from_str(&read("capabilities/main.json")).expect("capability parses");
    assert_eq!(cap["windows"], serde_json::json!(["main"]));
}
