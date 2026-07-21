//! The clap surface: subcommands, flags and their value vocabularies.
//!
//! This module is *declaration only* — no I/O, no business rules — so the whole
//! command surface can be parsed and asserted in tests without touching the
//! machine. It mirrors `docs/04-architecture/ipc-and-cli.md` §3.2/§3.3 plus the
//! two entry points the shell extensions need (`--invoke=context-menu` and
//! `--files-from`, see [`super::handoff`]).
//!
//! Flag names are public surface (§5 of the same doc): add, deprecate, but do
//! not rename.

use std::path::PathBuf;

use clap::{ArgGroup, Args, Parser, Subcommand, ValueEnum};
use serde::{Deserialize, Serialize};

use crate::profiles::{ProfileDuplex, Quality};
use crate::queue::job::{ColorMode, Duplex, Orientation, PaperSize, Scaling};

/// Subcommand names that switch the binary into headless mode when they appear
/// as the first non-flag argument (§3.1). Kept as data so the argv classifier
/// and the parser can never disagree.
pub const SUBCOMMANDS: &[&str] = &["print", "printers", "profiles", "queue", "update"];

/// `maeclyprint` — the whole command line.
#[derive(Debug, Parser, PartialEq, Eq)]
#[command(
    name = "maeclyprint",
    version,
    about = "Batch file printing for Windows and macOS.",
    long_about = "Batch file printing for Windows and macOS.\n\nRun without arguments to open the app; run a subcommand to work headlessly \
(no window is created). Exit codes are documented in docs/04-architecture/ipc-and-cli.md."
)]
pub struct Cli {
    /// How this process was started. `context-menu` opens the app with the
    /// supplied selection instead of running headlessly.
    #[arg(long, value_name = "SOURCE")]
    pub invoke: Option<InvokeSource>,

    /// Read the file selection from a JSON handoff file instead of argv. This
    /// is how the shell extensions pass a large selection (argv is capped at
    /// 32,767 characters on Windows).
    #[arg(long, value_name = "PATH")]
    pub files_from: Option<PathBuf>,

    /// What to do with a `--invoke=context-menu` selection.
    #[arg(long, value_name = "ACTION", default_value_t = ContextAction::Configure)]
    pub action: ContextAction,

    /// Profile (name or id) applied to a `--invoke=context-menu` selection.
    #[arg(long, value_name = "NAME-OR-ID")]
    pub profile: Option<String>,

    /// Force headless mode even without a subcommand.
    #[arg(long)]
    pub cli: bool,

    /// Emit machine-readable JSON on stdout (errors as JSON on stderr).
    #[arg(long, global = true)]
    pub json: bool,

    #[command(subcommand)]
    pub command: Option<Command>,
}

/// Entry points that are not a plain terminal invocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum InvokeSource {
    /// Launched by the Windows Explorer / macOS Finder integration.
    ContextMenu,
}

/// What a context-menu invocation asks for.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Default, ValueEnum, Serialize, Deserialize, specta::Type,
)]
#[serde(rename_all = "kebab-case")]
pub enum ContextAction {
    /// Open the print dialog pre-loaded with the selection.
    #[default]
    Configure,
    /// Print immediately with the default (or `--profile`) profile.
    PrintDefault,
    /// Add the selection to the queue without starting it.
    Queue,
}

impl std::fmt::Display for ContextAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            ContextAction::Configure => "configure",
            ContextAction::PrintDefault => "print-default",
            ContextAction::Queue => "queue",
        };
        f.write_str(s)
    }
}

#[derive(Debug, Subcommand, PartialEq, Eq)]
pub enum Command {
    /// Print one or more files.
    Print(PrintArgs),
    /// Inspect the printers installed on this machine.
    Printers {
        #[command(subcommand)]
        cmd: PrintersCmd,
    },
    /// Manage print profiles.
    Profiles {
        #[command(subcommand)]
        cmd: ProfilesCmd,
    },
    /// Inspect and control the persisted print queue.
    Queue {
        #[command(subcommand)]
        cmd: QueueCmd,
    },
    /// Check for a newer MaeclyPrint release.
    Update {
        #[command(subcommand)]
        cmd: UpdateCmd,
    },
}

/// `maeclyprint print …` — exactly one input source is required.
#[derive(Debug, Args, PartialEq, Eq)]
#[command(group(ArgGroup::new("input").required(true).args(["files", "files_from"])))]
pub struct PrintArgs {
    /// Files to print: a comma-separated list of paths, directories or
    /// wildcard patterns (`"*.pdf"`). Patterns are expanded by MaeclyPrint, so
    /// they work identically in cmd.exe and in a POSIX shell.
    #[arg(long, value_name = "FILES", value_delimiter = ',', num_args = 1..)]
    pub files: Vec<String>,

