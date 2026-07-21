//! FR-058 — external-dependency detection and the bundling that backs it.
//!
//! Two things are covered here:
//!
//! 1. **The search order** (`engine::deps::resolve`). It is pure, so every case
//!    runs against a tempdir on any host: a bundled copy, a configured path,
//!    a `PATH` entry, a vendor install directory, and nothing at all. No real
//!    program is ever executed and the host's own installs are never consulted
//!    — the probe only sees the directories a test hands it.
//!
//! 2. **The bundling declaration** (`tauri.conf.json`). The whole feature rests
//!    on `bundle.resources` shipping SumatraPDF; deleting that line would leave
//!    a clean Windows machine unable to print anything, and nothing else in the
//!    tree would notice. So it is asserted by parsing the config.

use std::path::{Path, PathBuf};

use maeclyprint_lib::engine::deps::{self, Dependency, FoundIn, Os, Probe, Resolution, ALL};
use maeclyprint_lib::engine::SUPPORTED_EXTS;

/// Create `dir/name` as an empty file and return its path.
fn touch(dir: &Path, name: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, b"").expect("write fixture");
    path
}

/// A probe over the four (already-owned) candidate lists.
fn probe<'a>(
    bundled: &'a [PathBuf],
    configured: Option<&'a Path>,
    path: &'a [PathBuf],
    installed: &'a [PathBuf],
    executables: &'a [&'a str],
) -> Probe<'a> {
    Probe {
        bundled_dirs: bundled,
        configured,
        path_dirs: path,
        install_dirs: installed,
        executables,
        exists: deps::is_file,
    }
}

const EXE: &str = "SumatraPDF.exe";
const EXES: &[&str] = &["SumatraPDF.exe"];

// ---------------------------------------------------------------------------
// FR-058: the search order
// ---------------------------------------------------------------------------

#[test]
fn fr_058_bundled_copy_is_found_first() {
    let root = tempfile::tempdir().unwrap();
    let bundled_dir = root.path().join("resources/sumatrapdf");
    let path_dir = root.path().join("path");
    std::fs::create_dir_all(&bundled_dir).unwrap();
    std::fs::create_dir_all(&path_dir).unwrap();

    let bundled_exe = touch(&bundled_dir, EXE);
    // A second, older copy on PATH must lose: the copy we ship is the one we
    // tested the flag translation against.
    touch(&path_dir, EXE);

    let bundled = vec![bundled_dir];
    let path = vec![path_dir];
    let resolution = resolve_with(&bundled, None, &path, &[]);

    assert_eq!(
        resolution,
        Resolution::Found {
            path: bundled_exe,
            found_in: FoundIn::Bundled,
        }
    );
}

#[test]
fn fr_058_configured_path_beats_path_and_install_dirs() {
    let root = tempfile::tempdir().unwrap();
    let custom_dir = root.path().join("D/tools");
    let path_dir = root.path().join("path");
    let install_dir = root.path().join("install");
    for dir in [&custom_dir, &path_dir, &install_dir] {
        std::fs::create_dir_all(dir).unwrap();
    }
    // The configured value names an executable directly, and it need not be
    // called what we would have looked for.
    let configured = touch(&custom_dir, "sumatra-portable.exe");
    touch(&path_dir, EXE);
    touch(&install_dir, EXE);

    let path = vec![path_dir];
    let installed = vec![install_dir];
    let resolution = resolve_with(&[], Some(&configured), &path, &installed);

    assert_eq!(
        resolution,
        Resolution::Found {
            path: configured,
            found_in: FoundIn::Configured,
        }
    );
}

