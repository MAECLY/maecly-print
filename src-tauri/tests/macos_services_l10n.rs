//! FR-092 for the Finder integration (FR-001): every Services menu title
//! declared in `Info.plist` has a translation in each shipped
//! `ServicesMenu.strings`, and the bundle config actually copies those files
//! into `Contents/Resources/<lang>.lproj/`.

use std::path::Path;

fn read(rel: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(rel);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {rel}: {e}"))
}

/// The `NSMenuItem → default` titles declared in Info.plist.
fn menu_titles(plist: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = plist;
    while let Some(i) = rest.find("<key>NSMenuItem</key>") {
        rest = &rest[i..];
        let start = rest.find("<string>").expect("NSMenuItem has a default") + 8;
        let end = rest[start..].find("</string>").expect("closed string") + start;
        out.push(rest[start..end].to_string());
        rest = &rest[end..];
    }
    out
}

/// `"key" = "value";` pairs of a `.strings` file (comments ignored).
fn strings_entries(src: &str) -> Vec<(String, String)> {
    src.lines()
        .map(str::trim)
        .filter(|l| l.starts_with('"') && l.ends_with(';'))
        .filter_map(|l| {
            let parts: Vec<&str> = l.split('"').collect();
            // ["", key, " = ", value, ";"]
            (parts.len() == 5).then(|| (parts[1].to_string(), parts[3].to_string()))
        })
        .collect()
}

#[test]
fn fr_092_every_services_title_is_translated_and_bundled() {
    let titles = menu_titles(&read("Info.plist"));
    assert_eq!(
        titles.len(),
        2,
        "both Services entries are declared: {titles:?}"
    );

    let conf: serde_json::Value = serde_json::from_str(&read("tauri.conf.json")).unwrap();
    let files = &conf["bundle"]["macOS"]["files"];

    for lang in ["en", "es"] {
        let source = format!("macos/{lang}.lproj/ServicesMenu.strings");
        let entries = strings_entries(&read(&source));
        for title in &titles {
            let translated = entries
                .iter()
                .find(|(k, _)| k == title)
                .map(|(_, v)| v.clone())
                .unwrap_or_else(|| panic!("{source} lacks \"{title}\""));
            assert!(!translated.is_empty());
            if lang == "es" {
                assert_ne!(&translated, title, "{title} is not translated in {source}");
            }
        }
        assert_eq!(
            files[format!("Resources/{lang}.lproj/ServicesMenu.strings")],
            serde_json::Value::String(source.clone()),
            "tauri.conf.json bundles {source}"
        );
    }

    assert!(read("Info.plist").contains("<key>CFBundleLocalizations</key>"));
}
