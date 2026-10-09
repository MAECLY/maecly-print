//! The macOS Finder integration: an NSServices provider (FR-001, FR-003).
//!
//! macOS has no direct equivalent of a Windows shell extension DLL. What it
//! has is *Services*: an app declares one in its `Info.plist`, and the system
//! puts it in the **Services** submenu of every context menu — including the
//! Finder's — for any selection whose type matches. When the user picks it,
//! AppKit delivers the selection to a *services provider* object living inside
//! the app process.
//!
//! That last part is what makes this cheap: there is no separate binary, no
//! `.appex`, no App Group, no sandbox, no second code signature, and nothing
//! for the user to switch on. See `docs/04-architecture/shell-extensions.md` §3
//! and `shell-ext-mac/README.md` for the full comparison with Finder Sync.
//!
//! ## Why the selection is not passed back through argv
//!
//! On Windows the extension is a DLL inside Explorer, so it *must* start (or
//! signal) the app: it writes a handoff file and launches
//! `maeclyprint --invoke=context-menu --files-from <PATH>`. On macOS the
//! service is answered by the app itself — Launch Services starts it if it is
//! not running and reuses it if it is, which is the same single-instance
//! guarantee `tauri-plugin-single-instance` provides on Windows. Relaunching
//! our own binary from inside our own process to tell ourselves something would
//! add a process spawn, a second window race and a code path with no test.
//!
//! So the selection is delivered in-process — but it still travels *through the
//! handoff file* (see [`super::handoff`]), and it is still read back through
//! `cli::handoff`'s validation gate. The transport is redundant here; the
//! validation is not, and having one contract instead of two is the point.

use std::panic::AssertUnwindSafe;
use std::path::PathBuf;

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol};
use objc2::{define_class, msg_send, ClassType, DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{NSApplication, NSPasteboard, NSUpdateDynamicServices};
use objc2_foundation::{NSArray, NSString, NSURL};

use crate::cli::args::ContextAction;
use crate::cli::handoff;

/// State the provider needs to hand a selection to the running app.
struct Ivars {
    app: tauri::AppHandle,
}

define_class!(
    // SAFETY:
    // - `NSObject` imposes no subclassing requirements.
    // - The type does not implement `Drop`.
    #[unsafe(super(NSObject))]
    // AppKit dispatches service messages on the main thread, and the object is
    // only ever created from `register` (itself main-thread-only), so the
    // stricter thread kind documents and enforces the real contract.
    #[thread_kind = MainThreadOnly]
    // Fixed runtime name: it shows up in crash reports and in `pbs` output, and
    // a generated name (which embeds the crate version) would change on every
    // release for no reason.
    #[name = "MaeclyPrintServicesProvider"]
    #[ivars = Ivars]
    struct ServicesProvider;

    impl ServicesProvider {
        /// `-(void)printWithMaeclyPrint:(NSPasteboard *)pb
        ///                      userData:(NSString *)ud
        ///                         error:(NSString **)err`
        ///
        /// The selector is derived from `NSMessage` in `Info.plist`
        /// (`printWithMaeclyPrint`) plus the `userData:error:` tail AppKit
        /// always appends; the two must be changed together.
        #[unsafe(method(printWithMaeclyPrint:userData:error:))]
        fn print_with_maecly_print(
            &self,
            pasteboard: &NSPasteboard,
            user_data: Option<&NSString>,
            _error: *mut *mut NSString,
        ) {
            // A panic must not unwind into an Objective-C frame: that is UB.
            // This seatbelt only matters where panics *unwind* — dev and test
            // builds. Release builds use `panic = "abort"`, so there a panic
            // never returns here: the process aborts right after
            // `logging::install_panic_hook` has written the panic to the log
            // file, which is the trace a release crash leaves. Nothing here is
            // expected to panic either way.
            let outcome = std::panic::catch_unwind(AssertUnwindSafe(|| {
                let action = action_from_user_data(user_data.map(|s| s.to_string()).as_deref());
                let paths = selection_from_pasteboard(pasteboard);
                deliver(&self.ivars().app, paths, action);
            }));
            if outcome.is_err() {
                tracing::error!(
                    event = "macos_service_panicked",
                    "the Finder service handler panicked; the selection was dropped"
                );
            }
        }
    }

    unsafe impl NSObjectProtocol for ServicesProvider {}
);

impl ServicesProvider {
    fn new(mtm: MainThreadMarker, app: tauri::AppHandle) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(Ivars { app });
        unsafe { msg_send![super(this), init] }
    }
}

