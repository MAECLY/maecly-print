//! OS-specific printer enumeration backends.
//!
//! - **Unix** (macOS, Linux): CUPS, driven through the `lpstat` / `lpoptions`
//!   / `ippfind` CLIs. We parse the locale-independent IPP keyword output
//!   (e.g. `printer-state=3`, `marker-levels=…`) rather than the localized
//!   human prose that `lpstat -l` prints.
//! - **Windows**: the print spooler via the `windows` crate (`EnumPrinters` +
//!   `DeviceCapabilities` + `GetDefaultPrinter`).
//!
//! Each backend exposes the same three functions returning the shared IPC
//! types from [`crate::ipc::printers`]. They return `anyhow::Result`; the IPC
//! layer maps failures to `IpcError`.
//!
//! Anything the two backends must agree on — today the printer-family
//! heuristic — lives here rather than being duplicated per platform, so the
//! same device cannot be classified differently on Windows and on macOS.
//!
//! [`macos`] is the exception to the "printer enumeration" theme: it holds the
//! Finder integration (the NSServices provider behind the "Print with
//! Pluryprint" context-menu entry). It lives here because it is OS-specific
//! glue of exactly the same kind — Objective-C runtime calls that must not be
//! compiled anywhere else — and the Windows half of the same feature is a
//! separate DLL crate outside `src-tauri` entirely.

#[cfg(unix)]
mod cups;
pub mod dnssd;
#[cfg(target_os = "macos")]
pub mod macos;
pub mod testpage;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
mod windows_dnssd;

use crate::ipc::printers::PrinterKind;

/// Keywords that mark a queue as *not* a physical paper printer. `xps` and
/// `onenote` only ever appear on Windows and `cups-pdf` only on CUPS, but
/// keeping one list means a queue shared between the two hosts is classified
/// the same way on both.
///
/// `fax` is deliberately here: a fax queue is not a destination Pluryprint
/// should offer when reassigning a document route, and the UI filters
/// `virtual` out of the reassign candidates.
const VIRTUAL_WORDS: &[&str] = &[
    "pdf",
    "xps",
    "onenote",
    "fax",
    "virtual",
    "cups-pdf",
    "print to file",
];
const LABEL_WORDS: &[&str] = &["zebra", "label", "zpl", "dymo"];
/// Note the Roland entries are spelled out per product line: a bare `versa`
/// also matches "Uni**versa**l", which is exactly how HP's universal driver is
/// named — it used to classify every HP queue on macOS as wide format.
const WIDE_WORDS: &[&str] = &[
    "designjet",
    "wide format",
    "plotter",
    "versacamm",
    "versauv",
    "versastudio",
    "versaexpress",
];
/// Inkjet families whose names do not say "multifunction". EcoTank models
/// (`EPSON L15160`, `ET-2850`) are matched by [`is_epson_ecotank`], since a
/// bare `l` or `et-` would match far too much.
const INKJET_WORDS: &[&str] = &[
    "inkjet",
    "ink jet",
    "ecotank",
    "eco tank",
    "smart tank",
    "ink tank",
    "inktank",
    "deskjet",
    "envy",
    "stylus",
    "maxify",
    "bubblejet",
];
/// Words that do say laser. Anything else that matches no family is a
/// generic [`PrinterKind::Printer`].
const LASER_WORDS: &[&str] = &["laser", "lbp", "i-sensys"];
const MFP_WORDS: &[&str] = &[
    "mfp",
    "workforce",
    "imagerunner",
    "all-in-one",
    "pixma",
    "officejet",
];

