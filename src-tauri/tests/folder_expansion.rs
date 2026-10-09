//! Folder expansion and path canonicalization.
//!
//! Covers the two halves of the folder-drop flow:
//!
//! - FR-004/FR-007: a dropped directory (flagged by `inspect_files`, see the
//!   `ipc::jobs` unit tests) is walked either one level deep or through
//!   the whole subtree (`expand_folder`), under safety ceilings that keep a
//!   drop of a disk root from wedging the app.
//! - FR-005/FR-059: files left out of the selection come back with the reason
//!   they were left out, never silently.
//! - NFR-025: every path arriving from outside the process is absolute,
//!   canonical and existing, or it is refused.
//!
//! Hermetic throughout: every path is a `tempfile::tempdir()`, the size ceiling
//! and the expansion limits are injected, and nothing reads the user's config.

use std::path::{Path, PathBuf};

use maeclyprint_lib::engine::{
    expand_folder, ExpansionLimits, FolderExpansion, FolderScan, SkipReason,
};
use maeclyprint_lib::ipc::jobs::expand_selection;
use maeclyprint_lib::util::error::IpcError;
use maeclyprint_lib::util::paths::canonical_input;

/// Generous ceiling: the limit tests override it explicitly.
const NO_SIZE_LIMIT_MB: u32 = 4096;

fn limits(max_files: u32, max_depth: u32) -> ExpansionLimits {
    ExpansionLimits {
        max_files,
        max_depth,
    }
}

/// Create `dir/name` with a byte of content and return its path.
fn touch(dir: &Path, name: &str) -> PathBuf {
    let p = dir.join(name);
    std::fs::write(&p, b"x").unwrap();
    p
}

fn mkdir(dir: &Path, name: &str) -> PathBuf {
    let p = dir.join(name);
    std::fs::create_dir_all(&p).unwrap();
    p
}

/// Tempdirs live under symlinked roots on macOS (`/var` → `/private/var`), so
/// the expected paths have to be canonical too or every comparison fails for
/// the wrong reason.
fn canonical_tempdir() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    (dir, root)
}

fn names(exp: &FolderExpansion) -> Vec<String> {
    exp.files
        .iter()
        .map(|f| {
            Path::new(f)
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned()
        })
        .collect()
}

// ---------------------------------------------------------------------------
// FR-004 / FR-007: what a folder expands to.
// ---------------------------------------------------------------------------

#[test]
fn fr_004_flat_folder_lists_every_supported_file() {
    let (_guard, root) = canonical_tempdir();
    touch(&root, "b.pdf");
    touch(&root, "a.png");
    touch(&root, "c.jpg");

    let exp = expand_folder(
        &root,
        FolderScan::TopLevel,
        ExpansionLimits::default(),
        NO_SIZE_LIMIT_MB,
    );

    assert_eq!(names(&exp), ["a.png", "b.pdf", "c.jpg"]);
    assert!(exp.skipped.is_empty(), "nothing to skip: {:?}", exp.skipped);
    assert!(!exp.files_truncated);
    assert!(!exp.depth_truncated);
}

#[test]
fn fr_007_top_level_stops_at_the_first_level_recursive_does_not() {
    let (_guard, root) = canonical_tempdir();
    touch(&root, "top.pdf");
    let sub = mkdir(&root, "sub");
    touch(&sub, "nested.pdf");
    let deeper = mkdir(&sub, "deeper");
    touch(&deeper, "deepest.pdf");

    let top = expand_folder(
        &root,
        FolderScan::TopLevel,
        ExpansionLimits::default(),
        NO_SIZE_LIMIT_MB,
    );
    assert_eq!(names(&top), ["top.pdf"]);
    // The subtree it did not enter is signalled, not hidden.
    assert!(
        top.depth_truncated,
        "a non-empty subfolder must be reported in top-level mode"
    );

    let rec = expand_folder(
        &root,
        FolderScan::Recursive,
        ExpansionLimits::default(),
        NO_SIZE_LIMIT_MB,
    );
    assert_eq!(names(&rec), ["deepest.pdf", "nested.pdf", "top.pdf"]);
    assert!(
        !rec.depth_truncated,
        "the whole tree fits under the ceiling"
    );
}

