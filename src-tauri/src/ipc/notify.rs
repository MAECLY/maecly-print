//! Native OS notifications (FR-072).
//!
//! The in-app toasts (FR-070/FR-071) only help someone looking at the window.
//! MaeclyPrint mostly runs in the background — a right-click hands it files and
//! the user goes back to what they were doing — so a finished, failed or stuck
//! batch is also announced through the OS notification centre (Windows toast,
//! macOS Notification Center) **when the window is not in front of the user**.
//!
//! The frontend decides *what* to say (it already owns the translated copy and
//! the batch bookkeeping); this command decides *whether* a native notification
//! is warranted, so the focus check uses the real window state rather than the
//! webview's own idea of it, and the user's `app.native_notifications`
//! preference is read where it is stored.

use tauri::Manager;
use tauri_plugin_notification::{NotificationExt, PermissionState};

use crate::util::error::IpcError;

/// What we know about the main window when a notification is requested.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowPresence {
    pub focused: bool,
    pub visible: bool,
    pub minimized: bool,
}

/// FR-072: whether a native notification should be shown. Pure.
///
/// Only when the user allowed it **and** the window is not what they are
/// looking at: hidden, minimized, or simply behind another app. A window in
/// front already shows the toast; a second, OS-level copy would be noise.
pub fn should_notify_natively(enabled: bool, window: Option<WindowPresence>) -> bool {
    if !enabled {
        return false;
    }
    match window {
        // No window at all (closed to the tray, or not created yet): the
        // notification is the only way to reach the user.
        None => true,
        Some(w) => !(w.focused && w.visible && !w.minimized),
    }
}

fn presence(app: &tauri::AppHandle) -> Option<WindowPresence> {
    let window = app.get_webview_window("main")?;
    Some(WindowPresence {
        // Errors read as "not in front", which errs towards telling the user.
        focused: window.is_focused().unwrap_or(false),
        visible: window.is_visible().unwrap_or(false),
        minimized: window.is_minimized().unwrap_or(false),
    })
}

/// Show `title`/`body` as a native notification if [`should_notify_natively`]
/// says so. Resolves to whether one was shown.
///
/// Never an error the UI has to handle: the in-app toast has already been
/// shown, so a denied permission or a notification service that is not
/// running is logged and reported as `false`.
#[tauri::command]
#[specta::specta]
pub fn notify_native(app: tauri::AppHandle, title: String, body: String) -> Result<bool, IpcError> {
    let enabled = crate::settings::Config::load_or_default()
        .app
        .native_notifications;
    if !should_notify_natively(enabled, presence(&app)) {
        return Ok(false);
    }

    // Ask politely, once, and respect the answer: a denial is the user's
    // choice and not an error worth surfacing.
    let notifications = app.notification();
    let permission = match notifications.permission_state() {
        Ok(PermissionState::Granted) => PermissionState::Granted,
        Ok(_) => notifications
            .request_permission()
            .unwrap_or(PermissionState::Denied),
        Err(e) => {
            tracing::debug!(event = "notification_permission_unknown", error = %e);
            PermissionState::Denied
        }
    };
    if permission != PermissionState::Granted {
        tracing::info!(event = "notification_permission_denied");
        return Ok(false);
    }

    match notifications.builder().title(title).body(body).show() {
        Ok(()) => Ok(true),
        Err(e) => {
            tracing::warn!(event = "notification_failed", error = %e);
            Ok(false)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FRONT: WindowPresence = WindowPresence {
        focused: true,
        visible: true,
        minimized: false,
    };

    #[test]
    fn fr_072_no_native_notification_while_the_window_is_in_front() {
        assert!(!should_notify_natively(true, Some(FRONT)));
    }

    #[test]
    fn fr_072_native_notification_when_the_window_is_in_the_background() {
        for w in [
            WindowPresence {
                focused: false,
                ..FRONT
            },
            WindowPresence {
                visible: false,
                ..FRONT
            },
            WindowPresence {
                minimized: true,
                ..FRONT
            },
        ] {
            assert!(should_notify_natively(true, Some(w)), "{w:?}");
        }
        assert!(should_notify_natively(true, None), "no window at all");
    }

    #[test]
    fn fr_072_the_preference_turns_native_notifications_off() {
        assert!(!should_notify_natively(
            false,
            Some(WindowPresence {
                focused: false,
                visible: false,
                minimized: true,
            })
        ));
        assert!(!should_notify_natively(false, None));
    }
}
