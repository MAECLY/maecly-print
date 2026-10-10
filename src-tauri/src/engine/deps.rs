//! External-dependency detection (FR-058).
//!
//! The hybrid engine (ADR-0003) shells out to programs we do not compile: the
//! final PDF printer on Windows (SumatraPDF) and the best-of-breed converters
//! the architecture reserves for Office/HTML/Markdown (LibreOffice, a
//! Chromium-class browser, Pandoc). None of them is guaranteed to exist on a
//! user's machine, so every one of them is *resolved* before it is *run*, and a
//! missing one produces a remediation message instead of an opaque
//! "failed to spawn" deep inside a job.
//!
//! # Shape
//!
//! - [`resolve`] is the whole decision and it is **pure**: it takes the
//!   candidate directories, the executable names, and an `exists` predicate,
//!   and returns where the program was found. It reads no environment variable
//!   and touches no global state, so the search order is testable against a
//!   tempdir on any OS.
//! - [`bundled_dirs`], [`path_dirs`] and [`install_dirs`] gather that input from
//!   the real machine. Only [`install_dirs`] needs environment variables, and it
//!   takes the lookup as a parameter for the same reason.
//! - [`report`] / [`report_all`] glue the two together and produce the typed
//!   [`DependencyReport`] the IPC layer ships to the UI.
//!
//! # Search order (identical for every dependency)
//!
//! 1. **Bundled** — next to our own executable, in the layout the installer
//!    produces (see `bundle.resources` in `tauri.conf.json`). A copy we ship
//!    always wins: it is the version we tested against.
//! 2. **Configured** — an explicit path the user pointed us at. There is no
//!    such setting in `config.toml` today, so callers pass `None`; the slot
//!    exists because the resolver is where the precedence has to live once one
//!    is added, and adding it later must not reshuffle the other three steps.
//! 3. **`PATH`** — the user's own install, however they manage it.
//! 4. **Well-known install directories** — the per-OS defaults the vendors'
//!    installers use.
//!
//! # Honesty
//!
//! [`Spec::blocks`] lists exactly the extensions that cannot print while this
//! dependency is missing — always a subset of
//! [`crate::engine::SUPPORTED_EXTS`], and taken from the converter routing
//! tables in [`crate::engine::convert`] so the two cannot drift. The
//! remediation text names the dependency and how to install it; the converter
//! returns the same text as `DependencyMissing` when a job needs it (FR-058).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use specta::Type;

/// The operating system a resolution is being computed for.
///
/// An explicit parameter rather than `cfg!`, so the Windows search order can be
/// unit-tested from a macOS dev machine (and vice versa) — the whole point of
/// keeping [`resolve`] pure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "lowercase")]
pub enum Os {
    /// Microsoft Windows.
    Windows,
    /// Apple macOS.
    MacOs,
    /// Linux (CUPS, like macOS).
    Linux,
}

impl Os {
    /// The OS this build targets, or [`Os::Linux`] for anything else Unix-y
    /// (the engine's Unix path is CUPS either way).
    pub fn current() -> Self {
        if cfg!(windows) {
            Os::Windows
        } else if cfg!(target_os = "macos") {
            Os::MacOs
        } else {
            Os::Linux
        }
    }
}

/// An external program the print pipeline can depend on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Type)]
#[serde(rename_all = "kebab-case")]
pub enum Dependency {
    /// The final PDF printer on Windows (ADR-0003). Not used elsewhere: macOS
    /// and Linux print through CUPS `lp`.
    SumatraPdf,
    /// Office → PDF converter (`soffice --headless --convert-to pdf`).
    LibreOffice,
    /// HTML → PDF converter: Edge or Chrome, whichever is installed.
    Chromium,
    /// Markdown → PDF converter.
    Pandoc,
}

/// Every dependency, in the order the UI should list them.
pub const ALL: &[Dependency] = &[
    Dependency::SumatraPdf,
    Dependency::LibreOffice,
    Dependency::Chromium,
    Dependency::Pandoc,
];

/// Which step of the search order produced the hit. Shown in diagnostics so
/// "it picked up the wrong copy" is answerable without a debugger.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "kebab-case")]
pub enum FoundIn {
    /// A copy shipped with the app, next to our executable.
    Bundled,
    /// A path the user configured explicitly.
    Configured,
    /// A `PATH` entry.
    Path,
    /// A well-known install directory for this OS.
    Installed,
}

