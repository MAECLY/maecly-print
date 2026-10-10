//! ADR-0010: every bundle carries the licence, the third-party notices and the
//! licence texts those notices point at, not only the repository.

use std::path::Path;

fn manifest_dir() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

fn read(rel: &str) -> String {
    let path = manifest_dir().join(rel);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {rel}: {e}"))
}

/// `bundle.resources` as source → destination pairs.
fn resources() -> Vec<(String, String)> {
    let conf: serde_json::Value = serde_json::from_str(&read("tauri.conf.json")).unwrap();
    conf["bundle"]["resources"]
        .as_object()
        .expect("bundle.resources is a map, so each file lands where About and the docs say")
        .iter()
        .map(|(from, to)| (from.clone(), to.as_str().unwrap().to_owned()))
        .collect()
}

#[test]
fn bundle_ships_the_licence_the_notices_and_the_licence_texts() {
    let resources = resources();
    for (from, to) in [
        ("../LICENSE", "LICENSE"),
        ("../THIRD-PARTY-NOTICES.md", "THIRD-PARTY-NOTICES.md"),
        ("../LICENSES/*", "LICENSES/"),
    ] {
        assert!(
            resources.iter().any(|(f, t)| f == from && t == to),
            "bundle.resources must map {from} to {to}: {resources:?}"
        );
    }
    // SumatraPDF keeps the path the engine looks in (engine::deps::bundled_dirs).
    assert!(resources
        .iter()
        .any(|(f, t)| f == "resources/sumatrapdf/*" && t == "resources/sumatrapdf/"));
}

#[test]
fn the_licence_is_the_agpl_and_the_manifest_says_so() {
    assert!(read("../LICENSE").starts_with("                    GNU AFFERO GENERAL PUBLIC LICENSE"));
    assert_eq!(env!("CARGO_PKG_LICENSE"), "AGPL-3.0-only");
}

#[test]
fn every_shipped_licence_text_is_named_in_the_notices() {
    let notices = read("../THIRD-PARTY-NOTICES.md");
    let dir = manifest_dir().join("../LICENSES");
    let mut texts: Vec<String> = std::fs::read_dir(&dir)
        .expect("LICENSES/ exists")
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    texts.sort();
    assert_eq!(texts, ["Apache-2.0.txt", "GPL-3.0.txt", "OFL-1.1.txt"]);
    for text in &texts {
        assert!(
            notices.contains(&format!("`LICENSES/{text}`")),
            "THIRD-PARTY-NOTICES.md never points at LICENSES/{text}"
        );
    }
}
