//! `pluryprint printers …`
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
///
/// The queue **id** comes first because it is what the spooler addresses;
/// the display name (CUPS `printer-info`) can differ from it and is not
/// unique. `--printer` accepts either, but the id is never ambiguous.
fn render(printers: &[Printer]) -> String {
    let rows: Vec<Vec<String>> = printers
        .iter()
        .map(|p| {
            vec![
                p.id.clone(),
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
        &[
            "ID",
            "NAME",
            "STATUS",
            "DEFAULT",
            "COLOR",
            "DUPLEX",
            "CONNECTION",
        ],
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ipc::printers::{PrinterKind, Supplies};

    /// FR-044: the human listing shows the queue id next to the display name,
    /// so what a user copies into `--printer` is something the spooler knows.
    #[test]
    fn fr_044_printers_list_shows_the_queue_id_and_the_display_name() {
        let p = Printer {
            id: "HP_LaserJet_Pro_M404".into(),
            name: "HP LaserJet Pro M404".into(),
            kind: PrinterKind::Laser,
            color: false,
            duplex: true,
            status: PrinterStatus::Ready,
            is_default: true,
            location: String::new(),
            connection: "USB".into(),
            driver: String::new(),
            shared: false,
            queued_jobs: 0,
            max_dpi: None,
            papers: vec![],
            trays: vec![],
            supplies: Supplies::default(),
            default_tray: None,
            default_paper: None,
        };
        let out = render(&[p]);
        let mut lines = out.lines();
        assert!(lines.next().unwrap().starts_with("ID"));
        let row = lines.next().unwrap();
        assert!(row.starts_with("HP_LaserJet_Pro_M404"), "{row}");
        assert!(row.contains("HP LaserJet Pro M404"), "{row}");
    }
}