/// The outcome of a search: where the executable is, or nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution {
    /// The executable exists.
    Found {
        /// Absolute path of the executable.
        path: PathBuf,
        /// Which search step found it.
        found_in: FoundIn,
    },
    /// No candidate location held the executable.
    Missing,
}

impl Resolution {
    /// The resolved path, or `None` when missing.
    pub fn path(&self) -> Option<&Path> {
        match self {
            Resolution::Found { path, .. } => Some(path),
            Resolution::Missing => None,
        }
    }
}

/// Everything [`resolve`] is allowed to look at. Assembling one is the impure
/// half of the module; deciding from one is the pure half.
pub struct Probe<'a> {
    /// Step 1: directories a bundled copy could sit in ([`bundled_dirs`]).
    pub bundled_dirs: &'a [PathBuf],
    /// Step 2: a full path to the executable the user configured, if any.
    pub configured: Option<&'a Path>,
    /// Step 3: the entries of `PATH` ([`path_dirs`]).
    pub path_dirs: &'a [PathBuf],
    /// Step 4: vendor install directories ([`install_dirs`]).
    pub install_dirs: &'a [PathBuf],
    /// Executable file names to try inside each directory, most preferred
    /// first (Edge before Chrome, say).
    pub executables: &'a [&'a str],
    /// Whether a candidate path is an existing, usable program. Injected so
    /// tests decide with tempfiles instead of the host's real installs.
    pub exists: fn(&Path) -> bool,
}

/// The default [`Probe::exists`]: a real file on disk.
///
/// Deliberately *not* an execute-permission check. On Windows there is no such
/// bit, and on Unix a file we can see but not execute is a broken install we
/// want to report as found-but-failing at spawn time rather than silently skip
/// in favour of a different copy.
pub fn is_file(path: &Path) -> bool {
    path.is_file()
}

/// Walk the search order and return the first hit.
///
/// Pure: every input arrives through `probe`. With no executable names (a
/// dependency that does not apply to this OS) the answer is always
/// [`Resolution::Missing`], since there is nothing to look for.
pub fn resolve(probe: &Probe<'_>) -> Resolution {
    if probe.executables.is_empty() {
        return Resolution::Missing;
    }

    if let Some(found) = first_hit(probe.bundled_dirs, probe.executables, probe.exists) {
        return Resolution::Found {
            path: found,
            found_in: FoundIn::Bundled,
        };
    }

    // The configured value is a full path to the program, not a directory: the
    // user picked a specific executable, so we do not go looking for our
    // preferred names next to it.
    if let Some(configured) = probe.configured {
        if (probe.exists)(configured) {
            return Resolution::Found {
                path: configured.to_path_buf(),
                found_in: FoundIn::Configured,
            };
        }
    }

    if let Some(found) = first_hit(probe.path_dirs, probe.executables, probe.exists) {
        return Resolution::Found {
            path: found,
            found_in: FoundIn::Path,
        };
    }

    if let Some(found) = first_hit(probe.install_dirs, probe.executables, probe.exists) {
        return Resolution::Found {
            path: found,
            found_in: FoundIn::Installed,
        };
    }

    Resolution::Missing
}

/// First `dir/exe` that `exists` accepts, scanning directories in order and, in
/// each, executable names in order.
fn first_hit(dirs: &[PathBuf], executables: &[&str], exists: fn(&Path) -> bool) -> Option<PathBuf> {
    dirs.iter().find_map(|dir| {
        executables
            .iter()
            .map(|exe| dir.join(exe))
            .find(|candidate| exists(candidate))
    })
}

// ---------------------------------------------------------------------------
// Per-dependency facts
// ---------------------------------------------------------------------------

