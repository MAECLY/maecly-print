//! `maeclyprint update …`
//!
//! `check` is the CLI face of [`crate::ipc::update::check_for_updates`]: the
//! same pure [`crate::update::evaluate`] over the same GitHub `releases/latest`
//! feed. `channel` reads/writes `updater.channel` through the settings core.
//!
//! `install` is the CLI face of [`crate::ipc::update::install_update`]: the
//! same [`crate::update::install`] core (manifest, signature verification,
//! per-user install, FR-087's major-version rule), driven through a windowless
//! Tauri app because the updater needs an `AppHandle`. Confirmation is the one
//! CLI-only concern: `--yes` skips it, an interactive terminal is asked, and a
//! non-interactive run without `--yes` is refused rather than guessed.

use crate::cli::args::UpdateCmd;
use crate::cli::exit::{CliError, ExitCode};
use crate::cli::output::Emitter;
use crate::cli::Paths;
use crate::ipc::settings::{get_app_settings_at, set_app_settings_at};
use crate::util::error::IpcError;

/// Run an `update` subcommand.
pub fn run(paths: &Paths, emitter: Emitter, cmd: UpdateCmd) -> Result<(), CliError> {
    match cmd {
        UpdateCmd::Check => {
            let (owner, repo) = crate::update::repo_slug().ok_or_else(|| {
                CliError::from(IpcError::UpdateCheckFailed(
                    "repository not configured".to_string(),
                ))
            })?;
            let release = crate::update::fetch_latest(&owner, &repo)
                .map_err(|e| CliError::from(IpcError::from(e)))?;
            let check = crate::update::evaluate(
                env!("CARGO_PKG_VERSION"),
                release,
                crate::queue::job::now_rfc3339(),
            );
            emitter.emit(&check, || match (&check.available, &check.latest) {
                (true, Some(latest)) => format!(
                    "Update available: {latest} (running {})\n{}",
                    check.current,
                    check.url.clone().unwrap_or_default()
                ),
                _ => format!("Up to date ({})", check.current),
            });
            Ok(())
        }

        UpdateCmd::Install { yes } => install(emitter, yes),

        UpdateCmd::Channel { channel } => {
            let mut settings = get_app_settings_at(&paths.config)?;
            match channel {
                None => {
                    let current = settings.updater.channel;
                    emitter.emit(&serde_json::json!({ "channel": current }), || {
                        format!("Update channel: {}", channel_name(current))
                    });
                }
                Some(new) => {
                    settings.updater.channel = new.to_settings();
                    let saved = set_app_settings_at(&paths.config, &paths.profiles, settings)?;
                    let channel = saved.updater.channel;
                    emitter.emit(&serde_json::json!({ "channel": channel }), || {
                        format!("Update channel set to {}", channel_name(channel))
                    });
                }
            }
            Ok(())
        }
    }
}

/// How `update install` obtains the user's go-ahead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Confirmation {
    /// `--yes`: proceed, majors included (the flag is the explicit opt-in).
    Granted,
    /// Ask on the terminal.
    Prompt,
    /// No `--yes` and nobody to ask: refuse instead of guessing.
    Refused,
}

/// Decide how to confirm an install. Pure.
pub fn confirmation(yes: bool, interactive: bool) -> Confirmation {
    match (yes, interactive) {
        (true, _) => Confirmation::Granted,
        (false, true) => Confirmation::Prompt,
        (false, false) => Confirmation::Refused,
    }
}

/// The question asked before installing; a major jump says so (FR-087).
pub fn prompt_text(current: &str, version: &str, major: bool) -> String {
    if major {
        format!(
            "MaeclyPrint {version} is a MAJOR update from {current} and may change how the app \
             works. Install it now? [y/N] "
        )
    } else {
        format!("Install MaeclyPrint {version} (running {current}) now? [y/N] ")
    }
}

/// Whether a typed answer is a yes. Pure.
pub fn is_yes(answer: &str) -> bool {
    matches!(
        answer.trim().to_ascii_lowercase().as_str(),
        "y" | "yes" | "s" | "si" | "sí"
    )
}

