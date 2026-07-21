//! Diagnostic & "About" IPC commands.
//!
//! [`app_info`] is the canary that proves the React ↔ Rust round-trip works and
//! that filesystem paths resolve on the host OS; it also carries the
//! external-dependency probe (FR-058), so a screen that needs to know whether
//! SumatraPDF is installed asks the same command it already asks for the
//! version. The rest back the About/Help
//! surface: opening the logs folder, prefilling a GitHub issue, and opening an
//! allow-listed external link. All three shell out via the `opener` plugin
//! Rust-side only, so no JS `opener` capability is needed.

use serde::Serialize;
use specta::Type;
use tauri_plugin_opener::OpenerExt;

use crate::engine::deps::{self, DependencyReport};
use crate::util::{error::IpcError, paths};

#[derive(Debug, Clone, Serialize, Type)]
pub struct AppInfo {
    pub version: String,
    pub build_date: String,
    pub config_dir: String,
    pub log_dir: String,
    pub data_dir: String,
    /// Target OS, e.g. `"macos"`, `"windows"`, `"linux"` (`std::env::consts::OS`).
    pub os: String,
    /// Target architecture, e.g. `"x86_64"`, `"aarch64"` (`std::env::consts::ARCH`).
    pub arch: String,
    /// FR-058: every external program the print pipeline can shell out to,
    /// with where it was found (or the remediation message for a missing one).
    ///
    /// It rides on `app_info` rather than a command of its own because this is
    /// exactly what `app_info` is for — "what does this build see on this
    /// host?" — and because the screens that care already fetch it once on
    /// mount. The probe itself is a handful of `is_file` calls (see
    /// [`deps::report_all`]), so it costs nothing to answer on every call and
    /// is never stale.
    ///
    /// `#[serde(default)]` — as on `AppSettings::meta`, and for the same
    /// reason: the server always sends it, but a test building an `AppInfo` by
    /// hand should not have to enumerate the machine's dependencies to talk
    /// about a version string. Consumers treat an absent list as "not probed",
    /// which raises no warning.
    #[serde(default)]
    pub dependencies: Vec<DependencyReport>,
}

#[tauri::command]
#[specta::specta]
pub fn app_info() -> Result<AppInfo, IpcError> {
    Ok(AppInfo {
        version: env!("CARGO_PKG_VERSION").to_string(),
        build_date: env!("BUILD_DATE").to_string(),
        config_dir: paths::config_dir().display().to_string(),
        log_dir: paths::log_dir().display().to_string(),
        data_dir: paths::data_dir().display().to_string(),
        os: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
        dependencies: deps::report_all(),
    })
}

/// Reveal the app's log directory in the OS file browser (creating it first if
/// it does not exist yet, so the command never no-ops on a fresh install).
#[tauri::command]
#[specta::specta]
pub fn open_logs_folder(app: tauri::AppHandle) -> Result<(), IpcError> {
    let dir = paths::log_dir();
    paths::ensure_dir(&dir)?;
    app.opener()
        .open_path(dir.to_string_lossy().into_owned(), None::<&str>)
        .map_err(|e| IpcError::Internal(format!("could not open the logs folder: {e}")))
}

/// Open a prefilled "new issue" page on the project's GitHub, seeding the body
/// with the app version, build date, and OS/arch followed by the user's text.
#[tauri::command]
#[specta::specta]
pub fn report_issue(app: tauri::AppHandle, text: String) -> Result<(), IpcError> {
    let url = issue_url(
        env!("CARGO_PKG_REPOSITORY"),
        env!("CARGO_PKG_VERSION"),
        env!("BUILD_DATE"),
        std::env::consts::OS,
        std::env::consts::ARCH,
        &text,
    );
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|e| IpcError::Internal(format!("could not open the issue page: {e}")))
}

/// Open an external link in the default browser, restricted to an allow-list of
/// project hosts (github.com, maecly.com, and `*.maecly.com` over HTTPS). Any
/// other URL is rejected with [`IpcError::InvalidRequest`] rather than opened.
#[tauri::command]
#[specta::specta]
pub fn open_external(app: tauri::AppHandle, url: String) -> Result<(), IpcError> {
    if !allowed_external(&url) {
        return Err(IpcError::InvalidRequest(format!(
            "refusing to open a non-allowlisted URL: {url}"
        )));
    }
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|e| IpcError::Internal(format!("could not open the link: {e}")))
}