/// The static facts about one dependency on one OS.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Spec {
    /// Which dependency these facts describe.
    pub id: Dependency,
    /// The name we print in messages, spelled the way the vendor spells it.
    pub name: &'static str,
    /// Executable file names to look for, most preferred first. Empty when the
    /// dependency does not apply to this OS.
    pub executables: &'static [&'static str],
    /// The subdirectory a bundled copy lives in, under the app's resource
    /// directory (see `bundle.resources` in `tauri.conf.json`).
    pub bundle_subdir: &'static str,
    /// Extensions that cannot be printed **today** while this is missing —
    /// always a subset of [`crate::engine::SUPPORTED_EXTS`].
    pub blocks: &'static [&'static str],
    /// What the user loses, phrased for the end of
    /// "`<name>` is missing — …". Stated in the present tense about the build
    /// they are running, never about the design target.
    pub effect: &'static str,
}

impl Spec {
    /// Whether this dependency is used at all on this OS. A non-applicable one
    /// is never reported as missing: SumatraPDF is irrelevant on macOS, and
    /// saying "SumatraPDF is missing" there would be a false alarm.
    pub fn applicable(&self) -> bool {
        !self.executables.is_empty()
    }

    /// The remediation message shown when this dependency is missing. Names the
    /// dependency and states the effect, per FR-058.
    pub fn remediation(&self) -> String {
        format!("{} is missing — {}", self.name, self.effect)
    }
}

/// What a missing browser blocks: HTML directly, and Markdown, whose Pandoc
/// output is printed by the same browser.
const CHROMIUM_BLOCKS: &[&str] = &["html", "htm", "md", "markdown"];

/// The facts about `dep` on `os`.
pub fn spec(dep: Dependency, os: Os) -> Spec {
    match dep {
        Dependency::SumatraPdf => Spec {
            id: dep,
            name: "SumatraPDF",
            // Windows-only by design: CUPS `lp` is the backend everywhere else,
            // so there is nothing to look for on macOS/Linux.
            executables: match os {
                Os::Windows => &["SumatraPDF.exe"],
                _ => &[],
            },
            bundle_subdir: "sumatrapdf",
            // The final printer for *every* format on Windows, so a missing
            // copy blocks the whole supported list — this is the one dependency
            // whose absence really does stop printing.
            blocks: crate::engine::SUPPORTED_EXTS,
            effect: "no file can be printed on Windows until it is installed. \
                     Install SumatraPDF, or place SumatraPDF.exe next to Pluryprint.exe",
        },
        Dependency::LibreOffice => Spec {
            id: dep,
            name: "LibreOffice",
            executables: match os {
                Os::Windows => &["soffice.exe"],
                _ => &["soffice"],
            },
            bundle_subdir: "libreoffice",
            blocks: crate::engine::convert::OFFICE_EXTS,
            effect: "Office, OpenDocument, TXT and RTF files cannot be printed until \
                     it is installed. Download it free from https://www.libreoffice.org/download",
        },
        Dependency::Chromium => Spec {
            id: dep,
            name: "Microsoft Edge or Google Chrome",
            executables: match os {
                // Edge first: it ships with Windows, so it is the copy the
                // architecture calls the recommended one.
                Os::Windows => &["msedge.exe", "chrome.exe"],
                Os::MacOs => &["Microsoft Edge", "Google Chrome", "Chromium"],
                Os::Linux => &[
                    "google-chrome",
                    "chromium",
                    "chromium-browser",
                    "microsoft-edge",
                ],
            },
            bundle_subdir: "chromium",
            blocks: CHROMIUM_BLOCKS,
            effect: "HTML and Markdown files cannot be printed until one is installed. \
                     Install Microsoft Edge (https://www.microsoft.com/edge) or \
                     Google Chrome (https://www.google.com/chrome)",
        },
        Dependency::Pandoc => Spec {
            id: dep,
            name: "Pandoc",
            executables: match os {
                Os::Windows => &["pandoc.exe"],
                _ => &["pandoc"],
            },
            bundle_subdir: "pandoc",
            blocks: crate::engine::convert::MARKDOWN_EXTS,
            effect: "Markdown files cannot be printed until it is installed. \
                     Download it from https://pandoc.org/installing.html",
        },
    }
}

// ---------------------------------------------------------------------------
// Gathering the environment (the impure half)
// ---------------------------------------------------------------------------