/// `update install [--yes]`.
fn install(emitter: Emitter, yes: bool) -> Result<(), CliError> {
    use std::io::{IsTerminal, Write};

    use crate::update::install as core;

    let app = crate::headless_updater_app().map_err(|e| {
        CliError::from(IpcError::UpdateInstallFailed(format!(
            "could not start the updater: {e}"
        )))
    })?;
    let handle = app.handle().clone();

    let Some(update) = tauri::async_runtime::block_on(core::find_update(&handle))? else {
        let none = core::UpdateInstallResult {
            installed: None,
            restart_required: false,
        };
        emitter.emit(&none, || {
            format!(
                "Up to date ({}); nothing to install",
                env!("CARGO_PKG_VERSION")
            )
        });
        return Ok(());
    };

    let major = core::is_major_bump(&update.current_version, &update.version);
    match confirmation(yes, std::io::stdin().is_terminal()) {
        Confirmation::Granted => {}
        Confirmation::Refused => {
            return Err(CliError::from(IpcError::InvalidRequest(format!(
                "MaeclyPrint {} is available; re-run with --yes to install it without a prompt",
                update.version
            ))));
        }
        Confirmation::Prompt => {
            eprint!(
                "{}",
                prompt_text(&update.current_version, &update.version, major)
            );
            let _ = std::io::stderr().flush();
            let mut answer = String::new();
            let _ = std::io::stdin().read_line(&mut answer);
            if !is_yes(&answer) {
                return Err(CliError::new(
                    ExitCode::Cancelled,
                    IpcError::InvalidState("update not installed: cancelled".to_string()),
                ));
            }
        }
    }

    if !emitter.json {
        eprintln!(
            "Downloading MaeclyPrint {} and verifying its signature…",
            update.version
        );
    }
    // Reaching this point *is* the explicit opt-in FR-087 asks for: `--yes`
    // (blanket, so no version is pinned) or a typed "yes" to a prompt that
    // named this very version.
    let confirmed = (!yes).then_some(update.version.as_str());
    let result =
        tauri::async_runtime::block_on(core::install(&update, true, confirmed, |_, _| {}))?;
    emitter.emit(&result, || {
        match (&result.installed, result.restart_required) {
            (Some(v), true) => format!("Installed MaeclyPrint {v}; restart the app to use it"),
            (Some(v), false) => format!("Installing MaeclyPrint {v}"),
            (None, _) => "Nothing to install".to_string(),
        }
    });
    Ok(())
}

/// The wire spelling of a channel, matching `config.toml` and the bindings.
fn channel_name(channel: crate::settings::UpdateChannel) -> &'static str {
    use crate::settings::UpdateChannel;
    match channel {
        UpdateChannel::Stable => "stable",
        UpdateChannel::Beta => "beta",
        UpdateChannel::Nightly => "nightly",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::args::CliChannel;
    use crate::settings::{Config, UpdateChannel};

    fn paths_in(dir: &std::path::Path) -> Paths {
        Paths {
            config: dir.join("config.toml"),
            profiles: dir.join("profiles.toml"),
            queue: dir.join("queue.json"),
        }
    }

    #[test]
    fn fr_008_channel_round_trips_through_config() {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths_in(dir.path());
        Config::default().save_to(&paths.config).unwrap();

        run(
            &paths,
            Emitter::new(true),
            UpdateCmd::Channel {
                channel: Some(CliChannel::Beta),
            },
        )
        .unwrap();

        assert_eq!(
            Config::load_from(&paths.config).updater.channel,
            UpdateChannel::Beta
        );
    }

    #[test]
    fn fr_087_install_needs_an_explicit_yes() {
        assert_eq!(confirmation(true, false), Confirmation::Granted);
        assert_eq!(confirmation(true, true), Confirmation::Granted);
        assert_eq!(confirmation(false, true), Confirmation::Prompt);
        // A script without --yes is refused, never silently allowed.
        assert_eq!(confirmation(false, false), Confirmation::Refused);
    }

    #[test]
    fn fr_087_the_prompt_calls_out_a_major_update() {
        assert!(prompt_text("1.4.0", "2.0.0", true).contains("MAJOR"));
        assert!(!prompt_text("1.4.0", "1.5.0", false).contains("MAJOR"));
        assert!(prompt_text("1.4.0", "1.5.0", false).ends_with("[y/N] "));
    }

    #[test]
    fn fr_087_only_an_explicit_answer_is_a_yes() {
        for yes in ["y", "Y", "yes", " YES \n", "s", "sí"] {
            assert!(is_yes(yes), "{yes:?}");
        }
        for no in ["", "\n", "n", "no", "maybe", "yess"] {
            assert!(!is_yes(no), "{no:?}");
        }
    }

    #[test]
    fn fr_009_install_failures_use_the_documented_code() {
        let signature: CliError = IpcError::UpdateSignatureInvalid("tampered".into()).into();
        assert_eq!(signature.exit, ExitCode::UpdateInstallFailed);
        let network: CliError = IpcError::UpdateInstallFailed("offline".into()).into();
        assert_eq!(network.exit, ExitCode::UpdateInstallFailed);
    }
}
