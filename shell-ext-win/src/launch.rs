//! Writing the handoff file and starting the app.
//!
//! This is the whole "producer" side of the contract in
//! `src-tauri/src/cli/handoff.rs`: write the selection into the handoff
//! directory, then run
//!
//! ```text
//! maeclyprint.exe --invoke=context-menu --files-from <path> [--action A] [--profile P]
//! ```
//!
//! `--action` and `--profile` are passed as well as being inside the file. The
//! file wins where it has an opinion, but a payload the app decides to ignore
//! still produces the right behaviour from argv, and the argv form is what a
//! human sees in Process Explorer when debugging.
//!
//! The command line's only variable part is one path, so its length is bounded
//! by `MAX_PATH` no matter how large the selection is — which is the entire
//! reason FR-003 is satisfiable.
//!
//! A second launch while the app is already running is not a problem here: the
//! app registers `tauri-plugin-single-instance`, so the running process
//! receives this argv and merges the selection.

use std::path::{Path, PathBuf};

use windows::core::{w, HSTRING, PCWSTR};
use windows::Win32::UI::Shell::{
    ShellExecuteExW, SEE_MASK_FLAG_NO_UI, SEE_MASK_NOASYNC, SHELLEXECUTEINFOW,
};
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

use crate::handoff::{self, Action};

/// Write the handoff file for `paths` and launch the app against it.
///
/// Returns the handoff path so the caller can log it. An empty selection is
/// refused before anything is written: the app rejects a payload with no paths,
/// and launching a window for nothing is worse than doing nothing.
pub fn deliver(
    paths: &[PathBuf],
    action: Action,
    profile_id: Option<String>,
) -> std::io::Result<PathBuf> {
    if paths.is_empty() {
        return Err(std::io::Error::other(
            "the selection contained no filesystem paths",
        ));
    }

    let exe = crate::module::app_exe().ok_or_else(|| {
        std::io::Error::other("could not locate maeclyprint.exe next to the shell extension")
    })?;

    let handoff_path = handoff::write(paths, action, profile_id.clone())?;

    match launch(&exe, &handoff_path, action, profile_id.as_deref()) {
        Ok(()) => Ok(handoff_path),
        Err(err) => {
            // The app will never read it, and the app's own orphan sweep only
            // runs on startup — which is exactly what just failed to happen.
            let _ = std::fs::remove_file(&handoff_path);
            Err(std::io::Error::other(err.to_string()))
        }
    }
}

fn launch(
    exe: &Path,
    handoff_path: &Path,
    action: Action,
    profile_id: Option<&str>,
) -> windows_core::Result<()> {
    let mut args = format!(
        "--invoke=context-menu --files-from \"{}\" --action {}",
        handoff_path.display(),
        action.as_str()
    );
    if let Some(profile) = profile_id {
        // Quote and escape: a profile name is user-controlled text and could
        // otherwise inject another argument.
        args.push_str(&format!(" --profile \"{}\"", profile.replace('"', "\\\"")));
    }

    let exe = HSTRING::from(exe.as_os_str());
    let args = HSTRING::from(args);
    let working_dir = HSTRING::from(
        exe.to_string()
            .rsplit_once('\\')
            .map(|(dir, _)| dir.to_string())
            .unwrap_or_default(),
    );

    let mut info = SHELLEXECUTEINFOW {
        cbSize: core::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        // NOASYNC: this DLL can be unloaded the moment `Invoke` returns, and an
        // asynchronous ShellExecuteEx would still be reading our memory.
        // NO_UI: Explorer must never end up owning an error dialog we raised.
        fMask: SEE_MASK_NOASYNC | SEE_MASK_FLAG_NO_UI,
        lpVerb: w!("open"),
        lpFile: PCWSTR(exe.as_ptr()),
        lpParameters: PCWSTR(args.as_ptr()),
        lpDirectory: PCWSTR(working_dir.as_ptr()),
        nShow: SW_SHOWNORMAL.0,
        ..Default::default()
    };

    unsafe { ShellExecuteExW(&mut info) }
}
