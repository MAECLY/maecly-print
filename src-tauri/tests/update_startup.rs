//! Integration tests for the launch-time update check (FR-080, FR-084).
//!
//! Everything here drives `run_startup_check_with`, the path/clock/fetcher-
//! injected core of the check that `lib.rs`'s `setup` schedules. That injection
//! is what makes these tests hermetic: the state file is a tempdir, "now" is a
//! literal, and the fetcher is a closure that records whether it was called —
//! so no test touches the network or the real per-user state. (The one test that
//! reads the published manifests lives in `src/update/mod.rs` and stays
//! `#[ignore]`.)
//!
//! The assertions cover the two halves of FR-080 — "on startup" and "at most
//! once per 24 hours" — plus FR-084's skip: whether the check runs at all, and
//! whether its result is worth telling the user about.

use std::cell::Cell;
use std::path::Path;

use pluryprint_lib::update::{
    run_startup_check_with, ReleaseInfo, UpdateCheck, UpdateState, MIN_CHECK_INTERVAL_HOURS,
    STARTUP_CHECK_DELAY,
};
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

/// The running version every test compares releases against.
const CURRENT: &str = "1.0.0";

fn at(rfc3339: &str) -> OffsetDateTime {
    OffsetDateTime::parse(rfc3339, &Rfc3339).expect("test timestamp is RFC 3339")
}

/// A newer release than [`CURRENT`], as GitHub would report it.
fn newer_release() -> ReleaseInfo {
    ReleaseInfo {
        tag_name: "v1.2.0".to_string(),
        html_url: "https://github.com/MAECLY/maecly-print/releases/tag/v1.2.0".to_string(),
        body: Some("Faster printing".to_string()),
    }
}

/// Write `state` to a fresh tempdir and return the directory (kept alive by the
/// caller) plus the state file inside it.
fn state_file(state: &UpdateState) -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("update-state.json");
    state.save_to(&path).expect("write the fixture state");
    (dir, path)
}

/// Run the launch-time check against `path`, serving `release` and recording
/// whether the fetcher was reached at all.
fn run(
    enabled: bool,
    path: &Path,
    now: &str,
    release: Option<ReleaseInfo>,
) -> (Option<UpdateCheck>, bool) {
    let fetched = Cell::new(false);
    let out = run_startup_check_with(enabled, path, at(now), CURRENT, || {
        fetched.set(true);
        Ok(release)
    });
    (out, fetched.get())
}

#[test]
fn fr_080_constants_are_the_requirement() {
    // "delayed by 30 seconds ... at most once per 24 hours" — the scheduler in
    // `lib.rs` consumes these, it does not carry its own numbers.
    assert_eq!(STARTUP_CHECK_DELAY, std::time::Duration::from_secs(30));
    assert_eq!(MIN_CHECK_INTERVAL_HOURS, 24);
}

#[test]
fn fr_080_never_checked_before_checks_and_reports() {
    // A fresh install: no state file at all (not even an empty one).
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("update-state.json");

    let (out, fetched) = run(true, &path, "2026-07-20T12:00:00Z", Some(newer_release()));
    assert!(fetched, "a first launch has nothing to throttle against");
    let check = out.expect("a newer release is worth telling the user about");
    assert!(check.available);
    assert_eq!(check.latest.as_deref(), Some("v1.2.0"));
    assert_eq!(check.current, CURRENT);
    assert!(check.url.is_some(), "the toast needs a link to the release");

    // The check stamped the state file, so the next launch is throttled.
    let saved = UpdateState::load_from(&path);
    assert_eq!(saved.last_check_at.as_deref(), Some("2026-07-20T12:00:00Z"));
}

#[test]
fn fr_080_a_check_one_hour_ago_does_not_check_again() {
    let (_dir, path) = state_file(&UpdateState {
        last_check_at: Some("2026-07-20T11:00:00Z".to_string()),
        skipped_version: None,
    });

    let (out, fetched) = run(true, &path, "2026-07-20T12:00:00Z", Some(newer_release()));
    assert!(
        !fetched,
        "inside the 24h window the launch check must not reach the network"
    );
    assert!(out.is_none());

    // And it left the previous stamp alone — a throttled launch is a no-op.
    assert_eq!(
        UpdateState::load_from(&path).last_check_at.as_deref(),
        Some("2026-07-20T11:00:00Z")
    );
}