/// Infer the coarse printer family the UI uses to pick an icon (and to decide
/// whether a queue may receive reassigned routing rules).
///
/// `model` is the driver / make-and-model string, `name` the queue's own name
/// and `port_or_uri` the device URI (CUPS) or port (Windows). All three are
/// searched because no single one is reliable across platforms: Windows reports
/// a *driver* name that for modern devices is a generic universal driver
/// ("HP Smart Universal Printing Driver") while the queue name carries the
/// model, and CUPS reports the model but hides the transport in the URI.
///
/// Order matters: virtual wins over everything (a "Fax MFP" is still virtual),
/// then label, then wide format, then multifunction, then inkjet, then laser
/// (only when a name says so). A device that matches nothing is a generic
/// printer: the old fallback called every unknown device a laser, an EPSON
/// EcoTank inkjet included.
pub fn infer_kind(model: &str, name: &str, port_or_uri: Option<&str>) -> PrinterKind {
    let haystack = format!(
        "{} {} {}",
        model.to_ascii_lowercase(),
        name.to_ascii_lowercase(),
        port_or_uri.unwrap_or("").to_ascii_lowercase(),
    );
    let has = |words: &[&str]| words.iter().any(|w| haystack.contains(w));
    if has(VIRTUAL_WORDS) {
        PrinterKind::Virtual
    } else if has(LABEL_WORDS) {
        PrinterKind::Label
    } else if has(WIDE_WORDS) {
        PrinterKind::Wide
    } else if has(MFP_WORDS) {
        PrinterKind::Mfp
    } else if has(INKJET_WORDS) || is_epson_ecotank(&haystack) {
        PrinterKind::Inkjet
    } else if has(LASER_WORDS) {
        PrinterKind::Laser
    } else {
        PrinterKind::Printer
    }
}

/// An EPSON EcoTank model in a lower-cased name: `epson` plus a word that is
/// `l` followed by digits (`L15160`, `L3250`) or `et-` followed by a digit
/// (`ET-2850`). EPSON's `LQ`/`LX` impact printers do not match.
fn is_epson_ecotank(haystack: &str) -> bool {
    haystack.contains("epson")
        && haystack
            .split(|c: char| !c.is_ascii_alphanumeric() && c != '-')
            .any(|w| {
                let l_series = w.strip_prefix('l').is_some_and(|rest| {
                    rest.len() >= 3 && rest.chars().all(|c| c.is_ascii_digit())
                });
                let et_series = w
                    .strip_prefix("et-")
                    .is_some_and(|rest| rest.chars().next().is_some_and(|c| c.is_ascii_digit()));
                l_series || et_series
            })
}

#[cfg(unix)]
pub use cups::{
    add_printer, default_printer, discover_printers, list_printers, open_printer_settings,
    print_test_page, set_os_default,
};

#[cfg(windows)]
pub use windows::{
    add_printer, default_printer, discover_printers, list_printers, open_printer_settings,
    paper_size_for_driver_name, print_test_page, set_os_default,
};

// Fallback for any other target so the crate always compiles.
#[cfg(not(any(unix, windows)))]
pub fn list_printers() -> anyhow::Result<Vec<crate::ipc::printers::Printer>> {
    Ok(Vec::new())
}
#[cfg(not(any(unix, windows)))]
pub fn default_printer() -> anyhow::Result<Option<String>> {
    Ok(None)
}
#[cfg(not(any(unix, windows)))]
pub fn discover_printers() -> anyhow::Result<Vec<crate::ipc::printers::DiscoveredPrinter>> {
    Ok(Vec::new())
}
#[cfg(not(any(unix, windows)))]
pub fn set_os_default(_name: &str) -> anyhow::Result<()> {
    Ok(())
}
#[cfg(not(any(unix, windows)))]
pub fn add_printer(_name: &str, _uri: &str) -> anyhow::Result<()> {
    Ok(())
}
#[cfg(not(any(unix, windows)))]
pub fn print_test_page(_name: &str) -> anyhow::Result<()> {
    Ok(())
}
#[cfg(not(any(unix, windows)))]
pub fn open_printer_settings() -> anyhow::Result<()> {
    Ok(())
}

