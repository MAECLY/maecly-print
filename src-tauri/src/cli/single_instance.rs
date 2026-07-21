//! Single instance (NFR-014): what the *second* launch hands to the first.
//!
//! Two MaeclyPrint processes must never run at once. Both would recover the
//! same `queue.json` into their own memory and rewrite it from there, and — the
//! damage that actually loses work — the second one's boot-time orphan sweep
//! (`queue::sweep_orphans`) deletes every cached PDF the first one's live jobs
//! have not yet printed, because they are not referenced by *its* copy of the
//! queue.
//!
//! `tauri-plugin-single-instance` closes that hole: the second process finds
//! the first, hands over its `argv` and exits. What is left to decide is what
//! that argv *means*, which is what [`interpret`] does — and it is a pure
//! function of the argv, so the interesting behaviour is testable without ever
//! starting two processes.
//!
//! Note what does *not* come through here: a headless `maeclyprint print …`
//! never builds a Tauri app, so it never registers the plugin and is never
//! forwarded. It also never touches `queue.json` or the shared conversion cache
//! ([`crate::cli::print`]), so it cannot race a running app either.

use crate::cli::args::Cli;
use crate::cli::{selection_from, Selection};

use clap::Parser;

/// Interpret the argv a second launch forwarded.
///
/// Returns the selection to merge into the running instance, or `None` when the
/// launch carried nothing usable (a plain re-launch, a malformed command line,
/// a stale or foreign handoff file). `None` is not an error: the running window
/// is still raised, which is what the user asked for by launching the app.
pub fn interpret<S: AsRef<str>>(argv: &[S]) -> Option<Selection> {
    let argv: Vec<String> = argv.iter().map(|a| a.as_ref().to_string()).collect();

    // Only a context-menu invocation carries a selection; anything else is a
    // plain re-launch. Classifying first keeps a stray `--help` in a forwarded
    // argv from being answered by a running GUI.
    if super::classify(argv.get(1..).unwrap_or_default()) != super::Intent::ContextMenu {
        return None;
    }

    match Cli::try_parse_from(&argv).ok().as_ref().map(selection_from) {
        Some(Ok(selection)) => Some(selection),
        Some(Err(err)) => {
            tracing::warn!(
                event = "second_instance_selection_rejected",
                error = %err.error,
                "a forwarded launch carried a selection we could not honour"
            );
            None
        }
        None => {
            tracing::warn!(
                event = "second_instance_argv_unparsed",
                "a forwarded launch had an unparseable command line"
            );
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nfr_014_plain_relaunch_carries_no_selection() {
        assert!(interpret(&["maeclyprint"]).is_none());
        assert!(interpret(&["maeclyprint", "-psn_0_1234"]).is_none());
        // A subcommand can never reach this path (headless never builds an
        // app), and if one somehow did we must not act on it.
        assert!(interpret(&["maeclyprint", "print", "--files", "a.pdf"]).is_none());
    }

    #[test]
    fn nfr_014_context_menu_without_a_handoff_is_rejected() {
        assert!(interpret(&["maeclyprint", "--invoke=context-menu"]).is_none());
    }

    #[test]
    fn nfr_014_foreign_handoff_file_is_rejected() {
        // A payload outside the handoff directory is refused even when it is
        // otherwise perfect: it is the containment check that stops another
        // process from planting one.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sel.json");
        std::fs::write(
            &path,
            format!(
                r#"{{"schema_version":1,"source":"test","invoked_at":"{}","paths":["/tmp/a.pdf"]}}"#,
                crate::queue::job::now_rfc3339()
            ),
        )
        .unwrap();

        let argv = vec![
            "maeclyprint".to_string(),
            "--invoke=context-menu".to_string(),
            "--files-from".to_string(),
            path.to_string_lossy().into_owned(),
        ];
        assert!(interpret(&argv).is_none());
        assert!(path.exists(), "a refused payload is not consumed");
    }
}
