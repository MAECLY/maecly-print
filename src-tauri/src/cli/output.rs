//! Rendering results to stdout/stderr (FR-010).
//!
//! Two modes, one rule: with `--json` **stdout carries nothing but the JSON
//! document** (so `| jq` works), and errors go to stderr as
//! `{"error":{"code":…,"message":…}}` — the same `{code, message}` envelope the
//! webview receives, so a script and the UI never see two different names for
//! the same failure. Without `--json` we print a plain aligned table; the
//! architecture doc mentions `comfy-table`, which is not a dependency of this
//! crate, and one table helper is not worth one.

use serde::Serialize;

use crate::cli::exit::CliError;

/// Where and how a command reports. Constructed once from the parsed `--json`
/// flag and threaded through the command implementations.
#[derive(Debug, Clone, Copy)]
pub struct Emitter {
    /// `--json` was given: print JSON documents instead of human text.
    pub json: bool,
}

impl Emitter {
    /// An emitter for the given `--json` setting.
    pub fn new(json: bool) -> Self {
        Self { json }
    }

    /// Emit a machine-readable document, or fall back to `human` when not in
    /// JSON mode. The closure is only evaluated in human mode, so building an
    /// expensive table never costs anything in a scripted run.
    pub fn emit<T: Serialize>(&self, value: &T, human: impl FnOnce() -> String) {
        if self.json {
            println!("{}", to_json(value));
        } else {
            let text = human();
            if !text.is_empty() {
                println!("{text}");
            }
        }
    }

    /// Report a failure on stderr, in the same shape the webview receives.
    pub fn fail(&self, err: &CliError) {
        if self.json {
            eprintln!("{}", error_json(&err.error));
        } else {
            eprintln!("error: {}", err.error);
        }
    }
}

/// Pretty JSON for a value, falling back to a machine-readable internal error
/// rather than panicking — a serialization bug must not take the process down
/// after the work already happened.
pub fn to_json<T: Serialize>(value: &T) -> String {
    serde_json::to_string_pretty(value).unwrap_or_else(|e| {
        format!(r#"{{"error":{{"code":"Internal","message":"failed to serialize output: {e}"}}}}"#)
    })
}

/// The `{"error":{"code":…,"message":…}}` envelope documented in §3.6.
pub fn error_json(err: &crate::util::error::IpcError) -> String {
    to_json(&serde_json::json!({ "error": err }))
}

/// A left-aligned text table with a header row. Columns are padded to the
/// widest cell; the last column is never padded, so copy-pasting a name does
/// not drag trailing spaces along.
pub fn table(headers: &[&str], rows: &[Vec<String>]) -> String {
    let cols = headers.len();
    let mut widths: Vec<usize> = headers.iter().map(|h| h.chars().count()).collect();
    for row in rows {
        for (i, cell) in row.iter().enumerate().take(cols) {
            widths[i] = widths[i].max(cell.chars().count());
        }
    }

    let render = |cells: &[String]| -> String {
        let mut line = String::new();
        for (i, cell) in cells.iter().enumerate().take(cols) {
            if i + 1 == cols {
                line.push_str(cell);
            } else {
                line.push_str(&format!("{cell:<width$}  ", width = widths[i]));
            }
        }
        line.trim_end().to_string()
    };

    let mut out = vec![render(
        &headers.iter().map(|h| h.to_string()).collect::<Vec<_>>(),
    )];
    out.extend(rows.iter().map(|r| render(r)));
    out.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::error::IpcError;

    #[test]
    fn fr_010_error_envelope_matches_the_ipc_shape() {
        let json = error_json(&IpcError::ProfileNotFound("ghost".into()));
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["error"]["code"], "ProfileNotFound");
        assert_eq!(v["error"]["message"], "ghost");
    }

    #[test]
    fn fr_010_json_documents_are_parseable() {
        #[derive(Serialize)]
        struct Row {
            name: String,
        }
        let rows = vec![Row { name: "a".into() }, Row { name: "b".into() }];
        let v: serde_json::Value = serde_json::from_str(&to_json(&rows)).unwrap();
        assert!(v.is_array());
        assert_eq!(v[1]["name"], "b");
    }

    #[test]
    fn fr_010_table_aligns_columns_without_trailing_padding() {
        let text = table(
            &["NAME", "STATUS"],
            &[
                vec!["Front Desk".to_string(), "ready".to_string()],
                vec!["HP".to_string(), "offline".to_string()],
            ],
        );
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines[0], "NAME        STATUS");
        assert_eq!(lines[2], "HP          offline");
        assert!(lines.iter().all(|l| !l.ends_with(' ')));
    }
}
