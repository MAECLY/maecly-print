//! FR-001 / FR-003 — the handoff file this DLL writes is exactly what the app
//! reads back.
//!
//! These tests are the reason a Windows-only feature can be developed on a
//! non-Windows machine: the transport contract is plain Rust, so the part that
//! actually breaks in production (a field renamed, a value spelled differently,
//! a cap crossed) is caught on any host.
//!
//! Hermetic: every path is a `tempfile::TempDir`, the clock is injected, and
//! nothing reads or writes the developer's real handoff directory.

use std::path::PathBuf;

use shell_ext_win::handoff::{self, Action, MAX_PATHS, SCHEMA_VERSION};

fn at(rfc3339: &str) -> time::OffsetDateTime {
    time::OffsetDateTime::parse(rfc3339, &time::format_description::well_known::Rfc3339).unwrap()
}

fn paths(items: &[&str]) -> Vec<PathBuf> {
    items.iter().map(PathBuf::from).collect()
}

/// The consumer deserializes into a struct with these exact field names. A
/// rename on either side makes the extension silently stop working — the app
/// reports "handoff file is not valid JSON" and opens an empty window — so the
/// key set is frozen here.
#[test]
fn fr_001_the_json_has_exactly_the_keys_the_app_deserializes() {
    let payload = handoff::build(
        &paths([r"C:\a.pdf"].as_slice()),
        Action::Configure,
        Some("quick".into()),
        at("2026-07-20T21:11:03Z"),
    );
    let value: serde_json::Value = serde_json::to_value(&payload).unwrap();
    let object = value.as_object().unwrap();

    let mut keys: Vec<&str> = object.keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        [
            "action",
            "invoked_at",
            "paths",
            "profile_id",
            "schema_version",
            "source",
            "truncated_from",
        ],
        "field names are the contract with src-tauri/src/cli/handoff.rs"
    );

    assert_eq!(object["schema_version"], serde_json::json!(SCHEMA_VERSION));
    assert_eq!(object["source"], serde_json::json!("windows-explorer"));
    assert_eq!(object["profile_id"], serde_json::json!("quick"));
    // An absent profile is null, not a missing key and not "": the app's
    // `Option<String>` has to see a null to fall back to `--profile`.
    let none = handoff::build(
        &paths([r"C:\a.pdf"].as_slice()),
        Action::Configure,
        None,
        at("2026-07-20T21:11:03Z"),
    );
    assert_eq!(
        serde_json::to_value(&none).unwrap()["profile_id"],
        serde_json::Value::Null
    );
}

/// The app parses `action` with serde in kebab-case and treats an unknown value
/// as a hard parse error, never as a silent default.
#[test]
fn fr_001_action_values_match_the_apps_vocabulary() {
    for (action, expected) in [
        (Action::Configure, "configure"),
        (Action::PrintDefault, "print-default"),
        (Action::Queue, "queue"),
    ] {
        assert_eq!(action.as_str(), expected);
        let payload = handoff::build(
            &paths([r"C:\a.pdf"].as_slice()),
            action,
            None,
            at("2026-07-20T21:11:03Z"),
        );
        assert_eq!(
            serde_json::to_value(&payload).unwrap()["action"],
            serde_json::json!(expected)
        );
    }
}

