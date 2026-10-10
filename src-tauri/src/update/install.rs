//! Signed update download + install (FR-082, FR-083, FR-084 "install now",
//! FR-087). See ADR-0008.
//!
//! The heavy lifting is `tauri-plugin-updater`: it reads the release manifest
//! (`latest.json`), downloads the artifact for this platform, verifies its
//! minisign signature against the public key embedded in `tauri.conf.json`
//! (`plugins.updater.pubkey`) **before** anything touches the disk, and runs
//! the platform installer — the per-user NSIS setup on Windows (no admin,
//! FR-083), an in-place `.app` swap on macOS.
//!
//! This module owns the policy around it, as pure functions with co-located
//! tests, so the GUI command ([`crate::ipc::update::install_update`]) and the
//! CLI (`pluryprint update install`) apply exactly the same rules:
//!
//! - [`resolve_endpoint`] — the manifest URL, overridable through
//!   [`ENDPOINT_ENV`] for local end-to-end testing.
//! - [`is_major_bump`] — FR-087: a major version jump is never installed
//!   without an explicit, separate opt-in.
//! - [`check_installable_location`] — refuse to "update" a binary that is not
//!   an installed app (a dev build in `target/`), which on macOS would replace
//!   the directory the binary happens to live in.
//! - [`classify_error`] — a tampered or unsigned artifact surfaces as
//!   [`IpcError::UpdateSignatureInvalid`] with a message that says nothing was
//!   installed, never as a generic failure.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use serde::Serialize;
use specta::Type;
use tauri::Url;
use tauri_plugin_updater::UpdaterExt;

use crate::util::error::{IpcError, IpcErrorCode};

/// Where releases publish their updater manifest. `releases/latest/download`
/// always redirects to the newest non-prerelease, so the URL never changes.
pub const DEFAULT_ENDPOINT: &str =
    "https://github.com/MAECLY/maecly-print/releases/latest/download/latest.json";

/// Environment variable that replaces [`DEFAULT_ENDPOINT`], for testing an
/// update end to end against a local server. Release builds still require
/// `https` (the plugin refuses plain `http` outside debug builds). The
/// deprecated `MAECLYPRINT_UPDATE_ENDPOINT` is still read, with a warning.
pub const ENDPOINT_ENV: &str = "PLURYPRINT_UPDATE_ENDPOINT";

/// The manifest URL: `override_value` when it is set and non-blank, the
/// default otherwise. Pure.
pub fn resolve_endpoint(override_value: Option<&str>) -> Result<Url, String> {
    let raw = override_value
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .unwrap_or(DEFAULT_ENDPOINT);
    let url = Url::parse(raw).map_err(|e| format!("invalid update endpoint '{raw}': {e}"))?;
    match url.scheme() {
        "https" | "http" => Ok(url),
        other => Err(format!(
            "invalid update endpoint '{raw}': unsupported scheme '{other}'"
        )),
    }
}

/// [`resolve_endpoint`] against the real environment.
pub fn endpoint() -> Result<Url, String> {
    resolve_endpoint(crate::util::legacy::env_var(ENDPOINT_ENV).as_deref())
}

/// FR-087: whether going from `current` to `candidate` crosses a **major**
/// version (`1.x` → `2.x`). Pure.
///
/// Such an update is never installed on the strength of a single "install"
/// action: the GUI asks for a second, explicit confirmation and the CLI
/// requires `--yes` or an interactive "yes". An unparseable version on either
/// side counts as a major bump — when in doubt, ask.
pub fn is_major_bump(current: &str, candidate: &str) -> bool {
    let parse = |s: &str| semver::Version::parse(s.trim().trim_start_matches(['v', 'V']));
    match (parse(current), parse(candidate)) {
        (Ok(cur), Ok(new)) => new.major > cur.major,
        _ => true,
    }
}

