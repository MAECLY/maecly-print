//! FR-058: guided installation of a missing dependency (winget, Homebrew, the
//! Microsoft Store or the vendor's web page), always after the user's explicit
//! consent, never silently (decision D13).
//!
//! The UI gets the options from the dependency report
//! ([`crate::engine::deps::DependencyReport::install`]): for each, the exact
//! command that will run (or the page that will open), whether Windows will
//! ask for administrator rights (LibreOffice's MSI) and whether it costs money
//! (HEVC). The consent dialog shows all of it; only its *Install* button sends
//! `consent: true`, and nothing runs without it.
//!
//! - **winget / brew** run with an argument vector (no shell), their output
//!   streamed line by line as [`DependencyInstallProgress`] events, bounded by
//!   [`INSTALL_TIMEOUT`]. One install at a time.
//! - **Store / web** open the page with the OS handler; the user finishes
//!   there.
//!
//! When a command finishes the dependency is probed again and the fresh
//! report comes back, so the UI knows at once whether the files it blocked can
//! print now.

use std::io::{BufRead, BufReader, Read};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri_specta::Event;

use crate::engine::deps::{self, Dependency, DependencyReport, InstallMethod, InstallTool, Os};
use crate::util::error::{IpcError, IpcErrorCode};

/// How long a winget/brew install may run before it is stopped.
pub const INSTALL_TIMEOUT: Duration = Duration::from_secs(30 * 60);

/// Set while an install runs: one at a time.
static INSTALLING: AtomicBool = AtomicBool::new(false);

/// How an install ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "kebab-case")]
pub enum InstallStatus {
    /// The command succeeded and the dependency is found now.
    Installed,
    /// The command succeeded but the dependency is still not found (a
    /// restart of Pluryprint, or of Windows, may be needed).
    NotDetected,
    /// The command failed; `exit_code` says how. The web page remains.
    Failed,
    /// A Store or web page was opened; the user finishes there.
    Opened,
}

/// The result of [`install_dependency`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct InstallOutcome {
    /// How it ended.
    pub status: InstallStatus,
    /// The command's exit code, when one ran.
    pub exit_code: Option<i32>,
    /// The dependency probed again after the install.
    pub report: DependencyReport,
}

/// One line of an install's output, as it happens (FR-058).
#[derive(Debug, Clone, Serialize, specta::Type, tauri_specta::Event)]
pub struct DependencyInstallProgress {
    /// The dependency being installed.
    pub id: Dependency,
    /// One line of the installer's output.
    pub line: String,
}

/// What [`plan`] decided to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Plan {
    /// Open this URL with the OS handler.
    Open(String),
    /// Run `tool` with these arguments.
    Run {
        /// The program.
        tool: InstallTool,
        /// Its argument vector.
        args: Vec<String>,
    },
}

/// Decide what installing `id` by `method` on `os` means, refusing anything
/// the user did not consent to or that is not on offer. Pure: `has_tool`
/// says whether winget/brew are present.
pub fn plan(
    id: Dependency,
    method: InstallMethod,
    consent: bool,
    os: Os,
    has_tool: &dyn Fn(InstallTool) -> bool,
) -> Result<Plan, IpcError> {
    if !consent {
        return Err(IpcError::localized(
            IpcErrorCode::InvalidRequest,
            "install_consent_required",
            "installing a dependency needs the user's consent",
        ));
    }
    let spec = deps::spec(id, os);
    let Some(recipe) = spec.install.iter().find(|r| r.method == method) else {
        return Err(IpcError::localized(
            IpcErrorCode::InvalidRequest,
            "install_not_offered",
            format!("{method:?} is not offered for {} here", spec.name),
        ));
    };
    match recipe.tool() {
        None => Ok(Plan::Open(recipe.url().unwrap_or_default())),
        Some(tool) if has_tool(tool) => Ok(Plan::Run {
            tool,
            args: recipe.args(),
        }),
        Some(tool) => Err(IpcError::localized(
            IpcErrorCode::InvalidRequest,
            "install_tool_missing",
            format!("{} is not installed", tool_name(tool)),
        )
        .with_param("tool", tool_name(tool))),
    }
}

fn tool_name(tool: InstallTool) -> &'static str {
    match tool {
        InstallTool::Winget => "winget",
        InstallTool::Brew => "Homebrew",
    }
}

