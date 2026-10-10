//! Update check against the release manifest of the user's channel.
//!
//! This is the *check*: it reads the updater manifest (`latest.json`) the
//! release workflow publishes for the configured channel (FR-085), compares
//! its version to the running one with semver, and reports whether something
//! newer exists plus a link to its release page. Downloading, verifying and
//! installing a release is [`install`]'s job, and only ever happens on an
//! explicit user action.
//!
//! The notice and the installer read the **same** manifest
//! ([`install::endpoint`]), so the version the About screen announces is the
//! version `update install` installs, the beta channel works for both, and
//! nothing calls the GitHub REST API (no 60-requests-an-hour limit, no
//! anonymous `api.github.com` lookup that a private repository answers
//! with 404).
//!
//! Everything except the one network call is a pure function
//! ([`interpret_manifest_response`]/[`is_newer`]/[`evaluate`]) with co-located
//! tests; [`fetch_manifest`] is the only part that touches the network and is
//! kept tiny so it is easy to reason about offline.
//!
//! Beyond the check itself this module owns the small amount of state the
//! update flow has to remember between runs ([`UpdateState`]): when the last
//! check ran (FR-080's once-per-24h throttle) and which version the user chose
//! to skip (FR-084). The decisions built on top of it — [`due_for_check`],
//! [`should_check_on_startup`], [`should_notify`] — are pure and tested;
//! [`run_startup_check_with`] sequences them against an injected state file,
//! clock and fetcher, and [`run_startup_check`] is the thin wrapper that binds
//! those three to the real config, the real state file and the network.

pub mod install;

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use specta::Type;
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

use crate::util::error::{IpcErrorCode, LocalizedError};
use crate::util::paths;

/// Parsed subset of a GitHub release we care about.
#[derive(Debug, Clone, PartialEq)]
pub struct ReleaseInfo {
    /// The git tag, e.g. `"v0.2.0"` (may or may not carry a leading `v`).
    pub tag_name: String,
    /// The release's human-facing page on github.com.
    pub html_url: String,
    /// Release notes (markdown), when the release has a body.
    pub body: Option<String>,
}

/// The result of an update check, returned to the frontend. `latest` is the tag
/// of the newest published release (when one was found), `available` is whether
/// it is strictly newer than `current`, and `url`/`notes` are populated only
/// when an update is available (so the UI has a link and changelog to show).
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
pub struct UpdateCheck {
    /// The running app's version.
    pub current: String,
    /// Tag of the newest published release, when one was found.
    pub latest: Option<String>,
    /// Whether `latest` is strictly newer than `current`.
    pub available: bool,
    /// Release page to open, set only when an update is available.
    pub url: Option<String>,
    /// Release notes (markdown), set only when an update is available.
    pub notes: Option<String>,
    /// RFC 3339 timestamp of when the check ran.
    pub checked_at: String,
}

/// Parse `(owner, repo)` from the crate's `repository` URL
/// (`https://github.com/<owner>/<repo>`). Returns `None` if it is not a
/// recognizable GitHub URL, so the caller can report "repository not
/// configured" rather than hit a bogus API path.
pub fn repo_slug() -> Option<(String, String)> {
    parse_repo_slug(env!("CARGO_PKG_REPOSITORY"))
}

/// Pure core of [`repo_slug`], split out for testing.
fn parse_repo_slug(url: &str) -> Option<(String, String)> {
    let rest = url
        .trim()
        .strip_prefix("https://github.com/")
        .or_else(|| url.trim().strip_prefix("http://github.com/"))?;
    let rest = rest.trim_end_matches('/').trim_end_matches(".git");
    let mut parts = rest.split('/');
    let owner = parts.next()?.to_string();
    let repo = parts.next()?.to_string();
    if owner.is_empty() || repo.is_empty() {
        return None;
    }
    Some((owner, repo))
}

/// Whether release `tag` is strictly newer than the running `current` version.
/// Both may carry a leading `v`/`V`, which is stripped before a semver compare.
/// An unparseable version on either side yields `false` (and logs), so a weird
/// tag never spuriously prompts an update.
///
/// Comparison uses [`semver::Version::cmp_precedence`], not the `Ord`/`>`
/// operator: `Ord` totally-orders versions and therefore ranks
/// `0.0.1+build` *above* `0.0.1`, which would spuriously flag a build-metadata-
/// only "release" as an update. SemVer §10 excludes build metadata from
/// precedence, and `cmp_precedence` implements exactly that.
pub fn is_newer(current: &str, tag: &str) -> bool {
    let cur = match semver::Version::parse(strip_v(current)) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(event = "update_version_unparseable", which = "current", value = %current, error = %e);
            return false;
        }
    };
    let latest = match semver::Version::parse(strip_v(tag)) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(event = "update_version_unparseable", which = "tag", value = %tag, error = %e);
            return false;
        }
    };
    latest.cmp_precedence(&cur).is_gt()
}

/// Strip a single leading `v`/`V` and surrounding whitespace from a version-ish
/// string.
fn strip_v(s: &str) -> &str {
    let s = s.trim();
    s.strip_prefix('v')
        .or_else(|| s.strip_prefix('V'))
        .unwrap_or(s)
}