/// FR-087's gate, pure: may `candidate` be installed over `current`?
///
/// - A major jump needs `allow_major`.
/// - A major jump also has to be the version the user confirmed. The About
///   screen asks about the release the GitHub API named, then the install
///   reads the update manifest again; if that now names another version (a
///   newer major published in between, or a manifest that differs from the
///   API), the confirmation was not for it. `confirmed` is that version, or
///   `None` where the opt-in is blanket (the CLI's `--yes`).
///
/// A minor or patch update needs no confirmation, so its version is not
/// compared: a manifest that runs ahead of the API must not block it.
pub fn check_confirmation(
    current: &str,
    candidate: &str,
    allow_major: bool,
    confirmed: Option<&str>,
) -> Result<(), IpcError> {
    if !is_major_bump(current, candidate) {
        return Ok(());
    }
    if !allow_major {
        return Err(IpcError::localized(
            IpcErrorCode::InvalidRequest,
            "major_update_unconfirmed",
            format!(
                "{candidate} is a major update from {current}; it is only installed after you confirm it explicitly"
            ),
        )
        .with_param("version", candidate)
        .with_param("current", current));
    }
    let bare = |v: &str| v.trim().trim_start_matches(['v', 'V']).to_string();
    match confirmed {
        Some(confirmed) if bare(confirmed) != bare(candidate) => Err(IpcError::localized(
            IpcErrorCode::InvalidRequest,
            "major_update_changed",
            format!(
                "you confirmed {confirmed}, but the update available now is {candidate}; check for updates again and confirm that version"
            ),
        )
        .with_param("version", candidate)
        .with_param("confirmed", confirmed)),
        _ => Ok(()),
    }
}

/// Refuse to install over something that is not an installed copy of the app.
/// Pure (`os` is `std::env::consts::OS`).
///
/// On macOS the updater replaces the `.app` bundle the executable lives in; a
/// binary that is *not* inside `<Name>.app/Contents/MacOS/` (a `cargo run`
/// build) would have its whole parent directory swapped out instead. Windows
/// runs the per-user installer, which installs to its own location, so any
/// executable can hand over to it.
pub fn check_installable_location(os: &str, exe: &Path) -> Result<(), String> {
    if os != "macos" {
        return Ok(());
    }
    let in_bundle = exe
        .parent()
        .filter(|macos| macos.file_name().is_some_and(|n| n == "MacOS"))
        .and_then(Path::parent)
        .filter(|contents| contents.file_name().is_some_and(|n| n == "Contents"))
        .and_then(Path::parent)
        .and_then(Path::extension)
        .is_some_and(|ext| ext == "app");
    if in_bundle {
        Ok(())
    } else {
        Err(format!(
            "this copy of Pluryprint ({}) is not an installed app bundle, so it cannot \
             update itself; download the release from the releases page instead",
            exe.display()
        ))
    }
}

/// The user-facing message for an artifact that failed verification.
pub const SIGNATURE_REJECTED: &str =
    "the downloaded update failed its signature check and was NOT installed. \
     It may have been tampered with or corrupted in transit; nothing on this \
     computer was changed. Try again later, or download the release from the \
     releases page";

/// Map an updater failure onto the IPC vocabulary. Pure.
///
/// Every way the signature can be wrong — bad signature, wrong key, a
/// signature that is not even valid base64, a version that does not match
/// what was signed — is the same thing to the user: the file is not trusted
/// and was not installed. That gets its own code so the UI can say so plainly.
pub fn classify_error(err: &tauri_plugin_updater::Error) -> IpcError {
    use tauri_plugin_updater::Error as E;
    match err {
        E::Minisign(_)
        | E::Base64(_)
        | E::SignatureUtf8(_)
        | E::SignedVersionMismatch { .. }
        | E::MissingSignedVersion => IpcError::localized(
            IpcErrorCode::UpdateSignatureInvalid,
            "signature_rejected",
            format!("{SIGNATURE_REJECTED} ({err})"),
        ),
        E::Reqwest(_) | E::Network(_) => IpcError::localized(
            IpcErrorCode::UpdateInstallFailed,
            "download_failed",
            format!("could not download the update — check your connection ({err})"),
        ),
        E::ReleaseNotFound | E::Serialization(_) | E::TargetNotFound(_) | E::TargetsNotFound(_) => {
            IpcError::localized(
                IpcErrorCode::UpdateInstallFailed,
                "no_platform_update",
                format!("the release does not offer an update for this platform yet ({err})"),
            )
        }
        other => IpcError::UpdateInstallFailed(other.to_string()),
    }
}