/// Register the provider with AppKit so the Finder's Services menu reaches us.
///
/// Must be called on the main thread (Tauri's `setup` hook is), and *after*
/// [`crate::cli::pending::PendingSelections`] is managed, so a service message
/// arriving immediately has somewhere to land.
///
/// Registration is a no-op in `cargo test` and in any non-bundled run: without
/// an `Info.plist` declaring the service, macOS never offers it. That is why
/// this function is not what the tests below exercise.
pub fn register(app: &tauri::AppHandle) -> anyhow::Result<()> {
    let Some(mtm) = MainThreadMarker::new() else {
        anyhow::bail!("the Finder services provider must be registered on the main thread");
    };

    let provider = ServicesProvider::new(mtm, app.clone());
    let ns_app = NSApplication::sharedApplication(mtm);
    let object: &AnyObject = &provider;
    // SAFETY: the provider outlives the application (leaked below) and
    // implements the selector named by `NSMessage` in `Info.plist`.
    unsafe { ns_app.setServicesProvider(Some(object)) };
    // AppKit does not take ownership of the services provider, so the object
    // must outlive this scope. It is a single allocation that lives exactly as
    // long as the process, which is what leaking it says.
    std::mem::forget(provider);

    // Tell the system to re-read this bundle's service declarations. Without
    // it a freshly built app can sit in the Services cache with the previous
    // build's (or no) entries — the single most confusing part of developing
    // this integration.
    NSUpdateDynamicServices();

    tracing::info!(
        event = "macos_services_provider_registered",
        class = ServicesProvider::NAME
    );
    Ok(())
}

/// Map the `NSUserData` string of the invoked menu entry onto an action.
///
/// The strings are deliberately the CLI's `--action` vocabulary, so the two
/// entry points cannot drift; an unknown value falls back to opening the dialog
/// rather than printing something the user did not ask for.
pub fn action_from_user_data(raw: Option<&str>) -> ContextAction {
    match raw.map(str::trim) {
        Some("configure") => ContextAction::Configure,
        Some("print-default") => ContextAction::PrintDefault,
        Some("queue") => ContextAction::Queue,
        other => {
            if let Some(value) = other.filter(|v| !v.is_empty()) {
                tracing::warn!(
                    event = "macos_service_unknown_action",
                    value,
                    "falling back to opening the print dialog"
                );
            }
            ContextAction::Configure
        }
    }
}

/// Read the file selection off the service pasteboard.
///
/// `readObjectsForClasses:options:` returns the *whole* selection — there is no
/// documented ceiling, which is what FR-003 needs — as `NSURL`s.
fn selection_from_pasteboard(pasteboard: &NSPasteboard) -> Vec<PathBuf> {
    let classes = NSArray::from_slice(&[NSURL::class()]);
    // SAFETY: `classes` holds exactly the class object the generic expects and
    // no read options are passed.
    let objects = unsafe { pasteboard.readObjectsForClasses_options(&classes, None) };
    match objects {
        Some(objects) => paths_from_objects(&objects),
        None => {
            tracing::warn!(
                event = "macos_service_empty_pasteboard",
                "the Finder handed us a selection with no file URLs"
            );
            Vec::new()
        }
    }
}