#[test]
fn fr_004_empty_folder_expands_to_nothing() {
    let (_guard, root) = canonical_tempdir();

    let exp = expand_folder(
        &root,
        FolderScan::Recursive,
        ExpansionLimits::default(),
        NO_SIZE_LIMIT_MB,
    );

    assert!(exp.files.is_empty());
    assert!(exp.skipped.is_empty());
    assert!(!exp.files_truncated);
    assert!(!exp.depth_truncated);
    assert_eq!(exp.root, root.to_string_lossy());
}

#[test]
fn fr_007_expansion_order_is_deterministic() {
    let (_guard, root) = canonical_tempdir();
    for n in ["z.pdf", "m.pdf", "a.pdf"] {
        touch(&root, n);
    }
    let sub = mkdir(&root, "sub");
    for n in ["y.png", "b.png"] {
        touch(&sub, n);
    }

    let first = expand_folder(
        &root,
        FolderScan::Recursive,
        ExpansionLimits::default(),
        NO_SIZE_LIMIT_MB,
    );
    for _ in 0..5 {
        let again = expand_folder(
            &root,
            FolderScan::Recursive,
            ExpansionLimits::default(),
            NO_SIZE_LIMIT_MB,
        );
        assert_eq!(first.files, again.files, "expansion order must be stable");
    }
    // Sorted by full path, so the `sub/` entries land between `m.pdf` and
    // `z.pdf` rather than in traversal order.
    assert_eq!(
        names(&first),
        ["a.pdf", "m.pdf", "b.png", "y.png", "z.pdf"],
        "files come back sorted by full path"
    );
}

// ---------------------------------------------------------------------------
// FR-005 / FR-059: nothing is discarded silently.
// ---------------------------------------------------------------------------

#[test]
fn fr_005_unsupported_formats_are_reported_with_a_reason() {
    let (_guard, root) = canonical_tempdir();
    touch(&root, "keep.pdf");
    touch(&root, "setup.exe");
    touch(&root, "README"); // no extension at all

    let exp = expand_folder(
        &root,
        FolderScan::TopLevel,
        ExpansionLimits::default(),
        NO_SIZE_LIMIT_MB,
    );

    assert_eq!(names(&exp), ["keep.pdf"]);
    assert_eq!(exp.skipped.len(), 2);
    assert!(exp
        .skipped
        .iter()
        .all(|s| s.reason == SkipReason::UnsupportedFormat));

    let exe = exp.skipped.iter().find(|s| s.name == "setup.exe").unwrap();
    assert!(
        exe.detail.contains("exe"),
        "the reason names the format: {}",
        exe.detail
    );
    let readme = exp.skipped.iter().find(|s| s.name == "README").unwrap();
    assert!(
        readme.detail.contains("no extension"),
        "an extensionless file says so: {}",
        readme.detail
    );
}

#[test]
fn fr_059_oversized_files_are_reported_separately_from_unsupported_ones() {
    let (_guard, root) = canonical_tempdir();
    touch(&root, "big.pdf");
    touch(&root, "movie.mkv");

    // A 0 MB ceiling makes every non-empty file oversized, which is the
    // cheapest hermetic way to exercise the size branch.
    let exp = expand_folder(&root, FolderScan::TopLevel, ExpansionLimits::default(), 0);

    assert!(exp.files.is_empty());
    let big = exp.skipped.iter().find(|s| s.name == "big.pdf").unwrap();
    assert_eq!(big.reason, SkipReason::TooLarge);
    assert!(big.detail.contains("limit"), "detail: {}", big.detail);
    // Format is decided before size, so an unsupported file never comes back
    // with the misleading "too large".
    let mkv = exp.skipped.iter().find(|s| s.name == "movie.mkv").unwrap();
    assert_eq!(mkv.reason, SkipReason::UnsupportedFormat);
}

// ---------------------------------------------------------------------------
// FR-007: safety ceilings.
// ---------------------------------------------------------------------------

#[test]
fn fr_007_file_ceiling_truncates_instead_of_walking_everything() {
    let (_guard, root) = canonical_tempdir();
    for i in 0..10 {
        touch(&root, &format!("f{i:02}.pdf"));
    }

    let exp = expand_folder(&root, FolderScan::TopLevel, limits(3, 16), NO_SIZE_LIMIT_MB);

    assert!(exp.files_truncated, "the ceiling must be reported");
    assert_eq!(exp.files.len(), 3);
    // Truncation is deterministic: the walk is name-sorted, so it is always the
    // same first three.
    assert_eq!(names(&exp), ["f00.pdf", "f01.pdf", "f02.pdf"]);
}