    /// Read the file list from a JSON handoff file (see `--invoke`).
    #[arg(long, value_name = "PATH")]
    pub files_from: Option<PathBuf>,

    /// Profile (name or id) to base the settings on; defaults to the app's
    /// default profile.
    #[arg(long, value_name = "NAME-OR-ID")]
    pub profile: Option<String>,

    /// Target printer; defaults to the app printer, then the OS default.
    #[arg(long, value_name = "NAME")]
    pub printer: Option<String>,

    #[command(flatten)]
    pub settings: SettingFlags,

    /// Accepted for compatibility: a headless `print` never opens a window, so
    /// this is already the behaviour.
    #[arg(long)]
    pub no_dialog: bool,

    /// Accepted for compatibility: a headless `print` always runs each file to
    /// completion before exiting.
    #[arg(long)]
    pub wait: bool,
}

/// The print-setting overrides shared by `print` and `profiles create`.
#[derive(Debug, Args, PartialEq, Eq, Default)]
pub struct SettingFlags {
    /// Print in colour.
    #[arg(long, conflicts_with = "mono")]
    pub color: bool,

    /// Print in grayscale.
    #[arg(long)]
    pub mono: bool,

    /// Double-sided mode.
    #[arg(long, value_name = "MODE", ignore_case = true)]
    pub duplex: Option<CliDuplex>,

    /// Number of copies (1-999).
    #[arg(long, value_name = "N")]
    pub copies: Option<u16>,

    /// Paper size.
    #[arg(long, value_name = "SIZE", ignore_case = true)]
    pub paper: Option<CliPaper>,

    /// Page range, e.g. `1-5,8`.
    #[arg(long, value_name = "SPEC")]
    pub range: Option<String>,

    /// How pages are fitted to the paper.
    #[arg(long, value_name = "MODE", ignore_case = true)]
    pub scale: Option<CliScale>,

    /// Page orientation.
    #[arg(long, value_name = "MODE", ignore_case = true)]
    pub orientation: Option<CliOrientation>,
}

#[derive(Debug, Subcommand, PartialEq, Eq)]
pub enum PrintersCmd {
    /// List every printer the OS spooler knows about.
    List,
}

#[derive(Debug, Subcommand, PartialEq, Eq)]
pub enum ProfilesCmd {
    /// List every profile, built-ins first.
    List,
    /// Show one profile.
    Show {
        /// Profile name or id.
        name_or_id: String,
    },
    /// Create a custom profile.
    Create {
        /// Name of the new profile (must be unique).
        #[arg(long, value_name = "NAME")]
        name: String,
        /// Existing profile (name or id) to copy the settings from.
        #[arg(long, value_name = "NAME-OR-ID")]
        from: Option<String>,
        /// Target printer for the profile.
        #[arg(long, value_name = "NAME")]
        printer: Option<String>,
        /// Print-quality preset.
        #[arg(long, value_name = "LEVEL", ignore_case = true)]
        quality: Option<CliQuality>,
        /// Input tray.
        #[arg(long, value_name = "TRAY")]
        tray: Option<String>,
        #[command(flatten)]
        settings: SettingFlags,
    },
    /// Delete a custom profile.
    Delete {
        /// Profile name or id.
        name_or_id: String,
    },
    /// Make a profile the app default.
    SetDefault {
        /// Profile name or id.
        name_or_id: String,
    },
}

#[derive(Debug, Subcommand, PartialEq, Eq)]
pub enum QueueCmd {
    /// Show the persisted queue.
    Status {
        /// Re-print the queue whenever it changes (Ctrl-C to stop).
        #[arg(long)]
        watch: bool,
    },
    /// Cancel one job.
    Cancel {
        /// Job id.
        id: String,
    },
    /// Cancel every job that has not reached a terminal state.
    CancelAll,
}

#[derive(Debug, Subcommand, PartialEq, Eq)]
pub enum UpdateCmd {
    /// Ask GitHub whether a newer release exists.
    Check,
    /// Download and install an update (not available in this version).
    Install {
        /// Skip the confirmation prompt.
        #[arg(long)]
        yes: bool,
    },
    /// Show or set the update channel.
    Channel {
        /// New channel; omit to print the current one.
        #[arg(value_name = "CHANNEL", ignore_case = true)]
        channel: Option<CliChannel>,
    },
}