/// Turn the pasteboard's objects into filesystem paths, in order and without
/// duplicates.
///
/// Anything that is not a *file* URL is skipped rather than guessed at: a
/// service pasteboard can legally carry other flavours, and a non-file URL has
/// no path to print. This mirrors the Windows side, where `SIGDN_FILESYSPATH`
/// fails for virtual shell items and they are dropped for the same reason.
fn paths_from_objects(objects: &NSArray<AnyObject>) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::with_capacity(objects.count());
    for index in 0..objects.count() {
        let object = objects.objectAtIndex(index);
        let Some(url) = object.downcast_ref::<NSURL>() else {
            continue;
        };
        if !url.isFileURL() {
            continue;
        }
        let Some(path) = url.path() else {
            continue;
        };
        let path = PathBuf::from(path.to_string());
        // The Finder does not hand out the same item twice, but a selection
        // assembled by another app might; the queue must not print it twice.
        if !out.contains(&path) {
            out.push(path);
        }
    }
    out
}

/// Hand a selection to the running app.
///
/// Every failure is logged and swallowed: by the time this runs the app window
/// is already on screen, so the worst outcome is an empty window the user can
/// still work in — never a crash inside a Finder-initiated call.
fn deliver(app: &tauri::AppHandle, paths: Vec<PathBuf>, action: ContextAction) {
    if paths.is_empty() {
        return;
    }
    let count = paths.len();
    let dir = handoff::handoff_dir();

    let payload = super::handoff::payload(&paths, action, None, time::OffsetDateTime::now_utc());
    let file = match super::handoff::write(&dir, &payload) {
        Ok(file) => file,
        Err(e) => {
            tracing::error!(event = "macos_service_handoff_write_failed", error = %e, count);
            return;
        }
    };

    // Read it back through the app's own gate rather than trusting the payload
    // we just built. It costs one file read and it means the Finder path is
    // covered by the same containment/freshness/cap rules a Windows Explorer
    // invocation goes through — the rules that actually matter, since anything
    // in the handoff directory is written by a process we do not control.
    if !handoff::is_within_dir(&dir, &file) {
        tracing::error!(event = "macos_service_handoff_escaped_dir", path = %file.display());
        let _ = std::fs::remove_file(&file);
        return;
    }
    let payload = match handoff::consume(&file) {
        Ok(payload) => payload,
        Err(e) => {
            tracing::error!(event = "macos_service_handoff_unreadable", error = %e.error);
            return;
        }
    };
    if !handoff::is_fresh(
        &payload.invoked_at,
        time::OffsetDateTime::now_utc(),
        handoff::MAX_HANDOFF_AGE,
    ) {
        tracing::error!(event = "macos_service_handoff_stale", invoked_at = %payload.invoked_at);
        return;
    }

    let selection = crate::cli::selection_from_payload(payload, action, None);
    deliver_selection(app, selection, count);
}

