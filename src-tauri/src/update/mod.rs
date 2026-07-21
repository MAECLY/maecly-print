//! Update check against the project's GitHub "latest release".
//!
//! This is a *check*, not an updater: it asks the GitHub Releases API for the
//! latest published release, compares its tag to the running version with
//! semver, and reports whether something newer exists plus a link to it. There
//! is deliberately no download or install path here — the user follows the link.
//!
//! Everything except the one network call is a pure function
//! ([`repo_slug`]/[`parse_release`]/[`is_newer`]/[`evaluate`]) with co-located
//! tests; [`fetch_latest`] is the only part that touches the network and is kept
//! tiny so it is easy to reason about offline.

use serde::Serialize;
use specta::Type;

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
    pub current: String,
    pub latest: Option<String>,
    pub available: bool,
    pub url: Option<String>,
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

/// Parse the JSON body of GitHub's `releases/latest` into a [`ReleaseInfo`].
/// Requires `tag_name`; tolerates a missing/empty `html_url` or `body`. Returns
/// `None` on any structural problem so the caller degrades gracefully.
pub fn parse_release(json: &str) -> Option<ReleaseInfo> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let tag_name = v.get("tag_name")?.as_str()?.to_string();
    if tag_name.is_empty() {
        return None;
    }
    let html_url = v
        .get("html_url")
        .and_then(|u| u.as_str())
        .unwrap_or_default()
        .to_string();
    let body = v
        .get("body")
        .and_then(|b| b.as_str())
        .map(str::to_string)
        .filter(|s| !s.is_empty());
    Some(ReleaseInfo {
        tag_name,
        html_url,
        body,
    })
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

/// Map a GitHub `releases/latest` response (`status` + `body`) to a result,
/// independent of the HTTP transport so the whole decision table is unit-tested
/// offline. [`fetch_latest`] is the thin wrapper that performs the request and
/// feeds its outcome here.
///
/// - `200` with a parseable release → `Ok(Some(_))`. A `200` whose body does
///   **not** parse is an error (`Err`), not `Ok(None)`: reporting "you're up to
///   date" off an unreadable payload would be a lie.
/// - `404` → `Ok(None)`: the repo has no releases yet — or does not exist at
///   all, which GitHub reports identically. [`fetch_latest`] disambiguates the
///   two with [`interpret_repo_probe`].
/// - `403` / `429` → rate-limit error.
/// - anything else → an "unexpected status" error.
pub fn interpret_response(status: u16, body: &str) -> Result<Option<ReleaseInfo>, String> {
    match status {
        200 => match parse_release(body) {
            Some(r) => Ok(Some(r)),
            None => Err("unexpected response from GitHub — try again later".to_string()),
        },
        // No releases published yet — a normal, non-error state.
        404 => Ok(None),
        403 | 429 => Err("GitHub rate limit reached — try again later".to_string()),
        other => Err(format!(
            "github.com returned an unexpected status ({other})"
        )),
    }
}