// ---------------------------------------------------------------------------
// Value vocabularies
//
// These exist because the CLI spelling and the wire spelling differ on purpose:
// `--duplex off|long-edge|short-edge` is the profile vocabulary users already
// see in the UI, while the engine's `Duplex` is `none|long|short`; `--scale
// actual` reads better than the engine's `none`. Each enum owns its mapping so
// the translation lives in exactly one place.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum CliDuplex {
    Off,
    LongEdge,
    ShortEdge,
}

impl CliDuplex {
    pub fn to_engine(self) -> Duplex {
        match self {
            CliDuplex::Off => Duplex::None,
            CliDuplex::LongEdge => Duplex::Long,
            CliDuplex::ShortEdge => Duplex::Short,
        }
    }

    pub fn to_profile(self) -> ProfileDuplex {
        match self {
            CliDuplex::Off => ProfileDuplex::Off,
            CliDuplex::LongEdge => ProfileDuplex::LongEdge,
            CliDuplex::ShortEdge => ProfileDuplex::ShortEdge,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum CliPaper {
    A4,
    Letter,
    Legal,
    A3,
}

impl CliPaper {
    pub fn to_engine(self) -> PaperSize {
        match self {
            CliPaper::A4 => PaperSize::A4,
            CliPaper::Letter => PaperSize::Letter,
            CliPaper::Legal => PaperSize::Legal,
            CliPaper::A3 => PaperSize::A3,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum CliScale {
    Fit,
    Shrink,
    /// The engine's `none`: print at the source's actual size.
    Actual,
}

impl CliScale {
    pub fn to_engine(self) -> Scaling {
        match self {
            CliScale::Fit => Scaling::Fit,
            CliScale::Shrink => Scaling::Shrink,
            CliScale::Actual => Scaling::None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum CliOrientation {
    Auto,
    Portrait,
    Landscape,
}

impl CliOrientation {
    pub fn to_engine(self) -> Orientation {
        match self {
            CliOrientation::Auto => Orientation::Auto,
            CliOrientation::Portrait => Orientation::Portrait,
            CliOrientation::Landscape => Orientation::Landscape,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum CliQuality {
    Draft,
    Normal,
    High,
}

impl CliQuality {
    pub fn to_profile(self) -> Quality {
        match self {
            CliQuality::Draft => Quality::Draft,
            CliQuality::Normal => Quality::Normal,
            CliQuality::High => Quality::High,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum CliChannel {
    Stable,
    Beta,
    Nightly,
}

impl CliChannel {
    pub fn to_settings(self) -> crate::settings::UpdateChannel {
        use crate::settings::UpdateChannel;
        match self {
            CliChannel::Stable => UpdateChannel::Stable,
            CliChannel::Beta => UpdateChannel::Beta,
            CliChannel::Nightly => UpdateChannel::Nightly,
        }
    }
}

impl SettingFlags {
    /// The colour override these flags express, if any. `--color` and `--mono`
    /// are mutually exclusive (enforced by clap), so at most one is set.
    pub fn color_mode(&self) -> Option<ColorMode> {
        match (self.color, self.mono) {
            (true, _) => Some(ColorMode::Color),
            (_, true) => Some(ColorMode::Monochrome),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// clap's own consistency checks (duplicate flags, bad groups, …). Failing
    /// this means the declaration above is malformed, which would otherwise
    /// only surface as a runtime panic in front of a user.
    #[test]
    fn fr_112_command_surface_is_well_formed() {
        use clap::CommandFactory;
        Cli::command().debug_assert();
    }

    #[test]
    fn fr_008_vocabularies_map_onto_the_engine() {
        assert_eq!(CliDuplex::Off.to_engine(), Duplex::None);
        assert_eq!(CliDuplex::LongEdge.to_engine(), Duplex::Long);
        assert_eq!(CliDuplex::ShortEdge.to_engine(), Duplex::Short);
        // `actual` is the engine's `none` — the one deliberate rename.
        assert_eq!(CliScale::Actual.to_engine(), Scaling::None);
        assert_eq!(CliPaper::A4.to_engine(), PaperSize::A4);
        assert_eq!(CliOrientation::Auto.to_engine(), Orientation::Auto);
    }

    #[test]
    fn fr_008_color_flags_are_exclusive_and_optional() {
        assert_eq!(SettingFlags::default().color_mode(), None);
        let color = SettingFlags {
            color: true,
            ..Default::default()
        };
        assert_eq!(color.color_mode(), Some(ColorMode::Color));
        let mono = SettingFlags {
            mono: true,
            ..Default::default()
        };
        assert_eq!(mono.color_mode(), Some(ColorMode::Monochrome));
    }
}
