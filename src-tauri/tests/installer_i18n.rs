//! FR-093 — the Windows installer speaks English and Spanish.
//!
//! Tauri translates its own NSIS pages for every language listed in
//! `bundle.windows.nsis.languages`; the lines our hooks add (the takeover of
//! a MaeclyPrint install: one question and the "Show details" log) come from
//! `src-tauri/windows/installer-strings.nsh`. Nothing here runs an installer,
//! so the contract is pinned on the sources:
//!
//! - the installer lists English and Spanish;
//! - every message the hooks show (`MessageBox`, `DetailPrint`) is a
//!   `$(PLURY_*)` language string, never a literal;
//! - every `$(PLURY_*)` the hooks use is declared for English (1033) and
//!   Spanish (1034), with the same `$R*` variables in both, and nothing is
//!   declared that the hooks no longer use;
//! - the files stay ASCII (NSIS reads a BOM-less include in the build
//!   machine's code page): Spanish letters are `${U+XXXX}` code points.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

fn read(rel: &str) -> String {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join(rel);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

const HOOKS: &str = "src-tauri/windows/installer-hooks.nsh";
const STRINGS: &str = "src-tauri/windows/installer-strings.nsh";
const EN: &str = "${PLURY_LANG_EN}";
const ES: &str = "${PLURY_LANG_ES}";

/// Code lines only (NSIS comments start with `;` or `#`).
fn code_lines(src: &str) -> impl Iterator<Item = &str> {
    src.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with(';') && !l.starts_with('#'))
}

/// Every `$(NAME)` reference in `src`.
fn refs(src: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for line in code_lines(src) {
        let mut rest = line;
        while let Some(i) = rest.find("$(") {
            rest = &rest[i + 2..];
            if let Some(j) = rest.find(')') {
                out.insert(rest[..j].to_string());
                rest = &rest[j..];
            }
        }
    }
    out
}

/// `LangString NAME LANG "text"` declarations: name → (lang → text).
fn lang_strings() -> BTreeMap<String, BTreeMap<String, String>> {
    let src = read(STRINGS);
    let mut out: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
    for line in code_lines(&src).filter(|l| l.starts_with("LangString ")) {
        let mut parts = line.splitn(4, ' ');
        let (_, name, lang, text) = (
            parts.next(),
            parts.next().expect("name"),
            parts.next().expect("language"),
            parts.next().expect("text"),
        );
        let text = text.trim_matches('"').to_string();
        out.entry(name.to_string())
            .or_default()
            .insert(lang.to_string(), text);
    }
    out
}

/// The `$R0`…`$R9` / `$0`…`$9` variables a message expands, in order.
fn variables(text: &str) -> Vec<String> {
    let b = text.as_bytes();
    let mut out = Vec::new();
    for i in 0..b.len() {
        if b[i] != b'$' {
            continue;
        }
        if b.get(i + 1) == Some(&b'R') && b.get(i + 2).is_some_and(u8::is_ascii_digit) {
            out.push(text[i..i + 3].to_string());
        } else if b.get(i + 1).is_some_and(u8::is_ascii_digit) {
            out.push(text[i..i + 2].to_string());
        }
    }
    out.sort();
    out
}

#[test]
fn fr_093_nsis_language_es() {
    let conf: serde_json::Value = serde_json::from_str(&read("src-tauri/tauri.conf.json")).unwrap();
    let langs: Vec<&str> = conf["bundle"]["windows"]["nsis"]["languages"]
        .as_array()
        .expect("bundle.windows.nsis.languages is set")
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert!(langs.contains(&"English"), "{langs:?}");
    assert!(langs.contains(&"Spanish"), "{langs:?}");

    let strings = read(STRINGS);
    assert!(strings.contains("!define PLURY_LANG_EN 1033"));
    assert!(strings.contains("!define PLURY_LANG_ES 1034"));
    assert!(
        read(HOOKS).contains("installer-strings.nsh"),
        "the hooks include their strings"
    );
}

#[test]
fn fr_093_every_hook_message_is_a_language_string() {
    let hooks = read(HOOKS);
    for line in code_lines(&hooks) {
        let shows = line.starts_with("DetailPrint ") || line.starts_with("MessageBox ");
        if !shows {
            continue;
        }
        // The first quoted argument is what the user reads.
        let text = line
            .split('"')
            .nth(1)
            .unwrap_or_else(|| panic!("no message in: {line}"));
        assert!(
            text.starts_with("$(PLURY_"),
            "a hook message is not translatable: {line}"
        );
    }
}

#[test]
fn fr_093_every_language_string_has_english_and_spanish() {
    let declared = lang_strings();
    let used = refs(&read(HOOKS));
    assert!(!used.is_empty());
    for name in &used {
        let langs = declared
            .get(name)
            .unwrap_or_else(|| panic!("$({name}) is used but never declared"));
        let en = langs
            .get(EN)
            .unwrap_or_else(|| panic!("{name}: no English"));
        let es = langs
            .get(ES)
            .unwrap_or_else(|| panic!("{name}: no Spanish"));
        assert!(!en.trim().is_empty() && !es.trim().is_empty(), "{name}");
        assert_ne!(
            en, es,
            "{name}: the Spanish line is a copy of the English one"
        );
        assert_eq!(variables(en), variables(es), "{name}: different variables");
    }
    for name in declared.keys() {
        assert!(used.contains(name), "{name} is declared but unused");
    }
}

#[test]
fn fr_093_installer_sources_stay_ascii() {
    for rel in [HOOKS, STRINGS] {
        let src = read(rel);
        if let Some((n, line)) = src.lines().enumerate().find(|(_, l)| !l.is_ascii()) {
            panic!("{rel}:{}: not ASCII: {line}", n + 1);
        }
    }
}
