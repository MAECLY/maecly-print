//! Windows print quality through a temporary per-user DEVMODE (decision D16,
//! `docs/03-requirements/print-settings.md`): a DEVMODE built from a
//! PrintTicket that carries the requested quality is made the user default
//! for the length of one job, under a named mutex and a journal, and restored
//! as soon as the job is in the queue; the job is then checked for what it
//! really got.
//!
//! **Stub (round-3 contract, owned by lane L6).**