/// The single reason the transport is JSON instead of a delimited list: Windows
/// file names may contain commas, quotes, backslashes and non-BMP characters,
/// and any of them would corrupt a naive separator-based format.
#[test]
fn fr_001_hostile_file_names_survive_the_round_trip() {
    let hostile = [
        r#"C:\Facturas\name, with "quotes".docx"#,
        r"C:\Facturas\tab	and\backslash.pdf",
        "C:\\Facturas\\emoji \u{1f5a8}\u{fe0f} and \u{05d0}\u{05d1}\u{05d2}.png",
        r"\\server\share\UNC path.pdf",
        r"C:\a\very\deep\path\that\keeps\going\and\going\file.tiff",
    ];
    let payload = handoff::build(
        &paths(hostile.as_slice()),
        Action::PrintDefault,
        None,
        at("2026-07-20T21:11:03Z"),
    );

    let json = serde_json::to_string(&payload).unwrap();
    let back: serde_json::Value = serde_json::from_str(&json).unwrap();
    let back: Vec<&str> = back["paths"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert_eq!(back, hostile);
}

/// FR-003: "selections of any size". 5,000 files is a plausible invoice folder
/// and roughly thirteen times what would fit in a 32,767-character command
/// line, which is the whole point of the file.
#[test]
fn fr_003_a_five_thousand_file_selection_fits_the_apps_limits() {
    let many: Vec<PathBuf> = (0..5_000)
        .map(|i| PathBuf::from(format!(r"C:\Users\ana\Facturas\2026-{i:05}.pdf")))
        .collect();
    let payload = handoff::build(&many, Action::Queue, None, at("2026-07-20T21:11:03Z"));
    let bytes = serde_json::to_vec(&payload).unwrap();

    assert_eq!(payload.paths.len(), 5_000);
    // The app refuses a handoff file over its byte cap before reading it.
    assert!(
        bytes.len() < handoff::MAX_BYTES,
        "5,000 paths serialized to {} bytes, over the app's cap",
        bytes.len()
    );
}

#[test]
fn fr_003_a_selection_over_the_cap_is_trimmed_rather_than_rejected() {
    let many: Vec<PathBuf> = (0..MAX_PATHS + 500)
        .map(|i| PathBuf::from(format!(r"C:\f{i}.pdf")))
        .collect();
    let payload = handoff::build(&many, Action::Queue, None, at("2026-07-20T21:11:03Z"));
    // Sending all of them would make the app reject the *whole* selection with
    // "lists N paths, more than the 10000 limit" and print nothing at all.
    assert_eq!(payload.paths.len(), MAX_PATHS);
    assert_eq!(payload.paths[0], r"C:\f0.pdf");
}

/// The app is launched the instant the file lands, so a reader must never be
/// able to observe a partially written file.
#[test]
fn fr_001_the_file_is_written_atomically_and_named_predictably() {
    let dir = tempfile::tempdir().unwrap();
    let payload = handoff::build(
        &paths([r"C:\a.pdf", r"C:\b.png"].as_slice()),
        Action::Configure,
        None,
        at("2026-07-20T21:11:03Z"),
    );

    let written = handoff::write_to_dir(dir.path(), &payload).unwrap();

    assert!(written.exists());
    assert_eq!(written.parent().unwrap(), dir.path());
    let name = written.file_name().unwrap().to_string_lossy().into_owned();
    assert!(name.starts_with("sel-"), "unexpected file name {name}");
    assert!(name.ends_with(".json"), "unexpected file name {name}");

    // No temporary left behind: the app sweeps orphans only every 24 h, and a
    // stray .tmp per right-click would accumulate for a day.
    let leftovers: Vec<String> = std::fs::read_dir(dir.path())
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".tmp"))
        .collect();
    assert!(leftovers.is_empty(), "left behind {leftovers:?}");

    let raw = std::fs::read_to_string(&written).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(parsed["paths"].as_array().unwrap().len(), 2);
    assert_eq!(
        parsed["invoked_at"],
        serde_json::json!("2026-07-20T21:11:03Z")
    );
}

/// Two right-clicks in the same second must not collide.
#[test]
fn fr_001_consecutive_writes_get_distinct_files() {
    let dir = tempfile::tempdir().unwrap();
    let payload = handoff::build(
        &paths([r"C:\a.pdf"].as_slice()),
        Action::Configure,
        None,
        at("2026-07-20T21:11:03Z"),
    );

    let first = handoff::write_to_dir(dir.path(), &payload).unwrap();
    let second = handoff::write_to_dir(dir.path(), &payload).unwrap();

    assert_ne!(first, second);
    assert!(first.exists() && second.exists());
}

/// `invoked_at` is checked against a 60-second replay window, so it has to be
/// in the format the app parses (`Rfc3339`) and in UTC.
#[test]
fn fr_001_the_timestamp_is_rfc3339_utc() {
    let payload = handoff::build(
        &paths([r"C:\a.pdf"].as_slice()),
        Action::Configure,
        None,
        at("2026-07-20T23:11:03+02:00"),
    );
    assert_eq!(payload.invoked_at, "2026-07-20T21:11:03Z");
    time::OffsetDateTime::parse(
        &payload.invoked_at,
        &time::format_description::well_known::Rfc3339,
    )
    .expect("the app parses this with the same description");
}

/// Read one of the app's source files. This crate cannot depend on the app
/// crate (it is a separate, standalone package outside the workspace), so
/// reading its source is the only automated link between the two.
fn app_source(relative: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(relative)
        .canonicalize()
        .unwrap_or_else(|e| panic!("{relative} is the app's source of truth, but: {e}"));
    std::fs::read_to_string(path).unwrap()
}

/// The literal of `const <name>: &str = "…";` in `source`.
fn const_str(source: &str, name: &str) -> String {
    let needle = format!("const {name}: &str = \"");
    let start = source
        .find(&needle)
        .unwrap_or_else(|| panic!("the app no longer declares `{name}`"))
        + needle.len();
    let rest = &source[start..];
    let end = rest.find('"').expect("unterminated string literal");
    rest[..end].to_owned()
}

