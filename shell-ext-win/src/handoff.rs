//! Producer half of the JSON handoff file.
//!
//! `src-tauri/src/cli/handoff.rs` is the consumer and the source of truth for
//! the schema; this module only *writes* what that module will accept. The two
//! cannot be one crate today (the app is a standalone crate outside any
//! workspace), so the contract is pinned here by
//! `tests/handoff_roundtrip.rs`, which asserts the exact key set, the exact
//! `action` spellings and the caps, and fails if the consumer's struct drifts.
//!
//! The rules the consumer enforces, and therefore the rules this module has to
//! satisfy:
//!
//! * `schema_version` must be exactly [`SCHEMA_VERSION`];
//! * `paths` must be non-empty and no longer than [`MAX_PATHS`];
//! * the file must be under 4 MiB;
//! * `invoked_at` must be an RFC 3339 stamp no older than 60 s (replay window),
//!   and no more than 5 s in the future (clock skew);
//! * the file must *canonicalize* inside [`handoff_dir`], so it has to be
//!   written there and nowhere else.
//!
//! Everything here is portable and unit-testable on any host; nothing in this
//! module touches Win32.

use std::io;
use std::path::{Path, PathBuf};

use serde::Serialize;

/// On-disk schema version. Mirrors `HANDOFF_SCHEMA_VERSION` in the app.
pub const SCHEMA_VERSION: u32 = 1;

/// Value written to `source`. The app uses it for telemetry and error text.
pub const SOURCE: &str = "windows-explorer";

/// Largest selection the app will honour (`MAX_HANDOFF_PATHS`). A bigger
/// selection is truncated rather than sent whole, because the app refuses an
/// over-long payload outright and the user would get nothing at all.
pub const MAX_PATHS: usize = 10_000;

/// What a menu entry asks the app to do.
///
/// The wire spellings are the app's `ContextAction` serde representation
/// (kebab-case). They are also the values accepted by the `--action` flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Open the print dialog pre-loaded with the selection.
    Configure,
    /// Print immediately with the default (or requested) profile.
    PrintDefault,
    /// Add to the queue without starting it.
    Queue,
}

impl Action {
    /// The exact string that goes on the wire.
    pub const fn as_str(self) -> &'static str {
        match self {
            Action::Configure => "configure",
            Action::PrintDefault => "print-default",
            Action::Queue => "queue",
        }
    }
}

impl Serialize for Action {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}

/// The payload. Field order is the documented one; field *names* are the
/// contract.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Handoff {
    pub schema_version: u32,
    pub source: String,
    pub action: Action,
    pub profile_id: Option<String>,
    pub invoked_at: String,
    pub paths: Vec<String>,
}

/// Build a payload from a selection.
///
/// Pure, with the clock injected, so the freshness contract can be asserted
/// without waiting. `paths` longer than [`MAX_PATHS`] is truncated — see the
/// constant for why — and the caller learns about it from the returned count.
pub fn build(
    paths: &[PathBuf],
    action: Action,
    profile_id: Option<String>,
    now: time::OffsetDateTime,
) -> Handoff {
    let paths: Vec<String> = paths
        .iter()
        .take(MAX_PATHS)
        // to_string_lossy, not a fallible conversion: a Windows path that is
        // not valid UTF-16-to-UTF-8 round-trippable is vanishingly rare and
        // dropping the whole selection over one name would be worse than
        // printing it with a replacement character in the log.
        .map(|p| p.to_string_lossy().into_owned())
        .collect();

    Handoff {
        schema_version: SCHEMA_VERSION,
        source: SOURCE.to_string(),
        action,
        profile_id,
        invoked_at: format_rfc3339(now),
        paths,
    }
}

