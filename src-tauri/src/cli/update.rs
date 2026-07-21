//! `maeclyprint update …`
//!
//! `check` is the CLI face of [`crate::ipc::update::check_for_updates`]: the
//! same pure [`crate::update::evaluate`] over the same GitHub `releases/latest`
//! feed. `channel` reads/writes `updater.channel` through the settings core.
//!
//! `install` is specified (§3.2) but the installer path is M4 work — there is
//! no `install_update` command behind the IPC surface either. Rather than
//! pretend, it reports the documented "update install failed" code with a
//! message that says why.

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
                .map_err(|e| CliError::from(IpcError::UpdateCheckFailed(e)))?;
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

        UpdateCmd::Install { yes } => {
            let _ = yes; // nothing to confirm while there is nothing to install
            Err(CliError::new(
                ExitCode::UpdateInstallFailed,
                IpcError::Internal(
                    "installing updates is not available in this version; \
                     run `update check` and follow the release link"
                        .to_string(),
                ),
            ))
        }

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
    fn fr_009_install_reports_the_documented_code() {
        let dir = tempfile::tempdir().unwrap();
        let err = run(
            &paths_in(dir.path()),
            Emitter::new(true),
            UpdateCmd::Install { yes: true },
        )
        .unwrap_err();
        assert_eq!(err.exit, ExitCode::UpdateInstallFailed);
    }
}
