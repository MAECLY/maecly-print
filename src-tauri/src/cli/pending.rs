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

use std::path::PathBuf;
use std::sync::Mutex;

use serde::Serialize;

use crate::cli::{handoff, Selection};
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

/// Build the store for a GUI launch: park whatever the command line handed us,
/// then retire the handoff file that selection arrived in.
///
/// The two steps belong together and in this order. The file is the only copy
/// of the selection until it is in memory here, and it is deliberately left on
/// disk by [`crate::cli::selection_from_at`] so that a launch which turns out
/// to be forwarded still has something for the running instance to read
/// (NFR-014). This process may therefore only delete it once it knows it is the
/// one keeping the selection — which is exactly the moment the store exists.
pub fn park_launch_selection(
    selection: Option<Selection>,
    handoff: Option<PathBuf>,
) -> PendingSelections {
    let pending = PendingSelections::with(selection);
    if let Some(path) = handoff {
        handoff::discard(&path);
    }
    pending
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

    /// The launch-time half of the handoff lifecycle: the selection is in the
    /// store *and* the file it came in is gone, so a reload of the webview
    /// cannot replay it and the payload no longer names the user's documents.
    #[test]
    fn nfr_014_parking_a_launch_selection_retires_its_handoff_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sel.json");
        std::fs::write(&path, "{}").unwrap();

        let store = park_launch_selection(Some(selection("/tmp/a.pdf")), Some(path.clone()));
        assert_eq!(store.drain().len(), 1);
        assert!(!path.exists(), "a parked selection retires its file");
    }

    /// A plain launch has nothing to park and nothing to delete.
    #[test]
    fn nfr_014_a_plain_launch_touches_no_handoff_file() {
        assert!(park_launch_selection(None, None).drain().is_empty());
    }
}
