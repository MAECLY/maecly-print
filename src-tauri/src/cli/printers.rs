//! `maeclyprint printers …`
//!
//! A read-only view of what the OS spooler reports, straight from
//! [`crate::platform`] — the same enumeration the Printers screen shows, so a
//! script and the UI can never disagree about which printers exist.

use crate::cli::args::PrintersCmd;
use crate::cli::exit::{CliError, ExitCode};
use crate::cli::output::{table, Emitter};
use crate::ipc::printers::{Printer, PrinterStatus};
use crate::util::error::IpcError;

/// Run a `printers` subcommand.
pub fn run(emitter: Emitter, cmd: PrintersCmd) -> Result<(), CliError> {
    match cmd {
        PrintersCmd::List => {
            // Enumeration failing is an environment problem (no spooler, no
            // CUPS), not a usage error.
            let list = crate::platform::list_printers()
                .map_err(|e| CliError::new(ExitCode::Printer, IpcError::Internal(e.to_string())))?;
            emitter.emit(&list, || render(&list));
            Ok(())
        }
    }
}

/// Human-mode rendering: one row per printer, capabilities spelled out.
fn render(printers: &[Printer]) -> String {
    let rows: Vec<Vec<String>> = printers
        .iter()
        .map(|p| {
            vec![
                p.name.clone(),
                status_name(p.status).to_string(),
                if p.is_default { "yes" } else { "" }.to_string(),
                if p.color { "color" } else { "mono" }.to_string(),
                if p.duplex { "duplex" } else { "simplex" }.to_string(),
                p.connection.clone(),
            ]
        })
        .collect();
    table(
        &["NAME", "STATUS", "DEFAULT", "COLOR", "DUPLEX", "CONNECTION"],
        &rows,
    )
}

/// The wire spelling of a status, matching the generated TypeScript bindings.
fn status_name(status: PrinterStatus) -> &'static str {
    match status {
        PrinterStatus::Ready => "ready",
        PrinterStatus::Offline => "offline",
        PrinterStatus::Error => "error",
        PrinterStatus::Busy => "busy",
    }
}