#[test]
fn fr_058_configured_path_that_does_not_exist_falls_through() {
    let root = tempfile::tempdir().unwrap();
    let path_dir = root.path().join("path");
    std::fs::create_dir_all(&path_dir).unwrap();
    let on_path = touch(&path_dir, EXE);

    let missing = root.path().join("nowhere/SumatraPDF.exe");
    let path = vec![path_dir];
    let resolution = resolve_with(&[], Some(&missing), &path, &[]);

    // A stale setting degrades to the next step rather than failing the job.
    assert_eq!(
        resolution,
        Resolution::Found {
            path: on_path,
            found_in: FoundIn::Path,
        }
    );
}

#[test]
fn fr_058_path_is_searched_before_well_known_install_dirs() {
    let root = tempfile::tempdir().unwrap();
    let path_dir = root.path().join("path");
    let install_dir = root.path().join("Program Files/SumatraPDF");
    std::fs::create_dir_all(&path_dir).unwrap();
    std::fs::create_dir_all(&install_dir).unwrap();
    let on_path = touch(&path_dir, EXE);
    touch(&install_dir, EXE);

    let path = vec![path_dir];
    let installed = vec![install_dir];
    let resolution = resolve_with(&[], None, &path, &installed);

    assert_eq!(
        resolution,
        Resolution::Found {
            path: on_path,
            found_in: FoundIn::Path,
        }
    );
}

#[test]
fn fr_058_install_dirs_are_the_last_resort() {
    let root = tempfile::tempdir().unwrap();
    let install_dir = root.path().join("AppData/Local/SumatraPDF");
    std::fs::create_dir_all(&install_dir).unwrap();
    let installed_exe = touch(&install_dir, EXE);

    let installed = vec![install_dir];
    let resolution = resolve_with(&[], None, &[], &installed);

    assert_eq!(
        resolution,
        Resolution::Found {
            path: installed_exe,
            found_in: FoundIn::Installed,
        }
    );
}

#[test]
fn fr_058_everything_absent_resolves_to_missing() {
    let root = tempfile::tempdir().unwrap();
    let empty = root.path().join("empty");
    std::fs::create_dir_all(&empty).unwrap();

    let dirs = vec![empty];
    // Every step points at a directory that exists but holds no executable.
    let resolution = resolve_with(&dirs, None, &dirs, &dirs);

    assert_eq!(resolution, Resolution::Missing);
}

/// `resolve` over the SumatraPDF executable name, with owned candidate lists.
fn resolve_with(
    bundled: &[PathBuf],
    configured: Option<&Path>,
    path: &[PathBuf],
    installed: &[PathBuf],
) -> Resolution {
    deps::resolve(&probe(bundled, configured, path, installed, EXES))
}

// ---------------------------------------------------------------------------
// FR-058: the remediation message
// ---------------------------------------------------------------------------

#[test]
fn fr_058_missing_dependency_reports_a_remediation_naming_it_and_the_effect() {
    let spec = deps::spec(Dependency::SumatraPdf, Os::Windows);
    let report = deps::from_resolution(&spec, &Resolution::Missing);

    assert!(report.applicable);
    assert!(!report.found);
    assert!(report.path.is_none());

    let message = report.remediation.expect("a missing dependency remediates");
    assert!(message.contains("SumatraPDF"), "names it: {message}");
    assert!(message.contains("printed"), "states the effect: {message}");
    assert!(
        message.contains("Install SumatraPDF"),
        "says what to do: {message}"
    );
}

#[test]
fn fr_058_every_applicable_dependency_has_a_remediation_that_names_it() {
    for os in [Os::Windows, Os::MacOs, Os::Linux] {
        for dep in ALL {
            let spec = deps::spec(*dep, os);
            let report = deps::from_resolution(&spec, &Resolution::Missing);
            if !spec.applicable() {
                // Not used on this OS: reporting it missing would be a false
                // alarm (SumatraPDF on macOS, where CUPS does the printing).
                assert!(report.remediation.is_none(), "{dep:?} on {os:?}");
                continue;
            }
            let message = report.remediation.expect("applicable + missing");
            assert!(
                message.starts_with(spec.name),
                "{dep:?} on {os:?} must name itself: {message}"
            );
            assert!(
                message.contains(" — "),
                "{dep:?} on {os:?} must state an effect: {message}"
            );
        }
    }
}