#[test]
fn fr_007_depth_ceiling_truncates_and_says_so() {
    let (_guard, root) = canonical_tempdir();
    touch(&root, "level0.pdf");
    let l1 = mkdir(&root, "one");
    touch(&l1, "level1.pdf");
    let l2 = mkdir(&l1, "two");
    touch(&l2, "level2.pdf");

    let exp = expand_folder(
        &root,
        FolderScan::Recursive,
        limits(500, 2),
        NO_SIZE_LIMIT_MB,
    );

    assert_eq!(names(&exp), ["level0.pdf", "level1.pdf"]);
    assert!(
        exp.depth_truncated,
        "content below the depth ceiling must be reported"
    );
    assert!(!exp.files_truncated);
    // The ceilings that were applied travel with the result.
    assert_eq!(exp.limits, limits(500, 2));
}

#[cfg(unix)]
#[test]
fn fr_007_symlink_loop_is_reported_not_followed() {
    let (_guard, root) = canonical_tempdir();
    touch(&root, "real.pdf");
    let sub = mkdir(&root, "sub");
    // `sub/back` points at its own ancestor: following it blindly never ends.
    std::os::unix::fs::symlink(&root, sub.join("back")).unwrap();

    let exp = expand_folder(
        &root,
        FolderScan::Recursive,
        ExpansionLimits::default(),
        NO_SIZE_LIMIT_MB,
    );

    assert_eq!(names(&exp), ["real.pdf"], "the file is found exactly once");
    let loop_skip = exp
        .skipped
        .iter()
        .find(|s| s.detail.contains("loop"))
        .unwrap_or_else(|| panic!("expected a reported loop, got {:?}", exp.skipped));
    assert_eq!(loop_skip.reason, SkipReason::Unreadable);
}

#[cfg(unix)]
#[test]
fn fr_004_legitimate_symlinks_are_still_followed() {
    let (_guard, root) = canonical_tempdir();
    let outside = tempfile::tempdir().unwrap();
    let target = outside.path().canonicalize().unwrap();
    touch(&target, "linked.pdf");
    std::os::unix::fs::symlink(&target, root.join("shortcut")).unwrap();

    let exp = expand_folder(
        &root,
        FolderScan::Recursive,
        ExpansionLimits::default(),
        NO_SIZE_LIMIT_MB,
    );

    assert_eq!(names(&exp), ["linked.pdf"]);
    assert!(exp.skipped.is_empty(), "{:?}", exp.skipped);
}

// ---------------------------------------------------------------------------
// NFR-025: every path from outside the process is resolved before it is used.
// (The files/folders split itself is `inspect_files`, unit-tested in
// `ipc::jobs`.)
// ---------------------------------------------------------------------------

#[test]
fn nfr_025_relative_and_missing_paths_are_refused() {
    let (_guard, root) = canonical_tempdir();
    let missing = root.join("ghost.pdf");

    assert!(
        matches!(
            canonical_input("relative/doc.pdf"),
            Err(IpcError::InvalidRequest(_))
        ),
        "a relative path is refused, never resolved against the cwd"
    );
    assert!(matches!(
        canonical_input(&missing.to_string_lossy()),
        Err(IpcError::FileNotFound(_))
    ));
}

#[test]
fn nfr_025_expansion_resolves_its_root_before_walking() {
    let (_guard, root) = canonical_tempdir();
    let sub = mkdir(&root, "sub");
    touch(&sub, "a.pdf");

    // A path with `..` in it names the same folder and must expand identically.
    let winding = root.join("sub").join("..").join("sub");
    let out = expand_selection(
        &[winding.to_string_lossy().into_owned()],
        FolderScan::TopLevel,
        ExpansionLimits::default(),
        NO_SIZE_LIMIT_MB,
    );

    assert_eq!(out.len(), 1);
    assert_eq!(out[0].root, sub.to_string_lossy());
    assert_eq!(names(&out[0]), ["a.pdf"]);
}