/// What the print engine needs to know about an enumerated printer.
///
/// Built from the same [`crate::ipc::printers::Printer`] the Printers screen
/// shows, so the sizes a user can pick there are exactly the ones a job is
/// validated against. Colour models are read separately on CUPS (they are
/// not part of the wire `Printer`); see [`printer_color_models`].
///
/// On CUPS the duplex and colour answers come from a fresh, status-checked
/// probe ([`cups::engine_caps`]): a failed probe is "unknown", so it cannot
/// pause a duplex job as unsupported.
pub fn printer_caps(p: &crate::ipc::printers::Printer) -> crate::engine::print::PrinterCaps {
    #[cfg(unix)]
    {
        cups::engine_caps(p)
    }
    #[cfg(not(unix))]
    {
        // Windows driver paper names are localized ("Carta"), so the gate
        // also gets the canonical name of every sheet the driver lists by id.
        #[cfg(windows)]
        let papers = windows::caps_papers(&p.id, &p.papers);
        #[cfg(not(windows))]
        let papers = p.papers.clone();
        crate::engine::print::PrinterCaps {
            duplex: p.duplex,
            papers,
            color_models: printer_color_models(&p.id),
        }
    }
}

/// The spooler queue id and capabilities of the printer named `name` (queue
/// id or display name), matched exactly as the dispatcher and the CLI match it
/// ([`crate::ipc::printers::match_installed`]): a display name resolves to its
/// queue id, so a preview names the queue `lp -d` will really receive. When
/// enumeration fails or the name is missing or ambiguous, the name is kept as
/// typed with [`crate::engine::print::PrinterCaps::unknown`] — the same
/// best-effort stance the dispatcher takes.
pub fn queue_of(name: &str) -> (String, crate::engine::print::PrinterCaps) {
    use crate::ipc::printers::{match_installed, PrinterMatch};
    let list = list_printers().unwrap_or_default();
    match match_installed(&list, name) {
        PrinterMatch::Found(p) => (p.id.clone(), printer_caps(p)),
        PrinterMatch::Ambiguous(_) | PrinterMatch::Missing => (
            name.to_string(),
            crate::engine::print::PrinterCaps::unknown(),
        ),
    }
}

/// The CUPS `ColorModel` choices of queue `id` (empty when not reported, and
/// always empty on Windows, where colour is a DEVMODE flag, not a model).
pub fn printer_color_models(id: &str) -> Vec<String> {
    #[cfg(unix)]
    {
        cups::color_models(id)
    }
    #[cfg(not(unix))]
    {
        let _ = id;
        Vec::new()
    }
}