/// What an install attempt did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct UpdateInstallResult {
    /// The version that was installed, or `None` when there was nothing newer.
    pub installed: Option<String>,
    /// The new version runs only after a restart (macOS). On Windows the
    /// installer restarts the app itself and this process exits.
    pub restart_required: bool,
}

/// Download progress, pushed while an install is running.
#[derive(Debug, Clone, Serialize, Type, tauri_specta::Event)]
pub struct UpdateDownloadProgress {
    /// Bytes received so far.
    pub downloaded: u64,
    /// Total size, when the server sent a `Content-Length`.
    pub total: Option<u64>,
}

/// One install at a time per process: a double click must not download twice
/// or run two installers.
static INSTALLING: AtomicBool = AtomicBool::new(false);

struct InstallGuard;

impl InstallGuard {
    fn acquire() -> Result<Self, IpcError> {
        if INSTALLING.swap(true, Ordering::SeqCst) {
            return Err(IpcError::localized(
                IpcErrorCode::InvalidState,
                "update_in_progress",
                "an update is already being installed",
            ));
        }
        Ok(Self)
    }
}

impl Drop for InstallGuard {
    fn drop(&mut self) {
        INSTALLING.store(false, Ordering::SeqCst);
    }
}

/// Ask the manifest whether a newer, installable release exists. For the GUI:
/// on Windows the installer relaunches the app when it is done.
pub async fn find_update<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> Result<Option<tauri_plugin_updater::Update>, IpcError> {
    find_update_with(app, Relaunch::App, || {}).await
}

/// What the Windows installer starts once it has installed an update.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Relaunch {
    /// The GUI, with the arguments it was started with (the plugin default).
    App,
    /// Nothing. For the headless CLI: the plugin would otherwise replay the
    /// CLI's own argv (`update install --yes`) through the installer's
    /// `/ARGS`, starting a second, invisible update run instead of the app.
    Nothing,
}

/// [`find_update`] for a given [`Relaunch`] choice, with `before_exit` run
/// right before the process hands over to the installer and exits.
///
/// On Windows `tauri-plugin-updater` ends `Update::install` with
/// `std::process::exit(0)`, so code after [`install`] never runs there and
/// neither do destructors. `before_exit` is the last moment to report a result
/// (the CLI prints it there), and the log is flushed after it so the buffered
/// lines describing the install are not lost with the process.
pub async fn find_update_with<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    relaunch: Relaunch,
    before_exit: impl Fn() + Send + Sync + 'static,
) -> Result<Option<tauri_plugin_updater::Update>, IpcError> {
    let url = endpoint().map_err(IpcError::UpdateInstallFailed)?;
    let updater = app
        .updater_builder()
        .endpoints(vec![url])
        .and_then(|b| {
            b.restart_after_install(relaunch == Relaunch::App)
                .on_before_exit(move || {
                    tracing::info!(
                        event = "update_installer_handoff",
                        "exiting for the installer"
                    );
                    before_exit();
                    crate::logging::flush();
                })
                .build()
        })
        .map_err(|e| classify_error(&e))?;
    updater.check().await.map_err(|e| classify_error(&e))
}