/// Directories a bundled copy could sit in, derived from `exe_dir` (the
/// directory holding our own executable).
///
/// Covers both installed layouts Tauri produces plus the two dev conveniences:
///
/// | Candidate                          | Why                                    |
/// |------------------------------------|----------------------------------------|
/// | `<exe>/resources/<subdir>`         | Windows/Linux bundle resource dir      |
/// | `<exe>/../Resources/<subdir>`      | macOS `.app` bundle resource dir       |
/// | `<exe>/<subdir>`, `<exe>/bin`, `<exe>` | dropped in by hand next to the app |
pub fn bundled_dirs(exe_dir: &Path, subdir: &str) -> Vec<PathBuf> {
    let mut dirs = vec![
        exe_dir.join("resources").join(subdir),
        exe_dir.join("resources"),
    ];
    if let Some(parent) = exe_dir.parent() {
        dirs.push(parent.join("Resources").join(subdir));
        dirs.push(parent.join("Resources"));
    }
    dirs.push(exe_dir.join(subdir));
    dirs.push(exe_dir.join("bin"));
    dirs.push(exe_dir.to_path_buf());
    dirs
}

/// The directory holding our own executable, when it can be determined.
pub fn current_exe_dir() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf))
}

/// The entries of `PATH`, as read from `env`.
pub fn path_dirs(env: &dyn Fn(&str) -> Option<String>) -> Vec<PathBuf> {
    env("PATH")
        .map(|value| std::env::split_paths(&value).collect())
        .unwrap_or_default()
}

/// Where each vendor's installer puts the program on `os`.
///
/// `env` supplies the environment variables the Windows locations expand from,
/// so the Windows table is testable from any host. Variables that are not set
/// simply contribute no candidate.
pub fn install_dirs(dep: Dependency, os: Os, env: &dyn Fn(&str) -> Option<String>) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    let mut push = |var: &str, tail: &[&str]| {
        if let Some(root) = env(var) {
            let mut dir = PathBuf::from(root);
            for segment in tail {
                dir.push(segment);
            }
            dirs.push(dir);
        }
    };

    match (dep, os) {
        (Dependency::SumatraPdf, Os::Windows) => {
            // The default installer is per-user; the "all users" option and the
            // 32-bit build land under Program Files.
            push("LOCALAPPDATA", &["SumatraPDF"]);
            push("ProgramFiles", &["SumatraPDF"]);
            push("ProgramFiles(x86)", &["SumatraPDF"]);
        }
        (Dependency::LibreOffice, Os::Windows) => {
            push("ProgramFiles", &["LibreOffice", "program"]);
            push("ProgramFiles(x86)", &["LibreOffice", "program"]);
        }
        (Dependency::LibreOffice, Os::MacOs) => {
            dirs.push(PathBuf::from(
                "/Applications/LibreOffice.app/Contents/MacOS",
            ));
        }
        (Dependency::Chromium, Os::Windows) => {
            push("ProgramFiles(x86)", &["Microsoft", "Edge", "Application"]);
            push("ProgramFiles", &["Microsoft", "Edge", "Application"]);
            push("ProgramFiles", &["Google", "Chrome", "Application"]);
            push("ProgramFiles(x86)", &["Google", "Chrome", "Application"]);
        }
        (Dependency::Chromium, Os::MacOs) => {
            dirs.push(PathBuf::from(
                "/Applications/Microsoft Edge.app/Contents/MacOS",
            ));
            dirs.push(PathBuf::from(
                "/Applications/Google Chrome.app/Contents/MacOS",
            ));
            dirs.push(PathBuf::from("/Applications/Chromium.app/Contents/MacOS"));
        }
        (Dependency::Pandoc, Os::Windows) => {
            push("LOCALAPPDATA", &["Pandoc"]);
            push("ProgramFiles", &["Pandoc"]);
        }
        (Dependency::Pandoc, Os::MacOs) => {
            // Homebrew, Apple Silicon then Intel.
            dirs.push(PathBuf::from("/opt/homebrew/bin"));
            dirs.push(PathBuf::from("/usr/local/bin"));
        }
        // Linux installs land on `PATH` (step 3) via the distribution's
        // packages; `/usr/bin` is listed anyway for a `PATH`-less environment.
        (_, Os::Linux) => dirs.push(PathBuf::from("/usr/bin")),
        // SumatraPDF does not exist off Windows.
        (Dependency::SumatraPdf, _) => {}
    }

    dirs
}