#[test]
fn fr_058_a_found_dependency_reports_no_remediation_and_blocks_nothing() {
    let root = tempfile::tempdir().unwrap();
    let found = touch(root.path(), EXE);
    let spec = deps::spec(Dependency::SumatraPdf, Os::Windows);

    let report = deps::from_resolution(
        &spec,
        &Resolution::Found {
            path: found.clone(),
            found_in: FoundIn::Bundled,
        },
    );

    assert!(report.found);
    assert_eq!(
        report.path.as_deref(),
        Some(found.to_string_lossy().as_ref())
    );
    assert_eq!(report.found_in, Some(FoundIn::Bundled));
    assert!(report.remediation.is_none());
    assert!(report.blocks.is_empty());
}

/// The honesty rule: a report may only claim a format is blocked if the engine
/// can actually print that format today. SumatraPDF is the final printer on
/// Windows, so it blocks the whole supported list; the converters the
/// architecture documents but has not wired block nothing, because their
/// formats are unsupported whether or not the tool is installed.
#[test]
fn fr_058_blocked_formats_are_a_subset_of_what_the_engine_supports() {
    for os in [Os::Windows, Os::MacOs, Os::Linux] {
        for dep in ALL {
            let spec = deps::spec(*dep, os);
            for ext in spec.blocks {
                assert!(
                    SUPPORTED_EXTS.contains(ext),
                    "{dep:?} on {os:?} claims to block '{ext}', which the engine \
                     cannot print anyway"
                );
            }
        }
    }
}

#[test]
fn fr_058_sumatrapdf_blocks_every_supported_format_on_windows_and_none_elsewhere() {
    let windows = deps::spec(Dependency::SumatraPdf, Os::Windows);
    assert_eq!(windows.blocks, SUPPORTED_EXTS);

    for os in [Os::MacOs, Os::Linux] {
        let spec = deps::spec(Dependency::SumatraPdf, os);
        assert!(
            !spec.applicable(),
            "CUPS prints on {os:?}; SumatraPDF is irrelevant there"
        );
        let report = deps::from_resolution(&spec, &Resolution::Missing);
        assert!(report.blocks.is_empty());
    }
}

/// The probe against the real host must not raise a false alarm on the machine
/// running the tests: on macOS/Linux nothing applicable can be missing in a way
/// that blocks a format, because the only format-blocking dependency there is
/// none.
#[test]
fn fr_058_report_all_never_blocks_a_format_off_windows() {
    let reports = deps::report_all();
    assert_eq!(reports.len(), ALL.len());

    if !cfg!(windows) {
        for report in &reports {
            assert!(
                report.blocks.is_empty(),
                "{} must not block formats off Windows: {:?}",
                report.name,
                report.blocks
            );
        }
        let sumatra = reports
            .iter()
            .find(|r| r.id == Dependency::SumatraPdf)
            .expect("SumatraPDF is always reported");
        assert!(!sumatra.applicable);
        assert!(sumatra.remediation.is_none());
    }
}

// ---------------------------------------------------------------------------
// FR-058: the bundling declaration
// ---------------------------------------------------------------------------