/// Map the status of the `GET /repos/{owner}/{repo}` probe that disambiguates a
/// `404` from `releases/latest`.
///
/// - `404` → the configured repository does not exist: the build's
///   `repository` slug is wrong, and reporting "you're up to date" off that
///   would hide a broken configuration behind reassuring copy.
/// - anything else (including a rate-limited `403`) → inconclusive, treated as
///   "the repo exists, it just has no releases yet", so a transient hiccup
///   never turns a normal state into an error.
pub fn interpret_repo_probe(status: u16) -> Result<(), String> {
    if status == 404 {
        return Err(
            "the configured repository was not found on github.com — this build's \
             repository URL is wrong"
                .to_string(),
        );
    }
    Ok(())
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

/// GET a GitHub API URL and return `(status, body)`.
///
/// A non-2xx status is delivered by ureq as an error carrying the response, so
/// it is unwrapped here and callers see a single shape; only a transport-level
/// failure (DNS, connection) is an `Err`.
fn get_api(agent: &ureq::Agent, url: &str) -> Result<(u16, String), String> {
    let user_agent = format!("MaeclyPrint/{}", env!("CARGO_PKG_VERSION"));
    let response = agent
        .get(url)
        .set("User-Agent", &user_agent)
        .set("Accept", "application/vnd.github+json")
        .set("X-GitHub-Api-Version", "2022-11-28")
        .call();

    match response {
        Ok(resp) => {
            let status = resp.status();
            let body = resp
                .into_string()
                .map_err(|_| "could not read the response from github.com".to_string())?;
            Ok((status, body))
        }
        Err(ureq::Error::Status(code, resp)) => Ok((code, resp.into_string().unwrap_or_default())),
        Err(ureq::Error::Transport(_)) => {
            Err("could not reach github.com — check your connection".to_string())
        }
    }
}

/// Fetch the latest published release for `owner/repo` from the GitHub API.
///
/// A thin transport wrapper: it performs the request (a 10-second timeout keeps
/// a hung request from blocking the off-thread command) and hands the status +
/// body to [`interpret_response`], which owns the status→result mapping.
///
/// When that yields "no release" — a `404` — a second request probes the
/// repository itself, because GitHub answers `404` both for "no releases yet"
/// and for "no such repository". Without it a wrong `repository` slug would
/// render as a reassuring "You're up to date" forever. The probe only runs on
/// that one branch, so the happy path is still a single request.
pub fn fetch_latest(owner: &str, repo: &str) -> Result<Option<ReleaseInfo>, String> {
    let agent = build_agent();
    let (status, body) = get_api(
        &agent,
        &format!("https://api.github.com/repos/{owner}/{repo}/releases/latest"),
    )?;
    let release = interpret_response(status, &body)?;

    if release.is_none() {
        let (probe_status, _) = get_api(
            &agent,
            &format!("https://api.github.com/repos/{owner}/{repo}"),
        )?;
        interpret_repo_probe(probe_status)?;
    }

    Ok(release)
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn parse_release_full_payload() {
        let json = r#"{
            "tag_name": "v0.2.0",
            "html_url": "https://github.com/MAECLY/maecly-print/releases/tag/v0.2.0",
            "body": "What is new - Faster printing"
        }"#;
        let r = parse_release(json).unwrap();
        assert_eq!(r.tag_name, "v0.2.0");
        assert_eq!(
            r.html_url,
            "https://github.com/MAECLY/maecly-print/releases/tag/v0.2.0"
        );
        assert!(r.body.unwrap().contains("Faster printing"));
    }

    #[test]
    fn parse_release_minimal_payload_tolerates_missing_body() {
        let json = r#"{"tag_name": "v0.2.0", "html_url": "https://x/y", "body": null}"#;
        let r = parse_release(json).unwrap();
        assert_eq!(r.tag_name, "v0.2.0");
        assert!(r.body.is_none());

        // Missing body key entirely, and empty body, both → None.
        let r2 = parse_release(r#"{"tag_name":"1.0.0","html_url":"https://x/y"}"#).unwrap();
        assert!(r2.body.is_none());
        let r3 = parse_release(r#"{"tag_name":"1.0.0","html_url":"","body":""}"#).unwrap();
        assert!(r3.body.is_none());

        // Missing the required tag → None.
        assert!(parse_release(r#"{"html_url":"https://x/y"}"#).is_none());
        assert!(parse_release("not json").is_none());
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
    fn interpret_response_maps_status_and_parse_outcomes() {
        let ok_body = r#"{"tag_name":"v0.2.0","html_url":"https://x/y"}"#;

        // 200 + parseable → Some.
        let r = interpret_response(200, ok_body).unwrap().unwrap();
        assert_eq!(r.tag_name, "v0.2.0");

        // 200 + unparseable body is an ERROR, not Ok(None) ("up to date").
        assert!(interpret_response(200, "not json at all").is_err());
        assert!(interpret_response(200, r#"{"no_tag":true}"#).is_err());

        // 404 → no releases yet (Ok(None)).
        assert_eq!(interpret_response(404, "").unwrap(), None);

        // Rate limiting.
        assert!(interpret_response(403, "").is_err());
        assert!(interpret_response(429, "").is_err());

        // Any other status is an unexpected-status error naming the code.
        let err = interpret_response(500, "").unwrap_err();
        assert!(err.contains("500"), "message names the status: {err}");
    }

    #[test]
    fn interpret_repo_probe_only_fails_on_a_missing_repository() {
        // A 404 on the repository itself means the configured slug is wrong;
        // without this the same 404 would render as "You're up to date".
        let err = interpret_repo_probe(404).unwrap_err();
        assert!(err.contains("repository"), "message names the cause: {err}");

        // Everything else is inconclusive and must not manufacture an error out
        // of a repository that simply has no releases yet.
        assert!(interpret_repo_probe(200).is_ok());
        assert!(interpret_repo_probe(403).is_ok()); // rate limited
        assert!(interpret_repo_probe(500).is_ok()); // GitHub hiccup
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

    /// Hits the real GitHub API. Ignored by default (network + rate limits);
    /// run explicitly with `cargo test -- --ignored live_github_release_check`.
    #[test]
    #[ignore = "network: hits api.github.com"]
    fn live_github_release_check() {
        let (owner, repo) = repo_slug().expect("repo configured");
        let result = fetch_latest(&owner, &repo);
        // Either a release, no releases (404 → None), or a friendly error — but
        // never a panic.
        assert!(result.is_ok() || result.is_err());
    }
}
