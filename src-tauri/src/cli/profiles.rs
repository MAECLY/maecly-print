//! `maeclyprint profiles …`
//!
//! A facade over [`crate::ipc::profiles`]' path-injected cores — the exact
//! functions the Tauri commands call — so the CLI inherits every rule the UI
//! has (built-ins are immutable, names are unique, the default cannot be
//! deleted) without restating any of them.
//!
//! The one thing the CLI adds is [`resolve`]: the IPC surface addresses
//! profiles by id, but a person on a command line types the name they see in
//! the app.

use crate::cli::args::ProfilesCmd;
use crate::cli::exit::{CliError, ExitCode};
use crate::cli::output::{table, Emitter};
use crate::cli::Paths;
use crate::ipc::profiles::{
    delete_profile_at, list_profiles_at, save_profile_at, set_default_profile_at,
};
use crate::profiles::{Profile, ProfileDuplex};
use crate::queue::job::ColorMode;
use crate::util::error::IpcError;

/// Run a `profiles` subcommand.
pub fn run(paths: &Paths, emitter: Emitter, cmd: ProfilesCmd) -> Result<(), CliError> {
    match cmd {
        ProfilesCmd::List => {
            let list = list_profiles_at(&paths.profiles, &paths.config)?;
            emitter.emit(&list, || render(&list));
            Ok(())
        }
        ProfilesCmd::Show { name_or_id } => {
            let profile = resolve(paths, &name_or_id)?;
            emitter.emit(&profile, || render(std::slice::from_ref(&profile)));
            Ok(())
        }
        ProfilesCmd::Create {
            name,
            from,
            printer,
            quality,
            tray,
            settings,
        } => {
            // `--from` clones an existing profile; without it we start from the
            // built-in "Quick" shape so a bare `create --name X` is meaningful.
            let base = match &from {
                Some(id) => resolve(paths, id)?,
                None => crate::profiles::builtins().remove(0),
            };
            let mut profile = Profile {
                // An empty id makes `save_profile_at` mint a fresh UUID v7.
                id: String::new(),
                name,
                builtin: false,
                default: false,
                ..base
            };
            if let Some(printer) = printer {
                profile.printer = printer;
            }
            if let Some(q) = quality {
                profile.quality = q.to_profile();
            }
            if let Some(tray) = tray {
                profile.tray = tray;
            }
            if let Some(mode) = settings.color_mode() {
                profile.color_mode = mode;
            }
            if let Some(d) = settings.duplex {
                profile.duplex = d.to_profile();
            }
            if let Some(c) = settings.copies {
                profile.copies = c;
            }
            if let Some(p) = settings.paper {
                profile.paper_size = p.to_engine();
            }
            if let Some(s) = settings.scale {
                profile.scaling = s.to_engine();
            }
            if let Some(o) = settings.orientation {
                profile.orientation = o.to_engine();
            }
            if let Some(c) = settings.collate() {
                profile.collate = c;
            }
            if let Some(r) = settings.range {
                profile.page_range = Some(r);
            }

            let saved = save_profile_at(&paths.profiles, &paths.config, profile)?;
            emitter.emit(&saved, || format!("Created profile \"{}\"", saved.name));
            Ok(())
        }
        ProfilesCmd::Delete { name_or_id } => {
            let profile = resolve(paths, &name_or_id)?;
            delete_profile_at(&paths.profiles, &paths.config, profile.id.clone())?;
            emitter.emit(&serde_json::json!({ "deleted": profile.id }), || {
                format!("Deleted profile \"{}\"", profile.name)
            });
            Ok(())
        }
        ProfilesCmd::SetDefault { name_or_id } => {
            let profile = resolve(paths, &name_or_id)?;
            set_default_profile_at(&paths.profiles, &paths.config, profile.id.clone())?;
            emitter.emit(&serde_json::json!({ "default": profile.id }), || {
                format!("\"{}\" is now the default profile", profile.name)
            });
            Ok(())
        }
    }
}

/// Find a profile by id, or failing that by name (case-insensitively).
///
/// An id match always wins, so a profile perversely *named* like another's id
/// can never shadow it. An ambiguous name is a usage error rather than an
/// arbitrary pick — printing 500 pages with the wrong profile is expensive.
pub fn resolve(paths: &Paths, name_or_id: &str) -> Result<Profile, CliError> {
    let all = list_profiles_at(&paths.profiles, &paths.config)?;
    let needle = name_or_id.trim();

    if let Some(hit) = all.iter().find(|p| p.id == needle) {
        return Ok(hit.clone());
    }

    let by_name: Vec<&Profile> = all
        .iter()
        .filter(|p| p.name.eq_ignore_ascii_case(needle))
        .collect();
    match by_name.as_slice() {
        [one] => Ok((*one).clone()),
        [] => Err(IpcError::ProfileNotFound(needle.to_string()).into()),
        many => Err(CliError::new(
            ExitCode::Profile,
            IpcError::InvalidRequest(format!(
                "\"{needle}\" matches {} profiles; use the id instead",
                many.len()
            )),
        )),
    }
}

/// The app's default profile, used when `print` is given no `--profile`.
pub fn default_profile(paths: &Paths) -> Result<Profile, CliError> {
    let all = list_profiles_at(&paths.profiles, &paths.config)?;
    all.into_iter()
        .find(|p| p.default)
        // `list_profiles_at` always stamps exactly one default (falling back to
        // the built-in "Quick"), so this arm is defensive only.
        .ok_or_else(|| IpcError::ProfileNotFound("<default>".to_string()).into())
}

/// Human-mode rendering: one row per profile.
fn render(profiles: &[Profile]) -> String {
    let rows: Vec<Vec<String>> = profiles
        .iter()
        .map(|p| {
            vec![
                p.name.clone(),
                p.id.clone(),
                match p.color_mode {
                    ColorMode::Color => "color".to_string(),
                    ColorMode::Monochrome => "mono".to_string(),
                },
                match p.duplex {
                    ProfileDuplex::Off => "off".to_string(),
                    ProfileDuplex::LongEdge => "long-edge".to_string(),
                    ProfileDuplex::ShortEdge => "short-edge".to_string(),
                },
                p.copies.to_string(),
                p.printer.clone(),
                match (p.builtin, p.default) {
                    (true, true) => "built-in, default".to_string(),
                    (true, false) => "built-in".to_string(),
                    (false, true) => "default".to_string(),
                    _ => String::new(),
                },
            ]
        })
        .collect();
    table(
        &[
            "NAME", "ID", "COLOR", "DUPLEX", "COPIES", "PRINTER", "FLAGS",
        ],
        &rows,
    )
}