// ---------------------------------------------------------------------------
// The wire shape
// ---------------------------------------------------------------------------

/// One dependency's status, as shipped to the UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct DependencyReport {
    /// Which dependency this row reports on.
    pub id: Dependency,
    /// Display name, e.g. `"SumatraPDF"`.
    pub name: String,
    /// Whether this dependency is used at all on the running OS. `false` means
    /// "irrelevant here", not "broken" — nothing should be flagged for it.
    pub applicable: bool,
    /// Whether an executable was found.
    pub found: bool,
    /// Where it was found, for diagnostics.
    pub path: Option<String>,
    /// Which search step found it, for diagnostics.
    pub found_in: Option<FoundIn>,
    /// Extensions that cannot be printed today because this is missing. Empty
    /// whenever the dependency is present, not applicable, or gates no format
    /// the engine currently supports.
    pub blocks: Vec<String>,
    /// FR-058's remediation message. `Some` only when the dependency is
    /// applicable and missing.
    pub remediation: Option<String>,
}

/// Look for `dep` on the real machine: assembles the four candidate lists from
/// the environment and hands them to [`resolve`].
///
/// `configured` is the user's explicit override; no setting feeds it yet (see
/// the module docs), so callers pass `None`.
pub fn resolve_on_host(dep: Dependency, os: Os, configured: Option<&Path>) -> Resolution {
    let spec = spec(dep, os);
    // Nothing to look for: skip the filesystem entirely.
    if !spec.applicable() {
        return Resolution::Missing;
    }

    let env = |name: &str| std::env::var(name).ok();
    let bundled = current_exe_dir()
        .map(|dir| bundled_dirs(&dir, spec.bundle_subdir))
        .unwrap_or_default();

    resolve(&Probe {
        bundled_dirs: &bundled,
        configured,
        path_dirs: &path_dirs(&env),
        install_dirs: &install_dirs(dep, os, &env),
        executables: spec.executables,
        exists: is_file,
    })
}

/// Status of one dependency on the real machine, ready for the wire.
pub fn report(dep: Dependency, os: Os, configured: Option<&Path>) -> DependencyReport {
    from_resolution(&spec(dep, os), &resolve_on_host(dep, os, configured))
}

/// Build the wire report from an already-computed [`Resolution`]. Pure, so the
/// message and `blocks` rules can be tested without touching the host.
///
/// A dependency this OS never uses is reported as not applicable and nothing
/// else: no remediation, no blocked formats. That is the guard against the
/// loudest possible false alarm — "SumatraPDF is missing" on a Mac, where the
/// engine prints through CUPS and would not have used it either way.
pub fn from_resolution(spec: &Spec, resolution: &Resolution) -> DependencyReport {
    if !spec.applicable() {
        return DependencyReport {
            id: spec.id,
            name: spec.name.to_string(),
            applicable: false,
            found: false,
            path: None,
            found_in: None,
            blocks: Vec::new(),
            remediation: None,
        };
    }

    let found = matches!(resolution, Resolution::Found { .. });
    DependencyReport {
        id: spec.id,
        name: spec.name.to_string(),
        applicable: spec.applicable(),
        found,
        path: resolution.path().map(|p| p.to_string_lossy().into_owned()),
        found_in: match resolution {
            Resolution::Found { found_in, .. } => Some(*found_in),
            Resolution::Missing => None,
        },
        blocks: if found {
            Vec::new()
        } else {
            spec.blocks.iter().map(|e| (*e).to_string()).collect()
        },
        remediation: if found {
            None
        } else {
            Some(spec.remediation())
        },
    }
}