/// Fold a fetched release (or its absence) and the running version into the wire
/// [`UpdateCheck`]. `checked_at` is the RFC 3339 time the check ran.
pub fn evaluate(current: &str, release: Option<ReleaseInfo>, checked_at: String) -> UpdateCheck {
    match release {
        Some(r) => {
            let available = is_newer(current, &r.tag_name);
            UpdateCheck {
                current: current.to_string(),
                latest: Some(r.tag_name),
                available,
                url: if available {
                    Some(r.html_url).filter(|u| !u.is_empty())
                } else {
                    None
                },
                notes: if available { r.body } else { None },
                checked_at,
            }
        }
        None => UpdateCheck {
            current: current.to_string(),
            latest: None,
            available: false,
            url: None,
            notes: None,
            checked_at,
        },
    }
}

/// An [`IpcErrorCode::UpdateCheckFailed`] with a stable `reason` the UI
/// translates (FR-092); the English text stays the log/CLI detail.
pub fn failure(reason: &'static str, message: impl Into<String>) -> LocalizedError {
    LocalizedError::new(IpcErrorCode::UpdateCheckFailed, reason, message)
}

/// Build the blocking HTTP agent, honouring a system proxy from the standard
/// environment variables. ureq does not read these itself, so we wire the first
/// of `HTTPS_PROXY`/`HTTP_PROXY` (either case) that parses. Only HTTP CONNECT
/// proxies are supported without the `socks-proxy` feature; a `socks://` value
/// would fail to parse here and is simply skipped (direct connection).
fn build_agent() -> ureq::Agent {
    let mut builder = ureq::AgentBuilder::new().timeout(std::time::Duration::from_secs(10));
    if let Some(proxy) = std::env::var("HTTPS_PROXY")
        .or_else(|_| std::env::var("https_proxy"))
        .or_else(|_| std::env::var("HTTP_PROXY"))
        .or_else(|_| std::env::var("http_proxy"))
        .ok()
        .filter(|p| !p.trim().is_empty())
        .and_then(|p| ureq::Proxy::new(p).ok())
    {
        builder = builder.proxy(proxy);
    }
    builder.build()
}

/// The release an updater manifest (`latest.json`) describes: its `version`
/// as the tag and its `notes` as the body; `page` is the link the UI offers.
/// `None` when the document has no version. Pure.
pub fn parse_manifest(json: &str, page: &str) -> Option<ReleaseInfo> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let version = v.get("version")?.as_str()?.trim().to_string();
    if version.is_empty() {
        return None;
    }
    Some(ReleaseInfo {
        tag_name: version,
        html_url: page.to_string(),
        body: v
            .get("notes")
            .and_then(|n| n.as_str())
            .map(str::to_string)
            .filter(|s| !s.is_empty()),
    })
}

/// Map an updater manifest response (`status` + `body`) to a result,
/// independent of the HTTP transport so the whole decision table is
/// unit-tested offline. Pure.
///
/// - `200` with a version → `Ok(Some(_))`. A `200` that does **not** parse is
///   an error, not `Ok(None)`: "you're up to date" off an unreadable payload
///   would be a lie.
/// - `404` → `Ok(None)`: nothing is published on this channel yet. While the
///   repository is private every anonymous request lands here too.
/// - `403` / `429` → rate-limit error.
/// - anything else → an "unexpected status" error naming the code.
pub fn interpret_manifest_response(
    status: u16,
    body: &str,
    page: &str,
) -> Result<Option<ReleaseInfo>, LocalizedError> {
    match status {
        200 => parse_manifest(body, page).map(Some).ok_or_else(|| {
            failure(
                "bad_response",
                "the update server's manifest has no version",
            )
        }),
        404 => Ok(None),
        403 | 429 => Err(failure(
            "rate_limited",
            "the update server is limiting requests — try again later",
        )),
        other => Err(failure(
            "bad_status",
            format!("the update server returned an unexpected status ({other})"),
        )
        .param("status", other)),
    }
}

/// The GitHub release page of `version` (`…/releases/tag/vX.Y.Z`), the link
/// the notice offers. Releases are tagged `v<version>` by
/// `.github/workflows/release.yml`. Pure.
pub fn release_page(version: &str) -> String {
    format!(
        "{}/releases/tag/v{}",
        env!("CARGO_PKG_REPOSITORY"),
        strip_v(version)
    )
}

/// GET the manifest at `url` and interpret it.
///
/// A thin transport wrapper: a 10-second timeout keeps a hung request from
/// blocking the off-thread command, and the status + body go to
/// [`interpret_manifest_response`]. `page` is the release link to report when
/// the manifest is a local test server's ([`install::ENDPOINT_ENV`]); for the
/// published channels the link is the release's own page ([`release_page`]).
pub fn fetch_manifest(
    url: &tauri::Url,
    overridden: bool,
) -> Result<Option<ReleaseInfo>, LocalizedError> {
    let user_agent = format!("Pluryprint/{}", env!("CARGO_PKG_VERSION"));
    let (status, body) = match build_agent()
        .get(url.as_str())
        .set("User-Agent", &user_agent)
        .set("Accept", "application/json")
        .call()
    {
        Ok(resp) => (resp.status(), resp.into_string().unwrap_or_default()),
        Err(ureq::Error::Status(code, resp)) => (code, resp.into_string().unwrap_or_default()),
        Err(ureq::Error::Transport(_)) => {
            return Err(failure(
                "offline",
                "could not reach the update server — check your connection",
            ))
        }
    };
    let mut release = interpret_manifest_response(status, &body, url.as_str())?;
    if !overridden {
        if let Some(r) = release.as_mut() {
            r.html_url = release_page(&r.tag_name);
        }
    }
    Ok(release)
}

