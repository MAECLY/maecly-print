//! Bringing the running window forward when a second launch hands it a
//! selection (FR-001, NFR-014), without ever synthesizing input.
//!
//! # The stall this replaces
//!
//! The single-instance callback used to call `WebviewWindow::set_focus`. On
//! Windows that is tao's `force_window_active`: it tries
//! `SetForegroundWindow`, and when Windows refuses (a background process may
//! not steal the foreground, and the running instance *is* in the background
//! when Explorer launches the forwarding process) it injects an Alt press and
//! release with `SendInput` and tries again. The injected Alt-up lands on our
//! window, `DefWindowProc` turns a lone Alt into `SC_KEYMENU`, and the window
//! enters menu mode: a modal loop that keeps the window looking active while
//! the event loop's own work (the IPC replies the page is waiting for, the
//! `selection-forwarded` drain) sits behind it. Nothing moved until the next
//! click or key ended menu mode, which is the "job submitted only after window
//! activity, 1 to 7 minutes later" defect of the round-3 upgrade test (row 7c).
//!
//! # What happens instead
//!
//! - The forwarding process — which Explorer started in response to the
//!   user's click and which may therefore set the foreground — passes that
//!   right on with `AllowSetForegroundWindow(ASFW_ANY)` before the plugin
//!   hands over its argv ([`grant_to_running_instance`]).
//! - The running instance restores its window and calls plain
//!   `SetForegroundWindow` ([`bring_to_front`]). If Windows still says no, the
//!   taskbar button flashes until the user switches to it. No keystroke is
//!   ever faked.

use tauri::{Runtime, WebviewWindow};

/// How [`bring_to_front`] ended. Logged; tests pin the decision table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Raise {
    /// The window was already the foreground window.
    AlreadyForeground,
    /// `SetForegroundWindow` succeeded.
    Raised,
    /// Windows refused the foreground; the taskbar button flashes instead.
    Flashed,
    /// Not Windows: the platform's own focus call was used.
    Platform,
}

impl Raise {
    /// The event name logged for this outcome.
    pub const fn as_str(self) -> &'static str {
        match self {
            Raise::AlreadyForeground => "already_foreground",
            Raise::Raised => "raised",
            Raise::Flashed => "flashed",
            Raise::Platform => "platform",
        }
    }
}

/// The decision, given whether the window already is the foreground window
/// and whether a plain `SetForegroundWindow` worked. Pure, so the "never fall
/// back to faked input" rule is testable on any host.
pub fn decide(already_foreground: bool, set_foreground_ok: impl FnOnce() -> bool) -> Raise {
    if already_foreground {
        Raise::AlreadyForeground
    } else if set_foreground_ok() {
        Raise::Raised
    } else {
        Raise::Flashed
    }
}

/// Let whichever process owns the running window take the foreground. Called
/// early in every GUI launch on Windows: a launch that turns out to be the
/// first instance has given away nothing it needs (the right only lasts until
/// the next user input). A no-op elsewhere.
pub fn grant_to_running_instance() {
    #[cfg(windows)]
    {
        use windows::Win32::UI::WindowsAndMessaging::{AllowSetForegroundWindow, ASFW_ANY};
        // Fails when this process holds no foreground right to pass on (it
        // was not started by the foreground app); nothing to do then.
        if let Err(e) = unsafe { AllowSetForegroundWindow(ASFW_ANY) } {
            tracing::debug!(event = "allow_set_foreground_failed", error = %e);
        }
    }
}

/// Restore `window` and bring it to the front without synthesizing input.
pub fn bring_to_front<R: Runtime>(window: &WebviewWindow<R>) -> Raise {
    let _ = window.unminimize();
    #[cfg(windows)]
    {
        windows_raise(window)
    }
    #[cfg(not(windows))]
    {
        let _ = window.set_focus();
        Raise::Platform
    }
}

#[cfg(windows)]
fn windows_raise<R: Runtime>(window: &WebviewWindow<R>) -> Raise {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        FlashWindowEx, GetForegroundWindow, SetForegroundWindow, FLASHWINFO, FLASHW_TIMERNOFG,
        FLASHW_TRAY,
    };

    let Ok(raw) = window.hwnd() else {
        return Raise::Flashed;
    };
    // tauri's HWND comes from another `windows` release; same pointer.
    let hwnd = HWND(raw.0);
    let foreground = unsafe { GetForegroundWindow() } == hwnd;
    let outcome = decide(foreground, || {
        unsafe { SetForegroundWindow(hwnd) }.as_bool()
    });
    if outcome == Raise::Flashed {
        let info = FLASHWINFO {
            cbSize: std::mem::size_of::<FLASHWINFO>() as u32,
            hwnd,
            dwFlags: FLASHW_TRAY | FLASHW_TIMERNOFG,
            uCount: 0,
            dwTimeout: 0,
        };
        let _ = unsafe { FlashWindowEx(&info) };
    }
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nfr_014_foreground_window_is_left_alone() {
        let outcome = decide(true, || panic!("must not even try"));
        assert_eq!(outcome, Raise::AlreadyForeground);
    }

    #[test]
    fn nfr_014_refused_foreground_flashes_instead_of_faking_input() {
        assert_eq!(decide(false, || true), Raise::Raised);
        assert_eq!(decide(false, || false), Raise::Flashed);
    }

    /// FR-001 (round-3 row 7c): the forwarded-launch callback must not call
    /// `set_focus`, whose Windows fallback injects an Alt keystroke that parks
    /// the window in menu mode and stalls the selection's delivery.
    #[test]
    fn fr_001_forwarded_launch_never_uses_set_focus() {
        let lib = include_str!("../lib.rs");
        let start = lib
            .find("tauri_plugin_single_instance::init")
            .expect("the single-instance plugin is registered in lib.rs");
        let callback = &lib[start..];
        let end = callback
            .find(".plugin(tauri_plugin_dialog")
            .expect("the next plugin follows the callback");
        let code: String = callback[..end]
            .lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
        let callback = code.as_str();
        assert!(
            !callback.contains(".set_focus("),
            "the single-instance callback must not call set_focus"
        );
        assert!(callback.contains("foreground::bring_to_front"));
    }
}