/// Status of every dependency on the running OS, in [`ALL`] order.
pub fn report_all() -> Vec<DependencyReport> {
    let os = Os::current();
    ALL.iter().map(|dep| report(*dep, os, None)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `Probe` with every step empty, so a test only fills the one it is about.
    fn empty_probe<'a>(executables: &'a [&'a str]) -> Probe<'a> {
        Probe {
            bundled_dirs: &[],
            configured: None,
            path_dirs: &[],
            install_dirs: &[],
            executables,
            exists: is_file,
        }
    }

    #[test]
    fn no_executables_never_resolves() {
        assert_eq!(resolve(&empty_probe(&[])), Resolution::Missing);
    }

    #[test]
    fn everything_absent_is_missing() {
        assert_eq!(resolve(&empty_probe(&["nothing.exe"])), Resolution::Missing);
    }

    #[test]
    fn executable_names_are_tried_in_order() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("chrome.exe"), b"x").unwrap();
        std::fs::write(dir.path().join("msedge.exe"), b"x").unwrap();
        let dirs = vec![dir.path().to_path_buf()];

        let resolution = resolve(&Probe {
            path_dirs: &dirs,
            ..empty_probe(&["msedge.exe", "chrome.exe"])
        });
        assert_eq!(resolution.path().unwrap(), dir.path().join("msedge.exe"));
    }

    #[test]
    fn bundled_layout_covers_the_installed_resource_dir() {
        let dirs = bundled_dirs(Path::new("/apps/Pluryprint"), "sumatrapdf");
        assert!(dirs.contains(&PathBuf::from("/apps/Pluryprint/resources/sumatrapdf")));
        assert!(dirs.contains(&PathBuf::from("/apps/Resources/sumatrapdf")));
        assert!(dirs.contains(&PathBuf::from("/apps/Pluryprint")));
    }

    #[test]
    fn windows_install_dirs_expand_from_the_environment() {
        let env = |name: &str| match name {
            "LOCALAPPDATA" => Some(r"C:\Users\ada\AppData\Local".to_string()),
            _ => None,
        };
        let dirs = install_dirs(Dependency::SumatraPdf, Os::Windows, &env);
        // Only the variable that is set contributes a candidate.
        assert_eq!(dirs.len(), 1);
        assert!(dirs[0].ends_with("SumatraPDF"));
    }

    #[test]
    fn sumatrapdf_is_not_applicable_off_windows() {
        for os in [Os::MacOs, Os::Linux] {
            let spec = spec(Dependency::SumatraPdf, os);
            assert!(!spec.applicable(), "{os:?}");
            let report = from_resolution(&spec, &Resolution::Missing);
            assert!(!report.applicable);
            assert!(
                report.remediation.is_none(),
                "a dependency this OS never uses must not raise an alarm"
            );
            assert!(report.blocks.is_empty());
        }
    }

    #[test]
    fn fr_058_every_converter_dependency_blocks_supported_formats_only() {
        for os in [Os::Windows, Os::MacOs, Os::Linux] {
            for dep in ALL {
                let spec = spec(*dep, os);
                for ext in spec.blocks {
                    assert!(
                        crate::engine::is_supported(ext),
                        "{dep:?} blocks {ext}, which the engine does not advertise"
                    );
                }
            }
        }
    }

    #[test]
    fn fr_058_missing_converters_name_what_they_block_and_how_to_install() {
        let cases = [
            (Dependency::LibreOffice, "docx", "libreoffice.org"),
            (Dependency::LibreOffice, "txt", "libreoffice.org"),
            (Dependency::Chromium, "html", "Microsoft Edge"),
            (Dependency::Chromium, "md", "Google Chrome"),
            (Dependency::Pandoc, "md", "pandoc.org"),
        ];
        for (dep, ext, hint) in cases {
            let report = from_resolution(&spec(dep, Os::Windows), &Resolution::Missing);
            assert!(
                report.blocks.iter().any(|b| b == ext),
                "{dep:?} must block {ext}"
            );
            let text = report.remediation.expect("missing → remediation");
            assert!(text.starts_with(&report.name), "{text}");
            assert!(text.contains(hint), "{dep:?}: {text}");
            assert!(!text.contains("changes nothing"), "stale text: {text}");
        }
    }

    #[test]
    fn a_found_converter_blocks_nothing() {
        let spec = spec(Dependency::LibreOffice, Os::MacOs);
        let report = from_resolution(
            &spec,
            &Resolution::Found {
                path: PathBuf::from("/Applications/LibreOffice.app/Contents/MacOS/soffice"),
                found_in: FoundIn::Installed,
            },
        );
        assert!(report.blocks.is_empty());
        assert!(report.remediation.is_none());
    }
}