/// The newest release on `channel` (FR-085): the manifest of that channel
/// ([`install::default_endpoint`]), or the one named by
/// [`install::ENDPOINT_ENV`] when it is set, so a test server drives the
/// check, the launch-time check and `update install` alike.
pub fn fetch_configured_latest(
    channel: crate::settings::UpdateChannel,
) -> Result<Option<ReleaseInfo>, LocalizedError> {
    let override_value = install::endpoint_override();
    let url = install::resolve_endpoint(override_value.as_deref(), channel)
        .map_err(|m| failure("not_configured", m))?;
    fetch_manifest(&url, override_value.is_some())
}

// ---------------------------------------------------------------------------
// Persisted update state (FR-080 throttle, FR-084 "skip this version")
// ---------------------------------------------------------------------------

/// FR-080: how long after launch the automatic check waits before running, so
/// a cold start never pays for a network round-trip. The caller owns the sleep
/// (see [`run_startup_check`]); the constant lives here because it is part of
/// the requirement, not of whoever schedules it.
pub const STARTUP_CHECK_DELAY: std::time::Duration = std::time::Duration::from_secs(30);

/// FR-080: the automatic check runs at most once per this window. Manual checks
/// from the About screen also stamp [`UpdateState::last_check_at`], so pressing
/// the button postpones the next automatic check rather than adding to it.
pub const MIN_CHECK_INTERVAL_HOURS: i64 = 24;

/// The little the update flow remembers between runs. Deliberately *not* part of
/// `config.toml`: none of it is a user preference — it is bookkeeping the app
/// writes for itself, and a corrupt or deleted file must degrade to "check
/// again", never to a settings-parse refusal.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct UpdateState {
    /// RFC 3339 timestamp of the last completed check (manual or automatic).
    #[serde(default)]
    pub last_check_at: Option<String>,
    /// The release tag the user chose to skip (FR-084). While it matches the
    /// latest release, the launch-time check stays silent about it; a *newer*
    /// release than the skipped one prompts again.
    #[serde(default)]
    pub skipped_version: Option<String>,
}

/// `update-state.json` in the per-user data directory, next to `queue.json`
/// (live state, not configuration).
pub fn state_path() -> PathBuf {
    paths::data_dir().join("update-state.json")
}

impl UpdateState {
    /// Read the state file, degrading to defaults when it is missing or
    /// unreadable. A corrupt file is logged and ignored rather than propagated:
    /// the worst case is one extra update check.
    pub fn load_from(path: &Path) -> UpdateState {
        let Ok(text) = std::fs::read_to_string(path) else {
            return UpdateState::default();
        };
        match serde_json::from_str(&text) {
            Ok(state) => state,
            Err(e) => {
                tracing::warn!(
                    event = "update_state_unreadable",
                    path = %path.display(),
                    error = %e,
                    "ignoring an unparseable update-state.json"
                );
                UpdateState::default()
            }
        }
    }

    /// Atomically replace the state file (temp file in the same directory, then
    /// rename), so a crash mid-write can never leave a truncated file behind.
    pub fn save_to(&self, path: &Path) -> std::io::Result<()> {
        use std::io::Write;

        let parent = path
            .parent()
            .expect("the update state path always has a parent directory");
        paths::ensure_dir(parent)?;

        let json = serde_json::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        let mut tmp = tempfile::NamedTempFile::new_in(parent)?;
        tmp.write_all(json.as_bytes())?;
        tmp.flush()?;
        tmp.persist(path)
            .map_err(|e| std::io::Error::other(format!("persist failed: {e}")))?;
        Ok(())
    }

    /// Drop a skipped version that is not newer than `current` (FR-084).
    ///
    /// A skip only ever silences a release *ahead* of the running build. Once
    /// the user is on that version or past it (installed by hand, or by a
    /// later update), the skip describes nothing and must stop showing up as
    /// "Skipping vX" in About. An unparseable tag is kept: guessing would be
    /// worse than one stale line. Returns whether anything changed.
    pub fn prune_stale_skip(&mut self, current: &str) -> bool {
        let Some(skipped) = self.skipped_version.as_deref() else {
            return false;
        };
        let parses = semver::Version::parse(strip_v(skipped)).is_ok()
            && semver::Version::parse(strip_v(current)).is_ok();
        if parses && !is_newer(current, skipped) {
            self.skipped_version = None;
            return true;
        }
        false
    }

    /// Read the state for display, removing a stale skip from the file too
    /// (see [`UpdateState::prune_stale_skip`]). A failed rewrite is logged:
    /// the returned value is already correct either way.
    pub fn load_current_from(path: &Path, current: &str) -> UpdateState {
        let mut state = Self::load_from(path);
        if state.prune_stale_skip(current) {
            match Self::update_at(path, |s| {
                s.prune_stale_skip(current);
            }) {
                Ok(saved) => return saved,
                Err(e) => tracing::warn!(event = "update_state_save_failed", error = %e),
            }
        }
        state
    }

    /// Read the real per-user state file.
    pub fn load() -> UpdateState {
        Self::load_from(&state_path())
    }

    /// Write the real per-user state file.
    pub fn save(&self) -> std::io::Result<()> {
        self.save_to(&state_path())
    }

