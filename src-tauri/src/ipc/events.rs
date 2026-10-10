//! Typed events pushed from the backend to the webview.
//!
//! These are the push counterpart to the request/response commands in
//! [`crate::ipc::jobs`]: the dispatcher owns queue state and emits these so the
//! UI never has to poll. Both derive [`tauri_specta::Event`], so the event
//! *names* and payload *types* are generated into `src/ipc/bindings.ts`
//! alongside the commands (the frontend listens via the generated `events`
//! object). Event names are the struct identifiers kebab-cased by the derive:
//! `QueueUpdated` → `"queue-updated"`, `JobProgressEvent` →
//! `"job-progress-event"`.

use serde::Serialize;

use crate::queue::job::QueueSnapshot;

/// The whole queue changed: a job transitioned, was submitted, cancelled,
/// cleared, or the queue was paused/resumed. Carries the full
/// [`QueueSnapshot`] so the frontend can replace its state wholesale rather
/// than reconcile deltas (FR-030). Emitted on every persisted mutation.
#[derive(Debug, Clone, Serialize, specta::Type, tauri_specta::Event)]
pub struct QueueUpdated(pub QueueSnapshot);

/// Fine-grained, in-flight progress for a single converting job. Emitted
/// throttled (≥100 ms per job) and NOT persisted — it is advisory UI sugar on
/// top of the authoritative [`QueueUpdated`] snapshots. `percent` is in
/// `0.0..=1.0`; `phase` is a short label such as `"converting"`.
#[derive(Debug, Clone, Serialize, specta::Type, tauri_specta::Event)]
pub struct JobProgressEvent {
    /// Id of the job this progress belongs to.
    pub id: String,
    /// Short label of the step in progress, e.g. `"converting"`.
    pub phase: String,
    /// Fraction done, in `0.0..=1.0`.
    pub percent: f32,
}