/// `bundle.resources` is the only thing that puts SumatraPDF on a clean Windows
/// machine. It is one line in a JSON file that no compiler checks, so it gets a
/// test: without it, the app builds, installs, and cannot print a single file.
#[test]
fn fr_058_tauri_conf_declares_the_sumatrapdf_bundle_resource() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tauri.conf.json");
    let text =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let conf: serde_json::Value = serde_json::from_str(&text).expect("tauri.conf.json parses");

    let resources = conf
        .pointer("/bundle/resources")
        .expect("bundle.resources must exist — see scripts/fetch-sumatrapdf.ps1");
    let patterns: Vec<&str> = match resources {
        serde_json::Value::Array(items) => items.iter().filter_map(|v| v.as_str()).collect(),
        // The map form is also legal Tauri config; keys are the source paths.
        serde_json::Value::Object(map) => map.keys().map(String::as_str).collect(),
        other => panic!("bundle.resources must be an array or a map, got {other}"),
    };

    assert!(
        patterns.iter().any(|p| p.contains("sumatrapdf")),
        "no bundle.resources entry ships SumatraPDF: {patterns:?}"
    );

    // The declared directory has to exist with at least one committed file:
    // tauri-build fails the build when a resource glob matches nothing, which
    // would break every platform, not just Windows.
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/sumatrapdf");
    assert!(
        dir.is_dir(),
        "{} is declared as a bundle resource but does not exist",
        dir.display()
    );
    assert!(
        std::fs::read_dir(&dir).expect("read resource dir").count() > 0,
        "{} must keep at least one committed file so the glob always matches",
        dir.display()
    );
}

/// The resolver looks for the bundled copy where `bundle.resources` puts it.
/// If either side moves, this fails instead of silently degrading to "user must
/// install SumatraPDF themselves".
#[test]
fn fr_058_bundled_search_covers_the_declared_resource_layout() {
    let spec = deps::spec(Dependency::SumatraPdf, Os::Windows);
    let dirs = deps::bundled_dirs(Path::new("/app"), spec.bundle_subdir);
    assert!(
        dirs.contains(&PathBuf::from("/app/resources/sumatrapdf")),
        "the Windows bundle resource dir must be searched: {dirs:?}"
    );
    assert!(
        dirs.contains(&PathBuf::from("/Resources/sumatrapdf")),
        "the macOS .app resource dir must be searched: {dirs:?}"
    );
}

// ---------------------------------------------------------------------------
// FR-058: the well-known install directories, per OS
// ---------------------------------------------------------------------------

/// The Windows table is the one that matters on the shipping target, and it is
/// the one a macOS dev machine can never exercise by accident — so it is driven
/// with an injected environment instead of the real one.
#[test]
fn fr_058_windows_install_dirs_come_from_the_environment() {
    let env = |name: &str| match name {
        "LOCALAPPDATA" => Some(r"C:\Users\ada\AppData\Local".to_string()),
        "ProgramFiles" => Some(r"C:\Program Files".to_string()),
        "ProgramFiles(x86)" => Some(r"C:\Program Files (x86)".to_string()),
        _ => None,
    };

    let sumatra = deps::install_dirs(Dependency::SumatraPdf, Os::Windows, &env);
    assert!(sumatra
        .iter()
        .any(|d| d.ends_with("SumatraPDF") && d.starts_with(r"C:\Users\ada\AppData\Local")));

    let chromium = deps::install_dirs(Dependency::Chromium, Os::Windows, &env);
    assert!(
        chromium
            .iter()
            .any(|d| d.ends_with("Edge/Application") || d.to_string_lossy().contains("Edge")),
        "Edge is the recommended browser on Windows: {chromium:?}"
    );

    // Nothing is fabricated for a variable the machine does not set.
    let bare = |_: &str| None;
    assert!(deps::install_dirs(Dependency::SumatraPdf, Os::Windows, &bare).is_empty());
}

#[test]
fn fr_058_path_dirs_are_read_from_the_supplied_environment() {
    let root = tempfile::tempdir().unwrap();
    let a = root.path().join("a");
    let b = root.path().join("b");
    let joined = std::env::join_paths([&a, &b]).unwrap();
    let value = joined.to_string_lossy().into_owned();

    let env = |name: &str| (name == "PATH").then(|| value.clone());
    assert_eq!(deps::path_dirs(&env), vec![a, b]);

    let empty = |_: &str| None;
    assert!(deps::path_dirs(&empty).is_empty());
}