    /// Apply `mutate` to the state **as it is on disk right now** and persist
    /// the result, returning it.
    ///
    /// This is the only safe way to change one field of the file, because the
    /// GUI and the CLI are two processes writing it: the caller that loads a
    /// copy, does something slow (the launch-time check waits on a network
    /// round-trip that takes seconds) and then saves its stale copy silently
    /// undoes whatever the other process wrote in between — a "skip this
    /// version" chosen during that window used to vanish.
    ///
    /// Re-reading immediately before the write is deliberately not a lock: the
    /// two fields are owned by different flows (`last_check_at` by a check,
    /// `skipped_version` by the user), so a field-wise merge loses nothing a
    /// lock would have saved, and the file is bookkeeping the app can always
    /// rebuild — a stale lock file would be a worse failure than one extra
    /// update check. The remaining window (re-read → rename) is microseconds
    /// wide and can only lose a write to the *same* field.
    pub fn update_at<F>(path: &Path, mutate: F) -> std::io::Result<UpdateState>
    where
        F: FnOnce(&mut UpdateState),
    {
        let mut state = Self::load_from(path);
        mutate(&mut state);
        state.save_to(path)?;
        Ok(state)
    }
}

/// Whether enough time has passed since `last_check_at` for another check
/// (FR-080). Pure.
///
/// Never having checked is due. So is an unparseable timestamp (a hand-edited
/// or older-build file must not wedge updates forever) and a timestamp in the
/// future (the clock moved backwards, or the file was copied from another
/// machine) — in both cases refusing to check would be the worse failure.
pub fn due_for_check(last_check_at: Option<&str>, now: OffsetDateTime) -> bool {
    let Some(raw) = last_check_at else {
        return true;
    };
    let Ok(last) = OffsetDateTime::parse(raw.trim(), &Rfc3339) else {
        tracing::warn!(event = "update_last_check_unparseable", value = %raw);
        return true;
    };
    let elapsed = now - last;
    elapsed.is_negative() || elapsed >= time::Duration::hours(MIN_CHECK_INTERVAL_HOURS)
}

/// FR-080: whether the launch-time check should run at all — the user's
/// `updater.check_on_startup` preference AND the once-per-24h throttle. Pure.
pub fn should_check_on_startup(
    check_on_startup: bool,
    state: &UpdateState,
    now: OffsetDateTime,
) -> bool {
    check_on_startup && due_for_check(state.last_check_at.as_deref(), now)
}

/// FR-084: whether an available update still deserves an unprompted
/// notification, i.e. the user has not skipped exactly this version. Pure.
///
/// Skipping is version-scoped, not permanent: skipping `1.2.0` silences that
/// release only — `1.3.0` prompts again. A manual check from the About screen
/// does not consult this; the user asked, so they get the answer either way.
pub fn should_notify(check: &UpdateCheck, skipped_version: Option<&str>) -> bool {
    if !check.available {
        return false;
    }
    let Some(latest) = check.latest.as_deref() else {
        return false;
    };
    match skipped_version {
        Some(skipped) => !same_version(skipped, latest),
        None => true,
    }
}

/// Whether two version-ish strings name the same release, ignoring a leading
/// `v` and (per SemVer §10) build metadata. Unparseable versions fall back to a
/// case-insensitive string compare so a non-semver tag can still be skipped.
fn same_version(a: &str, b: &str) -> bool {
    match (
        semver::Version::parse(strip_v(a)),
        semver::Version::parse(strip_v(b)),
    ) {
        (Ok(x), Ok(y)) => x.cmp_precedence(&y).is_eq(),
        _ => strip_v(a).eq_ignore_ascii_case(strip_v(b)),
    }
}

/// FR-080 launch-time check with every external dependency injected: the
/// preference, the state file, the clock, the running version, and the fetch
/// itself.
///
/// Returns `Some(check)` only when there is something the user should be told
/// about: the preference is on, the 24h window has elapsed, a newer release
/// exists, and it is not the one they skipped. Every other outcome — throttled,
/// disabled, offline, rate-limited, up to date — is `None`, because a silent
/// background check has no UI to fail into.
///
/// `fetch` is only called once the decision to check has been made, which is
/// what lets a test assert that a throttled or disabled launch performs no
/// network call at all.
///
/// The state file is stamped with `now` whenever a check actually completed,
/// including one that found nothing: the throttle counts *checks*, not
/// findings, or an up-to-date app would re-check on every launch. The stamp
/// goes through [`UpdateState::update_at`], so the seconds this function spends
/// inside `fetch` cannot swallow a "skip this version" the user chose from the
/// About screen meanwhile — and it is the state as re-read, not the copy loaded
/// before the network call, that decides whether to notify.
pub fn run_startup_check_with<F>(
    check_on_startup: bool,
    state_path: &Path,
    now: OffsetDateTime,
    current: &str,
    fetch: F,
) -> Option<UpdateCheck>
where
    F: FnOnce() -> Result<Option<ReleaseInfo>, String>,
{
    let state = UpdateState::load_from(state_path);

    if !should_check_on_startup(check_on_startup, &state, now) {
        return None;
    }

    let release = match fetch() {
        Ok(release) => release,
        Err(e) => {
            // Nobody asked for this check, so there is nowhere honest to show
            // an error: log it and try again after the next interval.
            tracing::warn!(event = "update_startup_check_failed", error = %e);
            return None;
        }
    };

    let checked_at = now
        .format(&Rfc3339)
        .unwrap_or_else(|_| crate::queue::job::now_rfc3339());
    let check = evaluate(current, release, checked_at);

    // Re-read before writing: `state` above was loaded before `fetch`, and the
    // user may have skipped a version from the About screen while the request
    // was in flight. Saving the pre-fetch copy would put that choice back to
    // whatever it was seconds ago.
    let latest_state = match UpdateState::update_at(state_path, |s| {
        s.last_check_at = Some(check.checked_at.clone());
    }) {
        Ok(saved) => saved,
        Err(e) => {
            tracing::warn!(event = "update_state_save_failed", error = %e);
            // The stamp is lost (the next launch re-checks), but the skip must
            // still be honoured, so fall back to the freshest copy we can read.
            UpdateState::load_from(state_path)
        }
    };

    should_notify(&check, latest_state.skipped_version.as_deref()).then_some(check)
}

