//! One JSONL line per interesting event, and nothing else.
//!
//! This code runs inside `explorer.exe`. It has no console, no tracing
//! subscriber and no right to slow the shell down, so logging is: best effort,
//! append-only, size-capped, and reached from exactly two places — a completed
//! `Invoke` and a caught panic. Nothing on the menu-drawing path logs.
//!
//! The file sits next to the app's own logs
//! (`<data_local_dir>/logs/shellext.jsonl`) so a bug report picks both up.

use std::io::Write;
use std::path::PathBuf;

/// Above this the file is restarted. A shell extension that fills the user's
/// disk with logs is a bug of its own.
const MAX_LOG_BYTES: u64 = 1024 * 1024;

fn log_path() -> Option<PathBuf> {
    let dir = directories::ProjectDirs::from("com", "Maecly", "MaeclyPrint")?
        .data_local_dir()
        .join("logs");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir.join("shellext.jsonl"))
}

fn write_line(line: &str) {
    let Some(path) = log_path() else { return };
    let truncate = std::fs::metadata(&path)
        .map(|m| m.len() > MAX_LOG_BYTES)
        .unwrap_or(false);
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(!truncate)
        .write(true)
        .truncate(truncate)
        .open(&path);
    if let Ok(mut file) = file {
        let _ = writeln!(file, "{line}");
    }
}

fn stamp() -> String {
    crate::handoff::format_rfc3339(time::OffsetDateTime::now_utc())
}

/// A menu entry was clicked and a handoff file was produced.
pub fn invoked(action: &str, count: usize, handoff: &std::path::Path) {
    let line = serde_json::json!({
        "ts": stamp(),
        "event": "shellext_invoked",
        "action": action,
        "files": count,
        "handoff": handoff.display().to_string(),
    });
    write_line(&line.to_string());
}

/// A menu entry was clicked but the handoff could not be delivered. The user
/// sees nothing happen, so this line is the only trace of why.
pub fn failed(what: &str, detail: &str) {
    let line = serde_json::json!({
        "ts": stamp(),
        "event": "shellext_failed",
        "where": what,
        "detail": detail,
    });
    write_line(&line.to_string());
}

/// A panic was caught at the COM boundary. Worth a line precisely because the
/// user experience is "the menu entry silently disappeared".
pub fn panicked(what: &'static str) {
    let line = serde_json::json!({
        "ts": stamp(),
        "event": "shellext_panic",
        "where": what,
    });
    write_line(&line.to_string());
}
