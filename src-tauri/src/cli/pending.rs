//! Selections waiting for the window (NFR-014, FR-001).
//!
//! A selection can reach the app in two ways: it is on the command line of the
//! process that boots the GUI, or it arrives later from a *second* launch that
//! the single-instance plugin forwarded ([`crate::cli::single_instance`]).
//! Either way the webview may not exist yet, so the selection is parked in
//! [`PendingSelections`] (Tauri managed state) and delivered twice over:
//!
//! - the window drains it once it has mounted, via [`take_pending_selections`];
//! - an *already open* window also gets a [`SelectionForwarded`] event, so a
//!   second launch feels instant instead of waiting for a poll.
//!
//! Draining is destructive on purpose: a selection is a one-shot instruction,
//! and re-delivering it on the next reload would re-print the user's files.

use std::sync::Mutex;

use serde::Serialize;

use crate::cli::Selection;
use crate::util::error::IpcError;

/// Selections not yet handed to the window, oldest first.
#[derive(Debug, Default)]
pub struct PendingSelections(Mutex<Vec<Selection>>);

impl PendingSelections {
    /// A store already holding `selection` (the launch-time case).
    pub fn with(selection: Option<Selection>) -> Self {
        Self(Mutex::new(selection.into_iter().collect()))
    }

    /// Park a selection for the window.
    pub fn push(&self, selection: Selection) {
        // A poisoned lock would mean a panic while holding it; the data is a
        // plain Vec, so recovering it is safe and losing a selection is worse.
        let mut guard = self.0.lock().unwrap_or_else(|e| e.into_inner());
        guard.push(selection);
    }

    /// Take everything parked so far, leaving the store empty.
    pub fn drain(&self) -> Vec<Selection> {
        let mut guard = self.0.lock().unwrap_or_else(|e| e.into_inner());
        std::mem::take(&mut *guard)
    }
}

/// A second launch handed us a selection while this window was already open.
#[derive(Debug, Clone, Serialize, specta::Type, tauri_specta::Event)]
pub struct SelectionForwarded(pub Selection);

/// Drain the selections handed to this instance (command line or a forwarded
/// second launch). Returns them oldest first and empties the store.
#[tauri::command]
#[specta::specta]
pub async fn take_pending_selections(
    state: tauri::State<'_, PendingSelections>,
) -> Result<Vec<Selection>, IpcError> {
    Ok(state.drain())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::args::ContextAction;

    fn selection(path: &str) -> Selection {
        Selection {
            paths: vec![path.to_string()],
            action: ContextAction::Configure,
            profile_id: None,
        }
    }

    #[test]
    fn nfr_014_drain_is_destructive_and_ordered() {
        let store = PendingSelections::with(Some(selection("/tmp/a.pdf")));
        store.push(selection("/tmp/b.pdf"));

        let drained = store.drain();
        assert_eq!(drained.len(), 2);
        assert_eq!(drained[0].paths, vec!["/tmp/a.pdf"]);
        assert_eq!(drained[1].paths, vec!["/tmp/b.pdf"]);
        assert!(
            store.drain().is_empty(),
            "a selection is delivered exactly once"
        );
    }

    #[test]
    fn nfr_014_empty_launch_parks_nothing() {
        assert!(PendingSelections::with(None).drain().is_empty());
    }
}