#[test]
fn fr_080_a_check_twenty_five_hours_ago_checks_again() {
    let (_dir, path) = state_file(&UpdateState {
        last_check_at: Some("2026-07-19T11:00:00Z".to_string()),
        skipped_version: None,
    });

    let (out, fetched) = run(true, &path, "2026-07-20T12:00:00Z", Some(newer_release()));
    assert!(fetched, "25h is past the once-per-24h window");
    assert!(out.is_some());
    assert_eq!(
        UpdateState::load_from(&path).last_check_at.as_deref(),
        Some("2026-07-20T12:00:00Z"),
        "the fresh check replaces the old stamp"
    );
}

#[test]
fn fr_080_check_on_startup_disabled_never_checks() {
    // Stale enough that the throttle alone would let it through: the
    // preference is what stops it.
    let (_dir, path) = state_file(&UpdateState {
        last_check_at: Some("2026-01-01T00:00:00Z".to_string()),
        skipped_version: None,
    });

    let (out, fetched) = run(false, &path, "2026-07-20T12:00:00Z", Some(newer_release()));
    assert!(
        !fetched,
        "the Settings switch is the user's answer, and it is honoured before anything else"
    );
    assert!(out.is_none());
    assert_eq!(
        UpdateState::load_from(&path).last_check_at.as_deref(),
        Some("2026-01-01T00:00:00Z"),
        "a launch that did not check must not pretend it did"
    );
}

#[test]
fn fr_084_the_skipped_version_is_not_announced_again() {
    let (_dir, path) = state_file(&UpdateState {
        last_check_at: None,
        skipped_version: Some("v1.2.0".to_string()),
    });

    let (out, fetched) = run(true, &path, "2026-07-20T12:00:00Z", Some(newer_release()));
    // The check still runs — skipping silences the *notification*, not the
    // bookkeeping, so About stays accurate and the throttle keeps ticking.
    assert!(fetched);
    assert!(
        out.is_none(),
        "the user already said no to exactly this version"
    );
    let saved = UpdateState::load_from(&path);
    assert_eq!(saved.last_check_at.as_deref(), Some("2026-07-20T12:00:00Z"));
    assert_eq!(
        saved.skipped_version.as_deref(),
        Some("v1.2.0"),
        "the skip survives the check that consulted it"
    );
}

#[test]
fn fr_084_a_release_newer_than_the_skipped_one_is_announced() {
    let (_dir, path) = state_file(&UpdateState {
        last_check_at: None,
        skipped_version: Some("v1.1.0".to_string()),
    });

    let (out, _) = run(true, &path, "2026-07-20T12:00:00Z", Some(newer_release()));
    let check = out.expect("skipping 1.1.0 does not silence 1.2.0");
    assert_eq!(check.latest.as_deref(), Some("v1.2.0"));
}

#[test]
fn a_completed_check_that_found_nothing_still_arms_the_throttle() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("update-state.json");

    // Same version as the running build: nothing to say.
    let up_to_date = ReleaseInfo {
        tag_name: "v1.0.0".to_string(),
        html_url: "https://x/y".to_string(),
        body: None,
    };
    let (out, fetched) = run(true, &path, "2026-07-20T12:00:00Z", Some(up_to_date));
    assert!(fetched);
    assert!(out.is_none(), "no toast when there is nothing newer");
    assert_eq!(
        UpdateState::load_from(&path).last_check_at.as_deref(),
        Some("2026-07-20T12:00:00Z"),
        "otherwise an up-to-date app would re-check on every single launch"
    );
}

#[test]
fn a_failed_check_stays_silent_and_retries_next_launch() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("update-state.json");

    // Offline / rate-limited: nobody asked for this check, so there is no
    // honest place to show the error.
    let out = run_startup_check_with(true, &path, at("2026-07-20T12:00:00Z"), CURRENT, || {
        Err("could not reach github.com — check your connection".to_string())
    });
    assert!(out.is_none());
    assert!(
        UpdateState::load_from(&path).last_check_at.is_none(),
        "a failed attempt must not burn the 24h window"
    );
}

#[test]
fn a_corrupt_state_file_does_not_wedge_the_launch_check() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("update-state.json");
    std::fs::write(&path, "{not json").unwrap();

    let (out, fetched) = run(true, &path, "2026-07-20T12:00:00Z", Some(newer_release()));
    assert!(fetched, "an unreadable stamp degrades to 'check again'");
    assert!(out.is_some());
}