/// Park a validated selection for the window and wake an already-open one.
///
/// Split from [`deliver`] because it is the delivery contract itself — park,
/// then notify, then focus — with none of the file I/O, and it is shared with
/// nothing else only because Windows delivers through the single-instance
/// plugin instead.
fn deliver_selection(app: &tauri::AppHandle, selection: crate::cli::Selection, count: usize) {
    use tauri::Manager;
    use tauri_specta::Event;

    match app.try_state::<crate::cli::pending::PendingSelections>() {
        Some(pending) => pending.push(selection.clone()),
        None => {
            tracing::warn!(
                event = "selection_dropped_before_setup",
                "a Finder selection arrived before the app finished starting"
            );
            return;
        }
    }
    if let Err(e) = crate::cli::pending::SelectionForwarded(selection).emit(app) {
        tracing::warn!(event = "selection_forwarded_emit_failed", error = %e);
    }
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
    tracing::info!(event = "macos_service_invoked", count);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(path: &str) -> Retained<NSURL> {
        NSURL::fileURLWithPath(&NSString::from_str(path))
    }

    /// The `Info.plist` that ships in the bundle. Read from the source tree, so
    /// the test is deterministic and needs neither a build nor a bundle.
    const INFO_PLIST: &str = include_str!("../../../Info.plist");

    /// Every `<string>` that follows a `<key>NAME</key>` in the plist.
    ///
    /// A deliberately dumb scanner rather than a plist parser: it needs no new
    /// dependency, and the file it reads is one we write.
    fn plist_values(key: &str) -> Vec<String> {
        let needle = format!("<key>{key}</key>");
        INFO_PLIST
            .match_indices(&needle)
            .filter_map(|(at, _)| {
                let rest = &INFO_PLIST[at + needle.len()..];
                let open = rest.find("<string>")? + "<string>".len();
                let close = rest[open..].find("</string>")?;
                Some(rest[open..open + close].to_string())
            })
            .collect()
    }

    /// FR-001, and the failure this whole integration is most likely to hit:
    /// `NSMessage` in the plist and the selector compiled into the binary must
    /// name the same method. When they disagree the menu entry still appears
    /// and simply does nothing, with no error anywhere — so it is asserted
    /// here, from the two artefacts themselves.
    #[test]
    fn fr_001_every_declared_service_is_implemented_by_the_provider() {
        let class = ServicesProvider::class();
        assert_eq!(
            class.name().to_str().unwrap(),
            "MaeclyPrintServicesProvider"
        );

        let messages = plist_values("NSMessage");
        assert_eq!(messages.len(), 2, "Info.plist declares two menu entries");
        for message in messages {
            // AppKit always appends `userData:error:` to the NSMessage name.
            let selector = format!("{message}:userData:error:");
            let name = std::ffi::CString::new(selector.clone()).unwrap();
            assert!(
                class
                    .instance_method(objc2::runtime::Sel::register(&name))
                    .is_some(),
                "Info.plist declares NSMessage {message:?} but the provider has \
                 no -{selector} method"
            );
        }
    }

    /// The other half of the same contract: every `NSUserData` in the plist is
    /// an action the provider recognizes, so a typo cannot silently degrade a
    /// "print now" entry into "open the dialog".
    #[test]
    fn fr_001_every_declared_menu_entry_maps_to_a_real_action() {
        let user_data = plist_values("NSUserData");
        assert_eq!(user_data.len(), 2);
        assert_eq!(
            action_from_user_data(Some(&user_data[0])),
            ContextAction::Configure
        );
        assert_eq!(
            action_from_user_data(Some(&user_data[1])),
            ContextAction::PrintDefault
        );
        // NSPortName must be the bundle's CFBundleName, which Tauri derives
        // from `productName` in tauri.conf.json.
        for port in plist_values("NSPortName") {
            assert_eq!(port, "MaeclyPrint");
        }
    }

    /// FR-001: the two menu entries declared in `Info.plist` map onto the two
    /// actions the app implements, using the same words the CLI uses.
    #[test]
    fn fr_001_user_data_selects_the_action() {
        assert_eq!(
            action_from_user_data(Some("configure")),
            ContextAction::Configure
        );
        assert_eq!(
            action_from_user_data(Some("print-default")),
            ContextAction::PrintDefault
        );
        assert_eq!(action_from_user_data(Some("queue")), ContextAction::Queue);
    }

    /// An entry we do not recognize (a stale Services cache still holding a
    /// previous build's declaration) must open the dialog, never print.
    #[test]
    fn fr_001_an_unknown_action_opens_the_dialog() {
        assert_eq!(action_from_user_data(None), ContextAction::Configure);
        assert_eq!(action_from_user_data(Some("")), ContextAction::Configure);
        assert_eq!(
            action_from_user_data(Some("delete-everything")),
            ContextAction::Configure
        );
        // AppKit hands the value straight from the plist, whitespace included.
        assert_eq!(
            action_from_user_data(Some(" print-default ")),
            ContextAction::PrintDefault
        );
    }

    /// FR-001/FR-004: files *and* folders come off the pasteboard as absolute
    /// paths, in the order the Finder listed them — separators and quotes in a
    /// name included, since those are what a delimited argv could not carry.
    #[test]
    fn fr_001_file_urls_become_absolute_paths_in_order() {
        let a = url("/tmp/b.pdf");
        let b = url("/tmp/a folder");
        let c = url("/tmp/name, with \"quotes\".docx");
        let objects: [&AnyObject; 3] = [&a, &b, &c];

        assert_eq!(
            paths_from_objects(&NSArray::from_slice(&objects)),
            vec![
                PathBuf::from("/tmp/b.pdf"),
                PathBuf::from("/tmp/a folder"),
                PathBuf::from("/tmp/name, with \"quotes\".docx"),
            ]
        );
    }

    /// A non-ASCII name comes back **decomposed** (NFD), because that is how
    /// `NSURL` reports a path on a macOS volume: `ñ` arrives as `n` + U+0303,
    /// not as the precomposed U+00F1 a user would type.
    ///
    /// This is asserted rather than normalized away for two reasons: the NFD
    /// path is the one that opens the file, which is all the print engine
    /// needs; and comparing these strings to a precomposed one — a path typed
    /// into the CLI, or one read back from `queue.json` written on Windows —
    /// will *not* match, so anything that dedupes across sources has to
    /// compare canonicalized paths, never raw strings.
    #[test]
    fn fr_005_unicode_names_arrive_in_the_filesystems_decomposed_form() {
        let dir = tempfile::tempdir().unwrap();
        // Written precomposed (NFC), the way a user types it.
        let precomposed = dir.path().join("facturas ñ 2026.pdf");
        std::fs::write(&precomposed, b"%PDF-1.4\n").unwrap();

        let from_finder =
            paths_from_objects(&NSArray::from_slice(&[
                &*url(&precomposed.to_string_lossy()) as &AnyObject,
            ]));
        let reported = &from_finder[0];

        assert_ne!(
            reported.to_string_lossy(),
            precomposed.to_string_lossy(),
            "NSURL is expected to report the decomposed form"
        );
        assert!(reported.to_string_lossy().contains("n\u{303}"));
        // …and it still names the same file, which is what actually matters.
        assert!(reported.exists());
        assert_eq!(
            reported.canonicalize().unwrap(),
            precomposed.canonicalize().unwrap()
        );
    }

    /// FR-003: no ceiling. A selection two orders of magnitude past Explorer's
    /// legacy 15-item limit arrives whole.
    #[test]
    fn fr_003_a_large_selection_arrives_whole() {
        let urls: Vec<Retained<NSURL>> =
            (0..2_000).map(|i| url(&format!("/tmp/f{i}.pdf"))).collect();
        let objects: Vec<&AnyObject> = urls.iter().map(|u| &**u as &AnyObject).collect();

        let paths = paths_from_objects(&NSArray::from_slice(&objects));
        assert_eq!(paths.len(), 2_000);
        assert_eq!(paths[1_999], PathBuf::from("/tmp/f1999.pdf"));
    }

    /// Anything that is not a file URL has no path to print, so it is dropped
    /// rather than turned into a bogus entry in the queue.
    #[test]
    fn fr_005_non_file_items_are_dropped_not_guessed_at() {
        let file = url("/tmp/a.pdf");
        let remote = NSURL::URLWithString(&NSString::from_str("https://example.com/x.pdf"))
            .expect("a well-formed URL");
        let text = NSString::from_str("not a url at all");
        let objects: [&AnyObject; 3] = [&file, &remote, &text];

        assert_eq!(
            paths_from_objects(&NSArray::from_slice(&objects)),
            vec![PathBuf::from("/tmp/a.pdf")]
        );
    }

    /// The same path twice must not become two print jobs.
    #[test]
    fn fr_003_duplicates_collapse() {
        let a = url("/tmp/a.pdf");
        let again = url("/tmp/a.pdf");
        let b = url("/tmp/b.pdf");
        let objects: [&AnyObject; 3] = [&a, &again, &b];

        assert_eq!(
            paths_from_objects(&NSArray::from_slice(&objects)),
            vec![PathBuf::from("/tmp/a.pdf"), PathBuf::from("/tmp/b.pdf")]
        );
    }

    #[test]
    fn fr_001_an_empty_pasteboard_yields_nothing() {
        let objects: [&AnyObject; 0] = [];
        assert!(paths_from_objects(&NSArray::from_slice(&objects)).is_empty());
    }
}