/// The literal of the first `<prefix>"…"` occurrence in `source`.
fn literal_after(source: &str, prefix: &str) -> String {
    let start = source
        .find(prefix)
        .unwrap_or_else(|| panic!("the app no longer spells `{prefix}`"))
        + prefix.len();
    let rest = &source[start..];
    let end = rest.find('"').expect("unterminated string literal");
    rest[..end].to_owned()
}

/// The app refuses any handoff file that does not canonicalize inside *its*
/// handoff directory, so the producer has to resolve the same path.
///
/// The rule is derived from the app's own source rather than re-typed here: a
/// second copy of `("com", "Maecly", "Pluryprint")` would agree with itself
/// forever, and renaming `ORG` in `src-tauri/src/util/paths.rs` would ship a
/// DLL writing into a directory the app never reads — with no test failing.
#[test]
fn fr_001_the_handoff_directory_matches_the_apps_rule() {
    let paths_rs = app_source("../src-tauri/src/util/paths.rs");
    let handoff_rs = app_source("../src-tauri/src/cli/handoff.rs");

    // `paths::data_dir()` — the base the app's handoff directory hangs off.
    assert!(
        paths_rs.contains("dirs().data_local_dir().to_path_buf()"),
        "the app's data_dir no longer resolves to ProjectDirs::data_local_dir"
    );
    let expected = directories::ProjectDirs::from(
        &const_str(&paths_rs, "QUALIFIER"),
        &const_str(&paths_rs, "ORG"),
        &const_str(&paths_rs, "APP"),
    )
    .expect("no home directory to resolve the app's directories in")
    .data_local_dir()
    .join(literal_after(&handoff_rs, "paths::data_dir().join(\""));

    assert_eq!(
        handoff::handoff_dir().unwrap(),
        expected,
        "the app's handoff directory moved; handoff::handoff_dir must follow"
    );
}

/// The test above resolves the app's rule with *this* crate's `directories`,
/// so it only proves agreement while both crates use the same release line.
/// Keep them in lockstep: a bump on one side alone could move one producer or
/// consumer of the handoff directory without any test noticing.
#[test]
fn fr_001_both_crates_resolve_paths_with_the_same_directories_release() {
    let requirement = |manifest: &str| {
        let line = manifest
            .lines()
            .find(|l| l.trim_start().starts_with("directories = "))
            .expect("the manifest declares `directories`");
        line.trim().to_owned()
    };
    assert_eq!(
        requirement(&app_source("Cargo.toml")),
        requirement(&app_source("../src-tauri/Cargo.toml")),
    );
}

/// The handoff directory, byte for byte, as 0.0.1 resolved it. Literals on
/// purpose: a `directories` upgrade that moved `data_local_dir` would move the
/// app's side too, and the rule-derived test above would still pass.
#[test]
fn fr_001_the_handoff_directory_is_pinned_byte_for_byte() {
    let dir = handoff::handoff_dir()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    #[cfg(windows)]
    {
        let local = std::env::var("LOCALAPPDATA").expect("LOCALAPPDATA is set");
        assert_eq!(dir, format!(r"{local}\Maecly\Pluryprint\data\handoff"));
    }
    #[cfg(target_os = "macos")]
    {
        let home = std::env::var("HOME").expect("HOME is set");
        assert_eq!(
            dir,
            format!("{home}/Library/Application Support/com.Maecly.Pluryprint/handoff")
        );
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    let _ = dir;
}

/// A drift detector, not a style check: the only automated link between the two
/// schemas is reading the consumer's source and confirming the names and limits
/// it declares are still the ones written here.
#[test]
fn fr_001_the_consumer_still_declares_this_schema() {
    let source = app_source("../src-tauri/src/cli/handoff.rs");

    for field in [
        "pub schema_version:",
        "pub source:",
        "pub action:",
        "pub profile_id:",
        "pub invoked_at:",
        "pub paths:",
    ] {
        assert!(
            source.contains(field),
            "the app no longer declares `{field}` — the handoff schema has drifted"
        );
    }
    assert!(
        source.contains("HANDOFF_SCHEMA_VERSION: u32 = 1"),
        "the app expects a schema version other than the one written here"
    );
    assert!(
        source.contains("MAX_HANDOFF_PATHS: usize = 10_000"),
        "the app's path cap changed; handoff::MAX_PATHS must follow"
    );
    assert!(
        source.contains("MAX_HANDOFF_BYTES: u64 = 64 * 1024 * 1024"),
        "the app's byte cap changed; handoff::MAX_BYTES must follow"
    );
}