/// Download, verify and install `update`, reporting progress through
/// `on_progress(downloaded, total)`.
///
/// `allow_major` is FR-087's explicit opt-in and `confirmed` the version it
/// was given for ([`check_confirmation`]); a major bump that fails either is
/// refused before a single byte is downloaded. On Windows a successful
/// install hands over to the installer and **this process exits**; elsewhere
/// the function returns and the caller offers a restart.
pub async fn install<F>(
    update: &tauri_plugin_updater::Update,
    allow_major: bool,
    confirmed: Option<&str>,
    mut on_progress: F,
) -> Result<UpdateInstallResult, IpcError>
where
    F: FnMut(u64, Option<u64>),
{
    let _guard = InstallGuard::acquire()?;

    check_confirmation(
        &update.current_version,
        &update.version,
        allow_major,
        confirmed,
    )?;

    let exe = std::env::current_exe()
        .map_err(|e| IpcError::UpdateInstallFailed(format!("cannot locate the app: {e}")))?;
    check_installable_location(std::env::consts::OS, &exe).map_err(|msg| {
        IpcError::localized(
            IpcErrorCode::UpdateInstallFailed,
            "not_an_installed_app",
            msg,
        )
    })?;

    tracing::info!(
        event = "update_install_started",
        from = %update.current_version,
        to = %update.version
    );
    let mut downloaded: u64 = 0;
    let bytes = update
        .download(
            |chunk, total| {
                downloaded += chunk as u64;
                on_progress(downloaded, total);
            },
            || {},
        )
        .await
        .map_err(|e| {
            let mapped = classify_error(&e);
            tracing::error!(event = "update_download_rejected", error = %e);
            mapped
        })?;

    // Only verified bytes reach this point: `download` checks the signature
    // and returns an error instead of the payload when it does not match.
    update.install(bytes).map_err(|e| {
        tracing::error!(event = "update_install_failed", error = %e);
        classify_error(&e)
    })?;

    tracing::info!(event = "update_installed", version = %update.version);
    Ok(UpdateInstallResult {
        installed: Some(update.version.clone()),
        restart_required: !cfg!(windows),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn fr_082_endpoint_defaults_to_the_github_latest_manifest() {
        assert_eq!(resolve_endpoint(None).unwrap().as_str(), DEFAULT_ENDPOINT);
        assert_eq!(
            resolve_endpoint(Some("   ")).unwrap().as_str(),
            DEFAULT_ENDPOINT
        );
        assert!(DEFAULT_ENDPOINT.starts_with(env!("CARGO_PKG_REPOSITORY")));
    }

    #[test]
    fn fr_082_endpoint_can_be_overridden_for_local_testing() {
        let url = resolve_endpoint(Some(" http://127.0.0.1:8765/latest.json ")).unwrap();
        assert_eq!(url.as_str(), "http://127.0.0.1:8765/latest.json");
        assert!(resolve_endpoint(Some("not a url")).is_err());
        assert!(resolve_endpoint(Some("file:///etc/passwd")).is_err());
    }

    #[test]
    fn fr_087_only_a_major_jump_is_a_major_bump() {
        assert!(!is_major_bump("1.2.3", "1.3.0"));
        assert!(!is_major_bump("1.2.3", "v1.2.4"));
        assert!(!is_major_bump("0.0.1", "0.9.0"));
        assert!(is_major_bump("1.9.9", "2.0.0"));
        assert!(is_major_bump("v0.4.0", "1.0.0"));
        // When in doubt, ask.
        assert!(is_major_bump("1.0.0", "nightly"));
    }

    /// FR-087: the second confirmation belongs to the version it named. A
    /// newer major (or a manifest that disagrees with the API) installs only
    /// after its own confirmation.
    #[test]
    fn fr_087_a_major_confirmation_covers_only_the_confirmed_version() {
        // Confirmed exactly what is offered (either spelling): fine.
        assert!(check_confirmation("1.4.0", "2.0.0", true, Some("2.0.0")).is_ok());
        assert!(check_confirmation("1.4.0", "2.0.0", true, Some("v2.0.0")).is_ok());

        // The user said yes to 2.0.0; 3.0.0 is what the manifest names now.
        let err = check_confirmation("1.4.0", "3.0.0", true, Some("2.0.0")).unwrap_err();
        assert_eq!(err.reason(), Some("major_update_changed"));

        // No opt-in at all.
        let err = check_confirmation("1.4.0", "2.0.0", false, None).unwrap_err();
        assert_eq!(err.reason(), Some("major_update_unconfirmed"));

        // The CLI's `--yes` is a blanket opt-in.
        assert!(check_confirmation("1.4.0", "3.0.0", true, None).is_ok());

        // A minor update needs no confirmation, whatever was on screen.
        assert!(check_confirmation("1.4.0", "1.5.1", false, Some("1.5.0")).is_ok());
    }

    #[test]
    fn fr_083_macos_refuses_to_update_a_binary_outside_an_app_bundle() {
        let bundled = PathBuf::from("/Applications/Pluryprint.app/Contents/MacOS/pluryprint");
        assert!(check_installable_location("macos", &bundled).is_ok());
        let user = PathBuf::from("/Users/a/Applications/Pluryprint.app/Contents/MacOS/pluryprint");
        assert!(check_installable_location("macos", &user).is_ok());

        let dev = PathBuf::from("/Users/a/src/maecly-print/src-tauri/target/debug/pluryprint");
        let err = check_installable_location("macos", &dev).unwrap_err();
        assert!(err.contains("not an installed app bundle"), "{err}");

        // Windows hands over to the per-user installer wherever the exe is.
        let win = PathBuf::from(r"C:\dev\target\debug\pluryprint.exe");
        assert!(check_installable_location("windows", &win).is_ok());
    }

    #[test]
    fn fr_082_a_bad_signature_is_reported_as_such() {
        use tauri_plugin_updater::Error as E;
        for err in [
            E::SignatureUtf8("zzz".into()),
            E::MissingSignedVersion,
            E::SignedVersionMismatch {
                signed: "1.0.0".into(),
                announced: "2.0.0".into(),
            },
        ] {
            let got = classify_error(&err);
            assert_eq!(got.code(), IpcErrorCode::UpdateSignatureInvalid, "{err:?}");
            assert_eq!(got.reason(), Some("signature_rejected"));
            assert!(got.message().contains("NOT installed"), "{got:?}");
        }
        let network = classify_error(&E::Network("503".into()));
        assert_eq!(network.code(), IpcErrorCode::UpdateInstallFailed);
        assert_eq!(network.reason(), Some("download_failed"));
        let missing = classify_error(&E::ReleaseNotFound);
        assert_eq!(missing.code(), IpcErrorCode::UpdateInstallFailed);
        assert_eq!(missing.reason(), Some("no_platform_update"));
    }

    /// The shipped configuration is what makes FR-082/083 true at all: a public
    /// key to verify against, signed updater artifacts, the documented
    /// endpoint, and a per-user (no admin) Windows installer.
    #[test]
    fn fr_083_shipped_config_signs_updates_and_installs_per_user() {
        let conf: serde_json::Value =
            serde_json::from_str(include_str!("../../tauri.conf.json")).unwrap();
        let updater = &conf["plugins"]["updater"];

        let pubkey = updater["pubkey"].as_str().expect("an updater public key");
        use base64::Engine as _;
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(pubkey)
            .expect("the public key is base64");
        let text = String::from_utf8(decoded).unwrap();
        assert!(text.contains("minisign public key"), "{text}");

        assert_eq!(updater["endpoints"][0], DEFAULT_ENDPOINT);
        assert_eq!(conf["bundle"]["createUpdaterArtifacts"], true);
        assert_eq!(
            conf["bundle"]["windows"]["nsis"]["installMode"],
            "currentUser"
        );
        assert!(
            updater.get("dangerousInsecureTransportProtocol").is_none(),
            "release builds must keep refusing plain-http endpoints"
        );
    }

    /// ADR-0008: the per-user NSIS setup is the only Windows installer. With
    /// `"targets": "all"` the bundler also built a per-machine WiX MSI, which
    /// installs a second copy under Program Files that NSIS updates never
    /// reach.
    #[test]
    fn fr_083_no_per_machine_msi_is_bundled() {
        let conf: serde_json::Value =
            serde_json::from_str(include_str!("../../tauri.conf.json")).unwrap();
        let targets = conf["bundle"]["targets"]
            .as_array()
            .expect("an explicit target list, never \"all\"");
        let names: Vec<&str> = targets.iter().filter_map(|t| t.as_str()).collect();
        assert!(names.contains(&"nsis"), "{names:?}");
        assert!(!names.contains(&"msi"), "{names:?}");
        assert!(
            names.contains(&"app") && names.contains(&"dmg"),
            "{names:?}"
        );
    }

    #[test]
    fn fr_082_only_one_install_runs_at_a_time() {
        let first = InstallGuard::acquire().unwrap();
        let busy = InstallGuard::acquire()
            .err()
            .expect("second install refused");
        assert_eq!(busy.code(), IpcErrorCode::InvalidState);
        assert_eq!(busy.reason(), Some("update_in_progress"));
        drop(first);
        assert!(InstallGuard::acquire().is_ok());
    }
}