/// FR-080 launch-time check against the real config, state file and network.
///
/// Scheduled from `lib.rs`'s `setup` on a background thread after
/// [`STARTUP_CHECK_DELAY`]; the result (when there is one) is pushed to the
/// webview as [`crate::ipc::update::UpdateAvailable`].
pub fn run_startup_check() -> Option<UpdateCheck> {
    let config = crate::settings::Config::load_or_default();
    let channel = config.updater.channel;
    run_startup_check_with(
        config.updater.check_on_startup,
        &state_path(),
        OffsetDateTime::now_utc(),
        env!("CARGO_PKG_VERSION"),
        || fetch_configured_latest(channel).map_err(|e| e.message),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// FR-080: the check reads the updater manifest, the same document
    /// `update install` reads, and maps its statuses to stable reasons.
    #[test]
    fn fr_080_an_updater_manifest_reads_as_a_release() {
        let url = "http://127.0.0.1:8787/latest.json";
        let r = interpret_manifest_response(
            200,
            r#"{"version":"0.0.2","notes":"Fixes","pub_date":"2026-10-09T00:00:00Z","platforms":{}}"#,
            url,
        )
        .unwrap()
        .unwrap();
        assert_eq!(r.tag_name, "0.0.2");
        assert_eq!(r.body.as_deref(), Some("Fixes"));
        assert_eq!(r.html_url, url);
        assert!(is_newer("0.0.1", &r.tag_name));

        // Nothing published on this channel (or the repository is private).
        assert_eq!(interpret_manifest_response(404, "", url).unwrap(), None);
        let bad = interpret_manifest_response(200, r#"{"notes":"x"}"#, url).unwrap_err();
        assert_eq!(bad.reason, "bad_response");
        assert_eq!(
            interpret_manifest_response(200, "not json", url)
                .unwrap_err()
                .reason,
            "bad_response"
        );
        let status = interpret_manifest_response(500, "", url).unwrap_err();
        assert_eq!(status.reason, "bad_status");
    }

    /// FR-092: every manifest failure carries a stable reason the UI
    /// translates, with its parameters declared.
    #[test]
    fn fr_092_update_failures_carry_stable_reasons() {
        let url = "https://example.invalid/latest.json";
        let reason = |status: u16, body: &str| {
            interpret_manifest_response(status, body, url)
                .unwrap_err()
                .reason
        };
        assert_eq!(reason(403, ""), "rate_limited");
        assert_eq!(reason(429, ""), "rate_limited");
        assert_eq!(reason(200, "nope"), "bad_response");
        let bad = interpret_manifest_response(502, "", url).unwrap_err();
        assert_eq!(bad.reason, "bad_status");
        assert_eq!(bad.params.get("status").map(String::as_str), Some("502"));
        assert_eq!(bad.code, IpcErrorCode::UpdateCheckFailed);
    }

    /// FR-080: the notice uses the release manifest, never the GitHub REST
    /// API. The API answers an anonymous request for a private repository
    /// with 404, rate-limits at 60 requests an hour, and knows nothing about
    /// the beta channel's manifest.
    #[test]
    fn fr_080_notice_uses_manifest_not_api() {
        let api = ["api", ".github", ".com/repos"].concat();
        for (name, source) in [
            ("update/mod.rs", include_str!("mod.rs")),
            ("update/install.rs", include_str!("install.rs")),
            ("ipc/update.rs", include_str!("../ipc/update.rs")),
            ("cli/update.rs", include_str!("../cli/update.rs")),
        ] {
            assert!(!source.contains(&api), "{name} still calls the REST API");
        }
        // The link the notice offers is the release's own page.
        assert_eq!(
            release_page("0.2.0"),
            "https://github.com/MAECLY/maecly-print/releases/tag/v0.2.0"
        );
        assert_eq!(release_page("v0.2.0-beta.1"), release_page("0.2.0-beta.1"));
    }

    /// FR-086: the beta manifest is rewritten by every release, stable ones
    /// included, so a beta user moves on to the stable release of the same
    /// version; and switching back to stable never offers an older version
    /// (the updater installs only strictly newer ones).
    #[test]
    fn fr_086_beta_to_stable_no_downgrade() {
        let release = |v: &str| {
            Some(ReleaseInfo {
                tag_name: v.to_string(),
                html_url: release_page(v),
                body: None,
            })
        };
        let now = || "2026-10-10T00:00:00Z".to_string();

        // On a beta, the stable release of the same version is an update.
        assert!(evaluate("0.2.0-beta.2", release("0.2.0"), now()).available);
        // A later beta is an update for a beta user.
        assert!(evaluate("0.2.0-beta.1", release("0.2.0-beta.2"), now()).available);
        // Back on stable while running a newer beta: no downgrade.
        assert!(!evaluate("0.3.0-beta.1", release("0.2.0"), now()).available);
        // The same version is never offered again.
        assert!(!evaluate("0.2.0", release("v0.2.0"), now()).available);
    }

    #[test]
    fn repo_slug_parses_the_configured_repository() {
        // Pins the *real* repository the app ships against — `git remote -v` is
        // https://github.com/MAECLY/maecly-print.git. This is the assertion that
        // catches a `repository` field pointing at a repo that does not exist,
        // which silently turns every update check into a 404 ("up to date") and
        // every About/issue link into a dead end. Owner case is significant here
        // only as documentation: GitHub itself is case-insensitive on slugs.
        assert_eq!(
            repo_slug(),
            Some(("MAECLY".to_string(), "maecly-print".to_string()))
        );
        assert_eq!(
            env!("CARGO_PKG_REPOSITORY"),
            "https://github.com/MAECLY/maecly-print",
            "the frontend mirrors this URL in src/lib/repo.ts"
        );
    }

    #[test]
    fn parse_repo_slug_variants() {
        assert_eq!(
            parse_repo_slug("https://github.com/MAECLY/maecly-print"),
            Some(("MAECLY".into(), "maecly-print".into()))
        );
        assert_eq!(
            parse_repo_slug("https://github.com/MAECLY/maecly-print.git/"),
            Some(("MAECLY".into(), "maecly-print".into()))
        );
        // A trailing path (branch/tree links) still yields the bare slug.
        assert_eq!(
            parse_repo_slug("https://github.com/MAECLY/maecly-print/tree/main/docs"),
            Some(("MAECLY".into(), "maecly-print".into()))
        );
        // Anything that is not a github.com repo URL is refused, so the caller
        // reports "repository not configured" instead of building a bogus API
        // path out of it.
        assert_eq!(parse_repo_slug("https://example.com/x/y"), None);
        assert_eq!(
            parse_repo_slug("https://gitlab.com/MAECLY/maecly-print"),
            None
        );
        assert_eq!(parse_repo_slug("not a url"), None);
        assert_eq!(parse_repo_slug("https://github.com/only-owner"), None);
        assert_eq!(parse_repo_slug("https://github.com/"), None);
        assert_eq!(parse_repo_slug(""), None);
    }

    #[test]
    fn is_newer_matrix() {
        assert!(is_newer("0.0.1", "v0.1.0"));
        assert!(is_newer("0.0.1", "0.1.0"));
        assert!(!is_newer("0.1.0", "v0.1.0")); // equal
        assert!(!is_newer("0.2.0", "v0.1.0")); // older
        assert!(!is_newer("0.0.1", "garbage")); // unparseable tag
        assert!(!is_newer("not-semver", "v1.0.0")); // unparseable current
    }

    #[test]
    fn is_newer_ignores_build_metadata() {
        // Semver precedence ignores build metadata (spec §10), so a tag
        // that differs only by `+meta` is NOT newer than the running version.
        assert!(!is_newer("0.0.1", "v0.0.1+hotfix"));
        assert!(!is_newer("0.0.1+local", "v0.0.1"));
        assert!(!is_newer("1.2.3+a", "1.2.3+b"));
    }

    #[test]
    fn is_newer_prerelease_is_older_than_release() {
        // A pre-release precedes its release: 1.0.0-beta < 1.0.0, so offering
        // 1.0.0-beta to a machine on 1.0.0 must not flag an update.
        assert!(!is_newer("1.0.0", "1.0.0-beta"));
        // And the reverse direction IS newer (beta machine, stable release out).
        assert!(is_newer("1.0.0-beta", "1.0.0"));
    }

    #[test]
    fn evaluate_with_newer_release_flags_available() {
        let r = ReleaseInfo {
            tag_name: "v0.2.0".to_string(),
            html_url: "https://x/y".to_string(),
            body: Some("notes".to_string()),
        };
        let out = evaluate("0.1.0", Some(r), "2026-07-12T00:00:00Z".to_string());
        assert!(out.available);
        assert_eq!(out.latest.as_deref(), Some("v0.2.0"));
        assert_eq!(out.url.as_deref(), Some("https://x/y"));
        assert_eq!(out.notes.as_deref(), Some("notes"));
    }

    #[test]
    fn evaluate_with_same_release_is_not_available() {
        let r = ReleaseInfo {
            tag_name: "v0.1.0".to_string(),
            html_url: "https://x/y".to_string(),
            body: Some("notes".to_string()),
        };
        let out = evaluate("0.1.0", Some(r), "2026-07-12T00:00:00Z".to_string());
        assert!(!out.available);
        assert_eq!(out.latest.as_deref(), Some("v0.1.0"));
        assert!(out.url.is_none(), "no link when nothing to update to");
        assert!(out.notes.is_none());
    }

    #[test]
    fn evaluate_with_no_release_found() {
        let out = evaluate("0.1.0", None, "2026-07-12T00:00:00Z".to_string());
        assert!(!out.available);
        assert!(out.latest.is_none());
        assert!(out.url.is_none());
        assert_eq!(out.current, "0.1.0");
    }

    // -- persisted state (FR-080 / FR-084) ----------------------------------

    /// A fixed "now" so every throttle assertion is deterministic.
    fn at(rfc3339: &str) -> OffsetDateTime {
        OffsetDateTime::parse(rfc3339, &Rfc3339).expect("test timestamp is RFC 3339")
    }

    fn available_check(latest: &str) -> UpdateCheck {
        UpdateCheck {
            current: "1.0.0".to_string(),
            latest: Some(latest.to_string()),
            available: true,
            url: Some("https://x/y".to_string()),
            notes: None,
            checked_at: "2026-07-20T10:00:00Z".to_string(),
        }
    }

    #[test]
    fn update_state_round_trips_through_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("update-state.json");

        let state = UpdateState {
            last_check_at: Some("2026-07-20T10:00:00Z".to_string()),
            skipped_version: Some("v9.9.9".to_string()),
        };
        // The parent directory does not exist yet: saving creates it.
        state.save_to(&path).unwrap();
        assert_eq!(UpdateState::load_from(&path), state);
    }

    #[test]
    fn update_state_degrades_to_defaults_when_missing_or_corrupt() {
        let dir = tempfile::tempdir().unwrap();

        // Missing file — a fresh install.
        let missing = dir.path().join("update-state.json");
        assert_eq!(UpdateState::load_from(&missing), UpdateState::default());

        // Corrupt file: ignored, not propagated. The cost of being wrong here
        // is one extra update check, so it must never surface as an error.
        let corrupt = dir.path().join("corrupt.json");
        std::fs::write(&corrupt, "{not json").unwrap();
        assert_eq!(UpdateState::load_from(&corrupt), UpdateState::default());

        // A file written by an older build that knew fewer fields still loads.
        let partial = dir.path().join("partial.json");
        std::fs::write(&partial, r#"{"last_check_at":"2026-07-20T10:00:00Z"}"#).unwrap();
        let loaded = UpdateState::load_from(&partial);
        assert_eq!(
            loaded.last_check_at.as_deref(),
            Some("2026-07-20T10:00:00Z")
        );
        assert!(loaded.skipped_version.is_none());
    }

    #[test]
    fn due_for_check_honours_the_24h_window() {
        let now = at("2026-07-20T12:00:00Z");

        // Never checked.
        assert!(due_for_check(None, now));
        // Exactly 24h ago is due; a minute short of it is not.
        assert!(due_for_check(Some("2026-07-19T12:00:00Z"), now));
        assert!(!due_for_check(Some("2026-07-19T12:01:00Z"), now));
        assert!(!due_for_check(Some("2026-07-20T11:59:00Z"), now));
        // Long ago.
        assert!(due_for_check(Some("2026-01-01T00:00:00Z"), now));
        // A different offset is still understood (RFC 3339, not just `Z`).
        assert!(!due_for_check(Some("2026-07-20T13:30:00+02:00"), now));
    }

    #[test]
    fn due_for_check_never_wedges_on_a_bad_timestamp() {
        let now = at("2026-07-20T12:00:00Z");
        // Unparseable (hand-edited file, older build) → check anyway.
        assert!(due_for_check(Some("yesterday"), now));
        assert!(due_for_check(Some(""), now));
        // In the future (clock moved back, file copied between machines) →
        // check anyway, rather than going silent until the date catches up.
        assert!(due_for_check(Some("2030-01-01T00:00:00Z"), now));
    }

    #[test]
    fn should_check_on_startup_respects_the_preference_and_the_throttle() {
        let now = at("2026-07-20T12:00:00Z");
        let fresh = UpdateState {
            last_check_at: Some("2026-07-20T11:00:00Z".to_string()),
            skipped_version: None,
        };
        let stale = UpdateState {
            last_check_at: Some("2026-07-18T11:00:00Z".to_string()),
            skipped_version: None,
        };

        // The preference wins outright: no check, however stale the state is.
        assert!(!should_check_on_startup(false, &stale, now));
        assert!(!should_check_on_startup(
            false,
            &UpdateState::default(),
            now
        ));
        // Enabled: only when the window has elapsed.
        assert!(!should_check_on_startup(true, &fresh, now));
        assert!(should_check_on_startup(true, &stale, now));
        assert!(should_check_on_startup(true, &UpdateState::default(), now));
    }

    #[test]
    fn should_notify_skips_only_the_skipped_version() {
        let check = available_check("v1.2.0");

        // Nothing skipped → notify.
        assert!(should_notify(&check, None));
        // Exactly this version skipped → stay silent, with or without the `v`.
        assert!(!should_notify(&check, Some("v1.2.0")));
        assert!(!should_notify(&check, Some("1.2.0")));
        // Build metadata is not part of precedence (SemVer §10).
        assert!(!should_notify(&check, Some("1.2.0+ci.7")));
        // A newer release than the skipped one prompts again — skipping is
        // per-version, not "never tell me about updates".
        assert!(should_notify(&available_check("v1.3.0"), Some("v1.2.0")));
        assert!(should_notify(&check, Some("v1.1.0")));
        // A non-semver tag can still be skipped, by exact (case-insensitive) name.
        assert!(!should_notify(&available_check("nightly"), Some("NIGHTLY")));
    }

    #[test]
    fn should_notify_stays_silent_when_there_is_nothing_to_offer() {
        // Up to date: never notify, whatever the skip state says.
        let up_to_date = UpdateCheck {
            available: false,
            ..available_check("v1.2.0")
        };
        assert!(!should_notify(&up_to_date, None));

        // `available` without a `latest` is a shape that cannot happen today
        // (`evaluate` sets them together) — treat it as nothing to say rather
        // than prompting for an unnamed version.
        let unnamed = UpdateCheck {
            latest: None,
            ..available_check("v1.2.0")
        };
        assert!(!should_notify(&unnamed, None));
    }

    #[test]
    fn update_at_merges_into_what_is_on_disk_now() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("update-state.json");

        // Someone else wrote a skip while we were holding an older copy.
        UpdateState {
            last_check_at: None,
            skipped_version: Some("v1.2.0".to_string()),
        }
        .save_to(&path)
        .unwrap();

        let saved = UpdateState::update_at(&path, |s| {
            s.last_check_at = Some("2026-07-20T12:00:00Z".into())
        })
        .unwrap();
        assert_eq!(saved.skipped_version.as_deref(), Some("v1.2.0"));
        assert_eq!(UpdateState::load_from(&path), saved);
    }

    #[test]
    fn fr_084_a_skip_chosen_during_the_startup_fetch_is_not_lost() {
        // The defect: the startup thread loaded the state, spent seconds on a
        // network call, then saved its stale copy — undoing a "skip this
        // version" the user chose from the About screen in that window.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("update-state.json");
        UpdateState::default().save_to(&path).unwrap();

        let notified = run_startup_check_with(
            true,
            &path,
            at("2026-07-20T12:00:00Z"),
            "1.0.0",
            // Stands in for the seconds the real fetch takes: the other process
            // writes the skip while this call is inside `fetch`.
            || {
                UpdateState::update_at(&path, |s| s.skipped_version = Some("v1.2.0".into()))
                    .unwrap();
                Ok(Some(ReleaseInfo {
                    tag_name: "v1.2.0".to_string(),
                    html_url: "https://x/y".to_string(),
                    body: None,
                }))
            },
        );

        assert!(
            notified.is_none(),
            "the version the user just skipped must not be announced"
        );
        let stored = UpdateState::load_from(&path);
        assert_eq!(
            stored.skipped_version.as_deref(),
            Some("v1.2.0"),
            "the skip survived the check that overlapped it"
        );
        assert_eq!(
            stored.last_check_at.as_deref(),
            Some("2026-07-20T12:00:00Z"),
            "…and the check still stamped its own field"
        );
    }

    #[test]
    fn startup_check_constants_match_fr_080() {
        // FR-080: "on startup (delayed by 30 seconds) and at most once per 24
        // hours". These two constants are the requirement; the scheduler that
        // eventually consumes them must not carry its own numbers.
        assert_eq!(STARTUP_CHECK_DELAY, std::time::Duration::from_secs(30));
        assert_eq!(MIN_CHECK_INTERVAL_HOURS, 24);
    }

    /// Reads the real published manifests. Ignored by default (network);
    /// run explicitly with `cargo test -- --ignored live_manifest_check`.
    /// While the repository is private both answer 404, which reads as
    /// "nothing published" (`Ok(None)`), never as an error.
    #[test]
    #[ignore = "network: reads the published update manifests"]
    fn live_manifest_check() {
        use crate::settings::UpdateChannel;
        for channel in [UpdateChannel::Stable, UpdateChannel::Beta] {
            let url = install::resolve_endpoint(None, channel).unwrap();
            let result = fetch_manifest(&url, false);
            assert!(result.is_ok(), "{channel:?}: {result:?}");
        }
    }

    #[test]
    fn fr_084_a_skip_at_or_below_the_running_version_is_stale() {
        // Skipped v0.2.0, then installed v0.3.0 by hand: the skip is history.
        let mut state = UpdateState {
            last_check_at: None,
            skipped_version: Some("v0.2.0".into()),
        };
        assert!(state.prune_stale_skip("0.3.0"));
        assert_eq!(state.skipped_version, None);

        // Skipping the very version now running is stale too.
        let mut state = UpdateState {
            last_check_at: None,
            skipped_version: Some("0.3.0".into()),
        };
        assert!(state.prune_stale_skip("0.3.0"));
        assert_eq!(state.skipped_version, None);
    }

    #[test]
    fn fr_084_a_skip_newer_than_the_running_version_is_kept() {
        let mut state = UpdateState {
            last_check_at: Some("2026-07-01T00:00:00Z".into()),
            skipped_version: Some("v0.4.0".into()),
        };
        assert!(!state.prune_stale_skip("0.3.0"));
        assert_eq!(state.skipped_version.as_deref(), Some("v0.4.0"));

        // A tag we cannot parse is left alone rather than guessed at.
        let mut state = UpdateState {
            last_check_at: None,
            skipped_version: Some("nightly-42".into()),
        };
        assert!(!state.prune_stale_skip("0.3.0"));
        assert!(!UpdateState::default().prune_stale_skip("0.3.0"));
    }

    #[test]
    fn fr_084_loading_for_the_ui_clears_a_stale_skip_on_disk() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("update-state.json");
        UpdateState {
            last_check_at: Some("2026-07-01T00:00:00Z".into()),
            skipped_version: Some("v0.0.0".into()),
        }
        .save_to(&path)
        .unwrap();

        let state = UpdateState::load_current_from(&path, "0.0.1");
        assert_eq!(state.skipped_version, None);
        assert_eq!(state.last_check_at.as_deref(), Some("2026-07-01T00:00:00Z"));
        assert_eq!(
            UpdateState::load_from(&path).skipped_version,
            None,
            "the stale skip is removed from the file, not just hidden"
        );
    }
}