/// Build the prefilled GitHub issue URL. `repo` is the `https://github.com/o/r`
/// base; the body is percent-encoded (`NON_ALPHANUMERIC`) so newlines, spaces
/// and `#` survive intact in the query string. Pure, for testing.
fn issue_url(
    repo: &str,
    version: &str,
    build_date: &str,
    os: &str,
    arch: &str,
    text: &str,
) -> String {
    use percent_encoding::{utf8_percent_encode, NON_ALPHANUMERIC};

    let body = format!(
        "- MaeclyPrint: {version}\n- Build date: {build_date}\n- OS: {os} {arch}\n\n{text}"
    );
    let enc = |s: &str| utf8_percent_encode(s, NON_ALPHANUMERIC).to_string();
    format!("{repo}/issues/new?title={}&body={}", enc(""), enc(&body))
}

/// Whether `url` is an allow-listed external link: HTTPS to exactly
/// `github.com`, `maecly.com`, or any `*.maecly.com` subdomain. The host is
/// parsed by hand (rather than pulling in a URL crate) and any `userinfo@` or
/// `:port` in the authority is rejected, so tricks like
/// `https://github.com@evil.example` or `https://github.com.evil.com` do not
/// slip through. Pure, for testing.
fn allowed_external(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://") else {
        return false;
    };
    // The authority is everything up to the first path/query/fragment marker.
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..end];
    // A bare host only: no userinfo and no port.
    if authority.contains('@') || authority.contains(':') {
        return false;
    }
    let host = authority.to_ascii_lowercase();
    host == "github.com" || host == "maecly.com" || host.ends_with(".maecly.com")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn issue_url_encodes_spaces_newlines_and_hash() {
        let url = issue_url(
            env!("CARGO_PKG_REPOSITORY"),
            "0.0.1",
            "2026-07-12",
            "macos",
            "aarch64",
            "crash on #3 with two files",
        );
        assert!(url.starts_with("https://github.com/MAECLY/maecly-print/issues/new?title=&body="));
        // Spaces → %20, newlines → %0A, '#' → %23 (NON_ALPHANUMERIC encodes all).
        assert!(url.contains("%20"), "spaces encoded: {url}");
        assert!(url.contains("%0A"), "newlines encoded: {url}");
        assert!(url.contains("%23"), "'#' encoded: {url}");
        // The environment block is present (percent-encoded).
        assert!(url.contains("MaeclyPrint"));
        assert!(!url.contains(' '), "no raw spaces in the URL");
    }

    #[test]
    fn allowed_external_accepts_project_hosts() {
        assert!(allowed_external("https://github.com/MAECLY/maecly-print"));
        assert!(allowed_external("https://github.com"));
        assert!(allowed_external("https://maecly.com/"));
        assert!(allowed_external("https://sub.maecly.com/path?q=1#frag"));
        assert!(allowed_external("https://GitHub.com/MAECLY")); // case-insensitive host
    }

    /// Every link the About screen can open is derived from the configured
    /// repository URL (mirrored frontend-side in `src/lib/repo.ts`), so the
    /// allow-list has to accept it exactly as configured — capitalized owner
    /// segment included. The rule is host-only and the host is lowercased before
    /// comparing, so the path's case never matters.
    #[test]
    fn allowed_external_accepts_the_configured_repository_links() {
        let repo = env!("CARGO_PKG_REPOSITORY");
        assert!(allowed_external(repo));
        assert!(allowed_external(&format!("{repo}/releases")));
        assert!(allowed_external(&format!("{repo}/blob/main/LICENSE")));
        assert!(allowed_external(&format!("{repo}/tree/main/docs")));
        assert!(allowed_external(&format!("{repo}/releases/tag/v9.9.9")));
        assert!(allowed_external(&issue_url(
            repo,
            "0.0.1",
            "2026-07-12",
            "windows",
            "x86_64",
            "hello"
        )));
    }

    #[test]
    fn allowed_external_rejects_everything_else() {
        let repo = env!("CARGO_PKG_REPOSITORY");
        // The real repository over plain http, or on a look-alike host, is still
        // refused: fixing the slug must not widen the rule.
        assert!(!allowed_external(&repo.replace("https://", "http://")));
        assert!(!allowed_external(
            &repo.replace("github.com", "github.com.evil.com")
        ));
        assert!(!allowed_external(&repo.replace(
            "https://github.com",
            "https://github.com@evil.example"
        )));
        assert!(!allowed_external("http://github.com")); // not https
        assert!(!allowed_external("https://evil-github.com"));
        assert!(!allowed_external("https://github.com.evil.com"));
        assert!(!allowed_external("https://user@github.com")); // userinfo trick
        assert!(!allowed_external("https://github.com:8080")); // port
        assert!(!allowed_external("https://notmaecly.com"));
        assert!(!allowed_external("ftp://maecly.com"));
        assert!(!allowed_external("https://"));
        // Only the two project hosts are allowed — not the API host the update
        // check talks to, and not raw.githubusercontent.com.
        assert!(!allowed_external("https://api.github.com/repos/MAECLY"));
        assert!(!allowed_external(
            "https://raw.githubusercontent.com/MAECLY"
        ));
    }
}
