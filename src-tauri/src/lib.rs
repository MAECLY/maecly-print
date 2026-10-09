//! MaeclyPrint Tauri backend.
//!
//! Boots the Tauri window and exposes the backend over typed (tauri-specta)
//! IPC: the print engine (`engine`), the persistent job queue (`queue`),
//! profile and settings persistence (`profiles`, `settings`), OS printer
//! enumeration and management (`platform`), and the GitHub-release update
//! check (`update`).
//!
//! The same backend also has a second, windowless face: the `cli` module. It
//! decides from argv whether this process is a GUI launch at all, so a
//! `maeclyprint print …` never reaches [`run_with`] and never creates a webview.

pub mod cli;
pub mod engine;
pub mod ipc;
pub mod logging;
pub mod platform;
pub mod profiles;
pub mod queue;
pub mod settings;
pub mod update;
pub mod util;

use tauri::Manager;

/// Boot the GUI with nothing pre-loaded. Kept as the zero-argument entry point
/// the mobile target expects; desktop `main` calls [`run_with`].
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    run_with(None, None);
}

/// Boot the GUI, optionally pre-loading a file selection handed to us on the
/// command line (a shell-extension launch — see [`cli`]).
///
/// `handoff` is the file that selection was read out of, when there was one. It
/// is still on disk: reading it does not delete it, because this process may
/// yet be intercepted by the single-instance plugin and forwarded to a running
/// instance that has to read the very same file. Whichever process ends up
/// parking the selection is the one that deletes it — here, in `setup`
/// ([`cli::pending::park_launch_selection`]); there, in the plugin callback
/// below.
///
/// The single-instance plugin is registered **first**, as its documentation
/// requires: a second launch must be intercepted before this process starts
/// recovering `queue.json` or sweeping the conversion cache, which is exactly
/// the work that would destroy the running instance's in-flight jobs
/// (NFR-014).
pub fn run_with(selection: Option<cli::Selection>, handoff: Option<std::path::PathBuf>) {
    // Initialize structured logging as early as possible so any startup error
    // is captured to disk.
    if let Err(e) = logging::init() {
        eprintln!("logging init failed: {e}");
    }
    // Release builds abort on panic and the Windows GUI has no console: the
    // hook is the only way a crash leaves a trace (NFR-033). Installed right
    // after the log directory exists, before anything that can panic.
    logging::install_panic_hook();

    // Drop handoff files a previous run never consumed (crash between "the
    // shell extension wrote it" and "we read it").
    cli::handoff::sweep(
        &cli::handoff::handoff_dir(),
        cli::handoff::ORPHAN_HANDOFF_AGE,
    );

    let builder = specta_builder();

    // Regenerate the typed bindings on every debug launch as a convenience;
    // `cargo test` does the same headlessly (see `export_bindings`), which is
    // what CI and the build pipeline rely on.
    #[cfg(debug_assertions)]
    export_bindings(&builder);

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            // A second launch already exited by the time this runs; all we get
            // is its argv. Interpreting it is a pure function of the argv and
            // the handoff file it names (see `cli::single_instance`), so the
            // only work here is delivery.
            if let Some(forwarded) = cli::single_instance::interpret(&argv) {
                use tauri_specta::Event;
                let cli::single_instance::Forwarded { selection, handoff } = forwarded;
                // `try_state`, not `state`: a launch racing our own `setup`
                // would otherwise panic on the not-yet-managed store.
                match app.try_state::<cli::pending::PendingSelections>() {
                    Some(pending) => {
                        pending.push(selection.clone());
                        // The selection lives in this process now, so the file
                        // it travelled in has done its job. Deleting it *here*
                        // rather than in the launching process is what lets the
                        // same payload be read twice (NFR-014).
                        cli::handoff::discard(&handoff);
                    }
                    None => tracing::warn!(
                        event = "selection_dropped_before_setup",
                        "a forwarded selection arrived before the app finished starting"
                    ),
                }
                if let Err(e) = cli::pending::SelectionForwarded(selection).emit(app) {
                    tracing::warn!(event = "selection_forwarded_emit_failed", error = %e);
                }
            }
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        // FR-072: native notifications. Driven Rust-side only
        // (`ipc::notify::notify_native`), so no JS capability is granted.
        .plugin(tauri_plugin_notification::init())
        .invoke_handler(builder.invoke_handler())
        .setup(move |app| {
            builder.mount_events(app);

            // Anything the command line handed us waits here until the window
            // has mounted and drains it (`take_pending_selections`); parking it
            // is also what retires the handoff file it arrived in.
            app.manage(cli::pending::park_launch_selection(selection, handoff));

            // FR-001: the Finder's Services menu ("Print with MaeclyPrint")
            // reaches the app through this provider, and reaches it in-process
            // — so it must be registered *after* the store above exists, or a
            // service message arriving immediately would have nowhere to land.
            // A registration failure is logged and survived: the app itself is
            // perfectly usable without the menu entry, and refusing to start
            // would be a far worse answer.
            #[cfg(target_os = "macos")]
            if let Err(e) = platform::macos::services::register(app.handle()) {
                tracing::warn!(event = "macos_services_register_failed", error = %e);
            }

            tracing::info!(
                event = "app_started",
                version = env!("CARGO_PKG_VERSION"),
                build_date = env!("BUILD_DATE"),
                "MaeclyPrint started"
            );

            // Boot the print queue: recover persisted state and start the
            // dispatcher, then expose the handle to the IPC commands.
            let queue = queue::spawn(app.handle().clone())
                .map_err(|e| format!("failed to start print queue: {e}"))?;
            app.manage(queue);

            // FR-080: the launch-time update check. It lives here, in `setup`,
            // rather than in a frontend effect, for three reasons: the
            // single-instance plugin above guarantees `setup` runs exactly once
            // per process, so a second launch cannot double-check; the webview
            // can be reloaded or its window closed and reopened, neither of
            // which should re-run a network check; and the preference, the
            // 24h throttle and the skipped-version rule already live in Rust
            // next to the state file, so the frontend would only be able to
            // re-derive them. Everything runs on a detached thread — the sleep
            // included — so a cold start never waits on the network, and a user
            // who quits before the delay elapses simply takes the thread down
            // with the process, having persisted nothing.
            let update_handle = app.handle().clone();
            std::thread::spawn(move || {
                std::thread::sleep(update::STARTUP_CHECK_DELAY);
                // `None` is every uninteresting outcome (disabled, throttled,
                // offline, up to date, skipped version) — see
                // `update::run_startup_check`.
                let Some(check) = update::run_startup_check() else {
                    return;
                };
                use tauri_specta::Event;
                tracing::info!(event = "update_available", latest = ?check.latest);
                if let Err(e) = ipc::update::UpdateAvailable(check).emit(&update_handle) {
                    tracing::warn!(event = "update_available_emit_failed", error = %e);
                }
            });

            // Resolve the window so failures here surface as setup errors,
            // not silent missing-window bugs.
            if let Some(_main) = app.get_webview_window("main") {
                tracing::debug!(event = "main_window_resolved");
            } else {
                tracing::warn!(event = "main_window_missing");
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

/// The single source of truth for the IPC command surface. Used by both the
/// runtime (`run`) and the binding-export test so they never drift.
fn specta_builder() -> tauri_specta::Builder<tauri::Wry> {
    tauri_specta::Builder::<tauri::Wry>::new()
        .commands(tauri_specta::collect_commands![
            ipc::diag::app_info,
            ipc::diag::open_logs_folder,
            ipc::diag::report_issue,
            ipc::diag::open_external,
            ipc::diag::log_frontend_error,
            ipc::notify::notify_native,
            ipc::printers::list_printers,
            ipc::printers::default_printer,
            ipc::printers::discover_printers,
            ipc::printers::get_printer_prefs,
            ipc::printers::set_printer_prefs,
            ipc::printers::set_default_printer,
            ipc::printers::print_test_page,
            ipc::printers::open_printer_settings,
            ipc::printers::add_printer,
            ipc::printers::get_printer_policy,
            ipc::jobs::print_files,
            ipc::jobs::queue_status,
            ipc::jobs::cancel_job,
            ipc::jobs::cancel_all,
            ipc::jobs::pause_job,
            ipc::jobs::resume_job,
            ipc::jobs::retry_job,
            ipc::jobs::pause_queue,
            ipc::jobs::resume_queue,
            ipc::jobs::clear_done,
            ipc::jobs::inspect_files,
            ipc::jobs::expand_folders,
            ipc::jobs::preview_command,
            ipc::profiles::list_profiles,
            ipc::profiles::get_profile,
            ipc::profiles::save_profile,
            ipc::profiles::delete_profile,
            ipc::profiles::set_default_profile,
            ipc::profiles::export_profile,
            ipc::profiles::import_profiles,
            ipc::settings::get_app_settings,
            ipc::settings::set_app_settings,
            ipc::update::check_for_updates,
            ipc::update::get_update_state,
            ipc::update::set_skipped_update_version,
            cli::pending::take_pending_selections,
        ])
        .events(tauri_specta::collect_events![
            ipc::events::QueueUpdated,
            ipc::events::JobProgressEvent,
            ipc::update::UpdateAvailable,
            cli::pending::SelectionForwarded
        ])
}

/// Export the TypeScript bindings to `src/ipc/bindings.ts`.
///
/// `@ts-nocheck` disables strict TS checks (notably `noUnusedLocals`) so
/// tauri-specta scaffolding like `__makeEvents__` / `TAURI_CHANNEL` doesn't
/// break CI typecheck. The file is generated, never hand-edited.
///
/// Compiled for tests as well as debug builds so `bindings_are_current` keeps
/// working under `cargo test --release`.
#[cfg(any(debug_assertions, test))]
fn export_bindings(builder: &tauri_specta::Builder<tauri::Wry>) {
    builder
        .export(
            specta_typescript::Typescript::default()
                // `u64` fields (file sizes/byte counts) map to a plain TS
                // `number`. Real print inputs are far below 2^53, so precision is
                // not a concern and the frontend avoids `bigint` ergonomics.
                .bigint(specta_typescript::BigIntExportBehavior::Number)
                .header(
                    "// @ts-nocheck\n/* eslint-disable */\n// AUTO-GENERATED by tauri-specta. Do not edit by hand.\n",
                ),
            "../src/ipc/bindings.ts",
        )
        .expect("failed to export typescript bindings");
}

/// Keeps `src/ipc/bindings.ts` in sync with the Rust command surface. Running
/// `cargo test` regenerates it headlessly (no GUI), which is how CI and the
/// build refresh the typed frontend bindings.
#[cfg(test)]
mod bindings_sync {
    #[test]
    fn bindings_are_current() {
        super::export_bindings(&super::specta_builder());
    }
}
