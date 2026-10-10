//! macOS-only integration with the Finder.
//!
//! Two halves of one contract:
//!
//! - [`services`] declares and answers the NSService that puts "Print with
//!   Pluryprint" in the Finder's Services submenu (FR-001);
//! - [`handoff`] writes the selection into the same JSON handoff file the
//!   Windows shell extension produces, so both platforms hand the app a
//!   selection in exactly one format ([`crate::cli::handoff`] reads it).
//!
//! The service itself is declared in `src-tauri/Info.plist`, which the Tauri
//! CLI merges into the bundle's generated `Info.plist`. Nothing here is
//! reachable unless the app runs from that bundle: a `cargo run` binary has no
//! `Info.plist` and therefore no service, which is expected and is why
//! everything testable in this module is testable without AppKit.

pub mod handoff;
pub mod services;