/// Map a driver/PWG media name onto the closest [`PaperSize`] the print engine
/// can express.
///
/// Per-printer `paper` overrides are free-form driver strings (`A4`,
/// `na_letter_8.5x11in`, `Letter`, `iso_a4_210x297mm`, `US Legal`…), while the
/// engine speaks a closed enum, so the match is on substrings after
/// normalization. Anything unrecognized falls back to A4 — the same default
/// [`JobSettings`](crate::queue::job::JobSettings) already uses — rather than
/// guessing.
pub fn media_paper_size(paper: Option<&str>) -> crate::queue::job::PaperSize {
    use crate::queue::job::PaperSize;
    let p = paper
        .unwrap_or("")
        .to_ascii_lowercase()
        .replace(['_', '-'], "");
    if p.contains("legal") {
        PaperSize::LEGAL
    } else if p.contains("letter") {
        PaperSize::LETTER
    } else if p.contains("a3") {
        PaperSize::A3
    } else {
        PaperSize::A4
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fr_media_paper_size_accepts_driver_and_pwg_names() {
        use crate::queue::job::PaperSize;
        assert_eq!(media_paper_size(Some("A4")), PaperSize::A4);
        assert_eq!(media_paper_size(Some("iso_a4_210x297mm")), PaperSize::A4);
        assert_eq!(media_paper_size(Some("Letter")), PaperSize::LETTER);
        assert_eq!(
            media_paper_size(Some("na_letter_8.5x11in")),
            PaperSize::LETTER
        );
        assert_eq!(media_paper_size(Some("US Legal")), PaperSize::LEGAL);
        assert_eq!(media_paper_size(Some("iso_a3_297x420mm")), PaperSize::A3);
        // Unknown media and "no override at all" both fall back to A4.
        assert_eq!(media_paper_size(Some("Photo 4x6")), PaperSize::A4);
        assert_eq!(media_paper_size(None), PaperSize::A4);
    }

    /// Every row is `(model, name, port_or_uri, expected)`. The table is the
    /// contract both backends now share, so a keyword can never again exist on
    /// one platform only.
    #[test]
    fn fr_infer_kind_is_identical_on_both_platforms() {
        let cases: &[(&str, &str, Option<&str>, PrinterKind)] = &[
            // Virtual queues, however each OS spells them.
            (
                "Microsoft Print to PDF",
                "Microsoft Print to PDF",
                Some("PORTPROMPT:"),
                PrinterKind::Virtual,
            ),
            (
                "Generic / Text Only",
                "Save as PDF",
                Some("cups-pdf:/"),
                PrinterKind::Virtual,
            ),
            (
                "Microsoft XPS Document Writer v4",
                "XPS Writer",
                None,
                PrinterKind::Virtual,
            ),
            ("Send to OneNote 16", "OneNote", None, PrinterKind::Virtual),
            // A fax is virtual on BOTH platforms now (it used to be `laser` on
            // CUPS, which let routing rules be reassigned onto a fax queue).
            ("Brother MFC Fax", "Fax", None, PrinterKind::Virtual),
            // Label printers.
            ("Zebra ZD420 ZPL", "Zebra", None, PrinterKind::Label),
            ("DYMO LabelWriter 450", "Dymo", None, PrinterKind::Label),
            // Wide format — `versa` used to be CUPS-only.
            ("HP DesignJet T650", "Plotter", None, PrinterKind::Wide),
            ("Roland VersaCAMM", "Roland", None, PrinterKind::Wide),
            // Multifunction — `officejet`/`imagerunner`/`all-in-one` used to be
            // CUPS-only, so the same device read `laser` on Windows.
            (
                "HP Smart Universal Printing Driver",
                "HP OfficeJet Pro 9015",
                Some("IPP://10.0.0.9"),
                PrinterKind::Mfp,
            ),
            (
                "Canon iR-ADV C3826",
                "Canon imageRUNNER",
                None,
                PrinterKind::Mfp,
            ),
            ("Canon PIXMA MFP", "Canon", None, PrinterKind::Mfp),
            ("EPSON WorkForce WF-7840", "Epson", None, PrinterKind::Mfp),
            // Laser only when a name says so.
            (
                "HP LaserJet Pro M404",
                "Office laser",
                Some("USB001"),
                PrinterKind::Laser,
            ),
            ("Canon LBP6030", "Canon", None, PrinterKind::Laser),
            // Inkjet: the EcoTank behind the Microsoft IPP class driver used to
            // read "laser".
            (
                "Microsoft IPP Class Driver",
                "EPSON L15160 (IPP)",
                Some("WSD-1"),
                PrinterKind::Inkjet,
            ),
            ("EPSON ET-2850 Series", "Epson", None, PrinterKind::Inkjet),
            ("HP DeskJet 2700", "HP", None, PrinterKind::Inkjet),
            // An EPSON impact printer is not an EcoTank.
            ("EPSON LQ-590II", "Epson", None, PrinterKind::Printer),
            // Nothing in the names: a generic printer, not a guessed laser.
            (
                "Generic PCL",
                "Front desk",
                Some("USB002"),
                PrinterKind::Printer,
            ),
        ];
        for (model, name, uri, expected) in cases {
            let got = infer_kind(model, name, *uri);
            assert_eq!(
                std::mem::discriminant(&got),
                std::mem::discriminant(expected),
                "infer_kind({model:?}, {name:?}, {uri:?}) = {got:?}, expected {expected:?}",
            );
        }
    }

    #[test]
    fn fr_infer_kind_prefers_virtual_over_every_other_family() {
        // A "Fax MFP" must not be offered as a reassign target just because it
        // also matches the multifunction list.
        assert!(matches!(
            infer_kind("Sharp MX-3071 Fax MFP", "Sharp fax", None),
            PrinterKind::Virtual
        ));
    }
}