/// RFC 3339 in UTC, e.g. `2026-07-20T21:11:03Z`, which is what the consumer
/// parses with `time`'s well-known `Rfc3339` description.
pub fn format_rfc3339(at: time::OffsetDateTime) -> String {
    at.to_offset(time::UtcOffset::UTC)
        .format(&time::format_description::well_known::Rfc3339)
        // The only way this fails is a year outside the formatter's range; a
        // stamp we cannot format is better sent as one the consumer will
        // reject as stale than as a silent success.
        .unwrap_or_else(|_| String::from("1970-01-01T00:00:00Z"))
}

/// Directory both sides agree on: `<data_local_dir>/handoff`.
///
/// Resolved through the same `directories::ProjectDirs` call the app makes
/// (`ProjectDirs::from("com", "Maecly", "MaeclyPrint")`), because the app
/// canonicalizes the incoming path and refuses anything that does not land
/// inside *its* idea of this directory. On Windows this is under
/// `%LOCALAPPDATA%`, which is ACL'd to the user — unlike a shared temp dir,
/// where another account could plant a payload.
pub fn handoff_dir() -> io::Result<PathBuf> {
    directories::ProjectDirs::from("com", "Maecly", "MaeclyPrint")
        .map(|d| d.data_local_dir().join("handoff"))
        .ok_or_else(|| io::Error::other("no home directory to resolve the handoff directory in"))
}

/// Write `payload` into `dir` as `sel-<uuid>.json` and return the path.
///
/// Atomic: the JSON goes to a `.tmp` sibling and is renamed into place, so the
/// app can never observe a half-written file even though it may be launched
/// the instant the rename lands.
pub fn write_to_dir(dir: &Path, payload: &Handoff) -> io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;

    let id = uuid::Uuid::now_v7().simple().to_string();
    let path = dir.join(format!("sel-{id}.json"));
    let tmp = dir.join(format!("sel-{id}.json.tmp"));

    let bytes = serde_json::to_vec(payload).map_err(io::Error::other)?;
    std::fs::write(&tmp, &bytes)?;
    match std::fs::rename(&tmp, &path) {
        Ok(()) => Ok(path),
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            Err(e)
        }
    }
}

/// Build and write a handoff file in one step, into the real handoff directory.
pub fn write(paths: &[PathBuf], action: Action, profile_id: Option<String>) -> io::Result<PathBuf> {
    let payload = build(paths, action, profile_id, time::OffsetDateTime::now_utc());
    write_to_dir(&handoff_dir()?, &payload)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(s: &str) -> time::OffsetDateTime {
        time::OffsetDateTime::parse(s, &time::format_description::well_known::Rfc3339).unwrap()
    }

    #[test]
    fn fr_001_builds_the_payload_the_app_expects() {
        let h = build(
            &[PathBuf::from(r"C:\a.pdf")],
            Action::Configure,
            None,
            at("2026-07-20T21:11:03Z"),
        );
        assert_eq!(h.schema_version, SCHEMA_VERSION);
        assert_eq!(h.source, "windows-explorer");
        assert_eq!(h.invoked_at, "2026-07-20T21:11:03Z");
        assert_eq!(h.paths, vec![r"C:\a.pdf"]);
    }

    #[test]
    fn fr_003_truncates_at_the_limit_the_app_enforces() {
        let many: Vec<PathBuf> = (0..MAX_PATHS + 10)
            .map(|i| PathBuf::from(format!(r"C:\f{i}.pdf")))
            .collect();
        let h = build(&many, Action::Queue, None, at("2026-07-20T21:11:03Z"));
        // Exactly at the cap, so the app accepts it instead of refusing the
        // whole selection.
        assert_eq!(h.paths.len(), MAX_PATHS);
    }

    #[test]
    fn fr_001_offsets_are_normalized_to_utc() {
        // A non-UTC clock must still produce a `Z` stamp: the consumer's
        // freshness window compares against `now_utc()`.
        assert_eq!(
            format_rfc3339(at("2026-07-20T23:11:03+02:00")),
            "2026-07-20T21:11:03Z"
        );
    }
}
