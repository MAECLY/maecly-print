//! FR-058 / ADR-0002 — the scripts that fetch the bundled SumatraPDF.
//!
//! The Windows installer ships SumatraPDF (GPLv3), fetched at packaging time
//! by `scripts/fetch-sumatrapdf.ps1` (and its POSIX twin). These tests guard
//! three things a reviewer would not see in a diff:
//!
//! - The PowerShell script stays pure ASCII. Windows PowerShell 5.1, the only
//!   PowerShell on a stock Windows 11, reads a BOM-less script in the ANSI code
//!   page; one em dash then broke the parser and the script exited 1 even with
//!   a valid pin.
//! - Both scripts pin the same release, the same archive checksums for x64
//!   and ARM64, the same source commit and the same licence checksums.
//! - Every pin is filled in, and the licence is fetched and verified rather
//!   than hoped for (the portable archives contain no licence file).

use std::path::PathBuf;

fn script(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../scripts")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// Every 64-hex-digit token in `text`, lower-cased, in order.
fn sha256s(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i].is_ascii_hexdigit() {
            let start = i;
            while i < bytes.len() && bytes[i].is_ascii_hexdigit() {
                i += 1;
            }
            if i - start == 64 {
                out.push(text[start..i].to_ascii_lowercase());
            }
        } else {
            i += 1;
        }
    }
    out
}

/// The 40-hex-digit commit pinned in `text`.
fn commit(text: &str, key: &str) -> String {
    let line = text
        .lines()
        .find(|l| l.trim_start().starts_with(key))
        .unwrap_or_else(|| panic!("no {key} line"));
    let hex: String = line
        .split(['\'', '"'])
        .find(|t| t.len() == 40 && t.chars().all(|c| c.is_ascii_hexdigit()))
        .unwrap_or_else(|| panic!("no commit in {line}"))
        .to_string();
    hex
}

#[test]
fn fr_058_fetch_script_parses_in_windows_powershell_5_1() {
    let ps1 = script("fetch-sumatrapdf.ps1");
    let bad: Vec<(usize, char)> = ps1
        .lines()
        .enumerate()
        .flat_map(|(n, l)| l.chars().filter(|c| !c.is_ascii()).map(move |c| (n + 1, c)))
        .collect();
    assert!(
        bad.is_empty(),
        "non-ASCII characters break Windows PowerShell 5.1 parsing: {bad:?}"
    );
}

#[test]
fn fr_058_both_fetch_scripts_pin_the_same_release_for_x64_and_arm64() {
    let ps1 = script("fetch-sumatrapdf.ps1");
    let sh = script("fetch-sumatrapdf.sh");

    let version = |text: &str| {
        text.lines()
            .find(|l| l.trim_start().starts_with("$VERSION =") || l.starts_with("VERSION="))
            .and_then(|l| l.split(['\'', '"']).nth(1))
            .map(str::to_string)
            .expect("a pinned VERSION")
    };
    assert_eq!(version(&ps1), version(&sh));

    // Two archives (x64, arm64) and three licence files, in the same order.
    let ps1_pins = sha256s(&ps1);
    let sh_pins = sha256s(&sh);
    assert_eq!(ps1_pins.len(), 5, "{ps1_pins:?}");
    assert_eq!(ps1_pins, sh_pins);

    assert_eq!(commit(&ps1, "$SOURCE_COMMIT"), commit(&sh, "SOURCE_COMMIT"));

    // Both architectures are wired to their own archive.
    assert!(ps1.contains("'arm64' = @{ Suffix = 'arm64'"));
    assert!(sh.contains(r#"arm64) suffix="arm64""#));
}

#[test]
fn fr_058_the_licence_is_fetched_and_verified_not_hoped_for() {
    for name in ["fetch-sumatrapdf.ps1", "fetch-sumatrapdf.sh"] {
        let text = script(name);
        for file in ["COPYING", "COPYING.BSD", "AUTHORS", "LICENSE.txt"] {
            assert!(text.contains(file), "{name} must ship {file}");
        }
        assert!(
            !text
                .to_lowercase()
                .contains("add sumatrapdf license.txt to the bundle by hand"),
            "{name}: a missing licence must fail the script, not warn"
        );
    }
}