/// Split installer output into lines worth showing: winget redraws progress
/// with carriage returns and spinners, so `\r` ends a line too, and lines
/// that are only a spinner or a bar are dropped. Pure.
pub fn output_lines(chunk: &str) -> Vec<String> {
    chunk
        .split(['\r', '\n'])
        .map(str::trim)
        .filter(|line| {
            !line.is_empty()
                && line
                    .chars()
                    .any(|c| c.is_alphanumeric() && !matches!(c, '█' | '▒'))
        })
        .map(str::to_string)
        .collect()
}

/// Run `program args`, sending each output line to `on_line`, stopping it at
/// `timeout`. Returns the exit code (`None` when it was stopped or the OS
/// gave none).
pub fn run_streaming(
    program: &PathBuf,
    args: &[String],
    timeout: Duration,
    on_line: &dyn Fn(String),
) -> std::io::Result<Option<i32>> {
    let mut cmd = Command::new(program);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        /// CREATE_NO_WINDOW: no console flashes up behind the app.
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = cmd.spawn()?;
    let (tx, rx) = mpsc::channel::<String>();
    let mut readers = Vec::new();
    for stream in [
        child
            .stdout
            .take()
            .map(|s| Box::new(s) as Box<dyn Read + Send>),
        child
            .stderr
            .take()
            .map(|s| Box::new(s) as Box<dyn Read + Send>),
    ]
    .into_iter()
    .flatten()
    {
        let tx = tx.clone();
        readers.push(std::thread::spawn(move || {
            let mut reader = BufReader::new(stream);
            let mut buf = Vec::new();
            // Split on both \r and \n as they arrive (winget's progress).
            while reader.read_until(b'\n', &mut buf).is_ok_and(|n| n > 0) {
                for line in output_lines(&String::from_utf8_lossy(&buf)) {
                    let _ = tx.send(line);
                }
                buf.clear();
            }
        }));
    }
    drop(tx);
    let started = Instant::now();
    let code = loop {
        while let Ok(line) = rx.try_recv() {
            on_line(line);
        }
        if let Some(status) = child.try_wait()? {
            break status.code();
        }
        if started.elapsed() > timeout {
            let _ = child.kill();
            let _ = child.wait();
            on_line(format!("stopped after {} minutes", timeout.as_secs() / 60));
            break None;
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    for reader in readers {
        let _ = reader.join();
    }
    while let Ok(line) = rx.try_recv() {
        on_line(line);
    }
    Ok(code)
}

/// Install the dependency `id` the way the user agreed to (`method`), after
/// the consent dialog (`consent` must be `true`). See the module docs.
#[tauri::command]
#[specta::specta]
pub async fn install_dependency(
    app: tauri::AppHandle,
    id: Dependency,
    method: InstallMethod,
    consent: bool,
) -> Result<InstallOutcome, IpcError> {
    let os = Os::current();
    let plan = plan(id, method, consent, os, &|tool| {
        deps::locate_tool(tool).is_some()
    })?;
    match plan {
        Plan::Open(url) => {
            use tauri_plugin_opener::OpenerExt;
            app.opener().open_url(&url, None::<&str>).map_err(|e| {
                IpcError::localized(
                    IpcErrorCode::Internal,
                    "open_link_failed",
                    format!("could not open {url}: {e}"),
                )
            })?;
            tracing::info!(event = "dependency_install_opened", dependency = ?id, url = %url);
            Ok(InstallOutcome {
                status: InstallStatus::Opened,
                exit_code: None,
                report: deps::report(id, os, None),
            })
        }
        Plan::Run { tool, args } => {
            if INSTALLING.swap(true, Ordering::SeqCst) {
                return Err(IpcError::localized(
                    IpcErrorCode::InvalidRequest,
                    "install_in_progress",
                    "another dependency is being installed",
                ));
            }
            let result = super::run_blocking(move || {
                let program = deps::locate_tool(tool).ok_or_else(|| {
                    IpcError::localized(
                        IpcErrorCode::InvalidRequest,
                        "install_tool_missing",
                        format!("{} is not installed", tool_name(tool)),
                    )
                    .with_param("tool", tool_name(tool))
                })?;
                tracing::info!(event = "dependency_install_started", dependency = ?id, program = %program.display(), args = ?args);
                let emit = |line: String| {
                    let _ = DependencyInstallProgress { id, line }.emit(&app);
                };
                let code = run_streaming(&program, &args, INSTALL_TIMEOUT, &emit).map_err(|e| {
                    IpcError::localized(
                        IpcErrorCode::Internal,
                        "install_start_failed",
                        format!("could not start {}: {e}", program.display()),
                    )
                    .with_param("tool", tool_name(tool))
                })?;
                let report = deps::report(id, os, None);
                let status = match code {
                    Some(0) if report.found => InstallStatus::Installed,
                    Some(0) => InstallStatus::NotDetected,
                    _ => InstallStatus::Failed,
                };
                tracing::info!(event = "dependency_install_finished", dependency = ?id, ?code, ?status);
                Ok(InstallOutcome {
                    status,
                    exit_code: code,
                    report,
                })
            })
            .await;
            INSTALLING.store(false, Ordering::SeqCst);
            result
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fr_058_install_requires_consent() {
        let err = plan(
            Dependency::LibreOffice,
            InstallMethod::Winget,
            false,
            Os::Windows,
            &|_| true,
        )
        .unwrap_err();
        assert_eq!(err.code(), IpcErrorCode::InvalidRequest);
        assert!(
            format!("{err:?}").contains("install_consent_required"),
            "{err:?}"
        );
        // Not even a web page opens without consent.
        assert!(plan(
            Dependency::LibreOffice,
            InstallMethod::Web,
            false,
            Os::Windows,
            &|_| true
        )
        .is_err());
    }

    #[test]
    fn fr_058_winget_absent_refuses_and_web_still_works() {
        let err = plan(
            Dependency::LibreOffice,
            InstallMethod::Winget,
            true,
            Os::Windows,
            &|_| false,
        )
        .unwrap_err();
        assert!(
            format!("{err:?}").contains("install_tool_missing"),
            "{err:?}"
        );
        assert_eq!(
            plan(
                Dependency::LibreOffice,
                InstallMethod::Web,
                true,
                Os::Windows,
                &|_| false
            )
            .unwrap(),
            Plan::Open("https://www.libreoffice.org/download/download-libreoffice/".into())
        );
    }

    #[test]
    fn fr_058_only_offered_methods_run() {
        // HEVC is paid: winget cannot buy it, so there is no winget recipe.
        let err = plan(
            Dependency::HevcCodec,
            InstallMethod::WingetStore,
            true,
            Os::Windows,
            &|_| true,
        )
        .unwrap_err();
        assert!(
            format!("{err:?}").contains("install_not_offered"),
            "{err:?}"
        );
        // Nothing is installable for a codec on macOS.
        assert!(plan(
            Dependency::HeifCodec,
            InstallMethod::Web,
            true,
            Os::MacOs,
            &|_| true
        )
        .is_err());
        match plan(
            Dependency::LibreOffice,
            InstallMethod::BrewCask,
            true,
            Os::MacOs,
            &|t| t == InstallTool::Brew,
        )
        .unwrap()
        {
            Plan::Run { tool, args } => {
                assert_eq!(tool, InstallTool::Brew);
                assert_eq!(args, ["install", "--cask", "libreoffice"]);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn fr_058_installer_output_is_split_into_readable_lines() {
        assert_eq!(
            output_lines("Found LibreOffice [TheDocumentFoundation.LibreOffice]\r\n  -\r  \\\r  ██████▒▒▒▒  20 MB / 350 MB\rSuccessfully installed\n"),
            [
                "Found LibreOffice [TheDocumentFoundation.LibreOffice]",
                "██████▒▒▒▒  20 MB / 350 MB",
                "Successfully installed"
            ]
        );
    }

    #[cfg(unix)]
    #[test]
    fn fr_058_run_streaming_reports_lines_and_the_exit_code() {
        let lines = std::sync::Mutex::new(Vec::new());
        let code = run_streaming(
            &PathBuf::from("/bin/sh"),
            &["-c".into(), "echo one; echo two >&2; exit 3".into()],
            Duration::from_secs(10),
            &|l| lines.lock().unwrap().push(l),
        )
        .unwrap();
        assert_eq!(code, Some(3));
        let mut got = lines.into_inner().unwrap();
        got.sort();
        assert_eq!(got, ["one", "two"]);

        let started = Instant::now();
        let code = run_streaming(
            &PathBuf::from("/bin/sh"),
            &["-c".into(), "sleep 20".into()],
            Duration::from_millis(300),
            &|_| {},
        )
        .unwrap();
        assert_eq!(code, None, "stopped at the timeout");
        assert!(started.elapsed() < Duration::from_secs(10));
    }
}
