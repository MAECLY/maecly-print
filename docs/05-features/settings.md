# Settings

> **TL;DR** — The **Settings** screen: five grouped sections (Appearance, Printing, Engine, Updates, Privacy) over the persisted app configuration in `config.toml`. Appearance writes both an instant localStorage cache and the backend; the rest write straight to `config.toml` optimistically, rolling back on a validation failure. Engine limits are enforced for real and reach the running queue on save, with no restart. A `config.toml` that cannot be parsed is announced with a banner and every save is refused until it is fixed (FR-101).

This document is the source of truth for the **content and behavior** of the Settings screen (`src/screens/SettingsScreen.tsx`). The persisted schema lives in the [data model](../04-architecture/data-model.md#2-configtoml--global-settings) (`settings::Config`); the wire contract is the [IPC & CLI doc](../04-architecture/ipc-and-cli.md). Appearance (theme + density) has a separate instant-boot cache in `src/lib/theme.ts`.

The screen is the GUI counterpart to the `get_app_settings` / `set_app_settings` IPC commands, plus `set_default_profile` for the default-profile control. The **Language** row (FR-094) also lives here. It is the last primary navigation destination in the sidebar.

## Screen layout

A single scrolling column (max 760 px) of labelled sections, each a stack of label/hint + control rows.

```
┌─ Settings ───────────────────────────────────────────────────────────────────┐
│ APPEARANCE                                                                     │
│   Theme        Follows your OS by default.        [ Light | Dark | System ]    │
│   Density      Comfortable for reading…           [ Comfortable | Compact ]    │
│   Language     Applies immediately, across the whole app. [ English  ▾ ]    │
│   Start minimized  Opens minimized, from the next launch.    [ ———⬤  ]        │
│   Native notifications  …while Pluryprint is in the background. [ ⬤——— ]   │
│ PRINTING                                                                       │
│   Default profile   Applied when no profile is chosen.   [ Quick        ▾ ]    │
│   ⓘ The installer adds Pluryprint to the Explorer right-click menu: on        │
│     Windows 11 under Show more options (macOS: Finder › Services).             │
│ ENGINE                                                                         │
│   Concurrent conversions   Applies to the next conv…     [ 2      ] jobs       │
│   Conversion timeout       0 disables the timeout.       [ 60     ] seconds    │
│   Print timeout            0 disables it. Counts from…   [ 300    ] seconds    │
│   Max file size            Largest input accepted…       [ 200    ] MB         │
│ UPDATES                                                                        │
│   Channel      [ Stable | Beta | Nightly ]                                     │
│   ⓘ Stable receives final releases only. Beta also receives every pre-rel…    │
│   Check on startup   Asks GitHub … 30 seconds after launch…  [ ⬤———  ]         │
│   ⓘ The launch-time check only reads the number of the latest release…         │
│ PRIVACY                                                                        │
│   Crash reports            Off by default…               [ ———⬤  ]             │
│   Anonymous usage metrics  …never without consent.       [ ———⬤  ]             │
│   ⓘ No data is sent in this version — these switches record consent only.      │
└───────────────────────────────────────────────────────────────────────────────┘
```

When `config.toml` exists but cannot be parsed, a warning banner (`role="alert"`) sits above the first section, naming the file to repair or delete (see [Edge & empty states](#edge--empty-states)).

## Data model

`get_app_settings` / `set_app_settings` expose **four** sections of `config.toml` bundled as one `AppSettings`, plus a read-only, server-computed `meta` (`SettingsMeta`): `config_unreadable` (the file exists but could not be parsed, so the values are defaults), `config_path` (the absolute file path, for the banner) and `limits` (the engine `Bounds` the write path enforces, which the Engine fields validate against instead of a client-side copy). `meta` is ignored on write. The `[printers]` section is intentionally excluded — it is owned by the [Printers screen](./printers-management.md) (`get_printer_prefs` / `set_printer_prefs`) — and `schema_version` is server-managed. Both are preserved untouched across a settings write.

| Section       | Wire type      | Fields                                                                                   |
| ------------- | -------------- | ---------------------------------------------------------------------------------------- |
| `app`         | `AppCfg`       | `locale`, `theme` (`light\|dark\|system`), `launch_minimized`, `density` (`comfortable\|compact`), `native_notifications` (FR-072, default `true`) |
| `engine`      | `EngineCfg`    | `max_concurrent_conversions`, `conversion_timeout_seconds`, `print_timeout_seconds`, `max_file_size_mb`, `default_profile` |
| `updater`     | `UpdaterCfg`   | `channel` (`stable\|beta\|nightly`), `check_on_startup`                                   |
| `telemetry`   | `TelemetryCfg` | `crash_reports`, `anonymous_usage_metrics`                                                |

Optional fields that skip-serialize when unset (`density`, both telemetry flags) are normalized to concrete values by `normalizeAppSettings` (`src/lib/ipc.ts`) so the controls never bind to `undefined`. The `default_profile` field is driven by the Printing section but written through `set_default_profile`, not the settings bundle (see below).

## Sections

### Appearance — dual-write theme & density

Theme is a **3-way** control (Light / Dark / System) driven by `App`'s `useTheme` hook; density is Comfortable / Compact from `useDensity`. Appearance is special: it lives in **two** places and both are written on every change.

1. **Instant-boot cache** — `localStorage` keys `pluryprint:theme` / `pluryprint:density` (`src/lib/theme.ts`), applied to the DOM synchronously at boot — `bootAppearance()` runs in `main.tsx` before the first render, because the `--mp-*` tokens exist only under the theme class and React effects run after the first paint — so there is no theme flash before the backend loads. These hooks are DOM/localStorage-only and never call IPC.
2. **Backend source of truth** — the same value is persisted to `config.toml` via `patch({ app: { theme } })` / `patch({ app: { density } })` when the backend is reachable.

On divergence the **backend wins**: once `get_app_settings` resolves, a reconcile effect (in both `SettingsScreen` and `App`) overrides the cache with the persisted value. The reconcile is idempotent, so it settles without fighting a fresh user change. In the standalone preview (backend `unavailable`) appearance stays local-only — no backend write.

The section also carries the **Native notifications** switch (`app.native_notifications`, on by default; FR-072). It is read by the backend before every OS notification (`notify_native`), so turning it off takes effect immediately. With it on, a finished batch, a failed batch and a job stopped for the user are announced through the OS notification centre while the Pluryprint window is unfocused, hidden or minimized; the in-app toasts are unaffected either way.

### Appearance — language

**Language** (FR-094) is a select over the supported locales (`en-US` English, `es-ES` Español), persisted as `[app].locale` through `patch({ app: { locale } })`. `I18nProvider` reads it back, so the whole UI re-renders in the new language as soon as the optimistic write lands. The select is bound to the *effective* locale: a stored regional tag such as `es-MX` shows (and renders) Spanish. Saving also rewrites the context-menu label cache so the Explorer menu follows the language ([context-menu-actions.md](./context-menu-actions.md#localization)). Disabled while the backend is unavailable or loading.

### Appearance — start minimized

**Start minimized** persists `[app].launch_minimized`. It is honoured at startup: `lib.rs` reads the flag straight from `config.toml` in `setup` (before any IPC round-trip) and minimizes the main window to the taskbar / Dock. Pluryprint has no tray icon, so minimizing is the whole behaviour. A launch that carries a file selection (Explorer/Finder context menu) is never minimized, because it exists to show that selection (`settings::should_start_minimized`). A second launch un-minimizes and focuses the window through the single-instance callback. The change applies from the next launch.

### Printing — default profile

A dropdown of every profile from the profiles store; selecting one calls `profilesStore.setDefault` → `set_default_profile`, which writes `config.toml`'s `[engine].default_profile` (the single source of the default — see [profiles-management.md](./profiles-management.md#default-profile-resolution)). Because that command writes the config behind the settings store's back, the store syncs its cached `engine.default_profile` afterward so a later whole-section engine write can't silently revert the default (`settingsStore.syncDefaultProfile` also rewrites the confirmed baseline and any queued write). The row is disabled until both the profile list and the settings backend are loaded; when the list fails to load, a note with a **Retry** re-runs the profiles store's `init`. A muted, per-platform note follows: on macOS it says right-click printing works from **Finder › Services › Print with Pluryprint** (menu titles follow the system language); on Windows it says the Explorer entry is built but no installer registers it yet, so it does not appear ([FR-001](../03-requirements/functional-requirements.md)).

### Engine — real limits, applied without a restart

Four numeric fields with **local text state**: typing never persists; the value commits on blur or Enter, and only when it parses to an in-bounds whole number. An out-of-range entry shows an inline error and is **not** sent to the backend.

| Field                    | Bounds              | Enforcement                                                                 |
| ------------------------ | ------------------- | -------------------------------------------------------------------------- |
| Concurrent conversions   | `1..=8`             | Read by the dispatcher at app start and pushed to it again on every save (clamped `1..=8`). A raised cap starts waiting conversions at once; a lowered one never stops running work. |
| Conversion timeout       | `0`, or `10..=3600` | `0` disables it; otherwise a conversion that overruns fails with `conversion_timeout` (terminal, no auto-retry), whether the dispatcher's timer or the converter process's own bound ends it first. A new value applies to conversions that start after the save, including the bound on the LibreOffice, browser or Pandoc process itself. The CLI's `print` bounds its conversions with this value too, and its spool with the print timeout. |
| Print timeout            | `0`, or `10..=3600` (default `300`) | Its own budget, separate from the conversion timeout: it runs from handing the PDF to the spooler until the OS job leaves the queue (see [queue.md](./queue.md)). `0` disables it. When it runs out the job fails with `print_timeout` (terminal, no auto-retry). A `config.toml` written before this field existed loads with the default. |
| Max file size (MB)       | `1..=4096`          | Enforced live by `inspect_files` and by submission — an oversized file is reported unsupported / rejected. |

These are not cosmetic: `max_file_size_mb` gates the Print Job screen's file inspection and submission immediately, and the concurrency cap and both timeouts reach the running queue on save — `set_app_settings` sends the dispatcher a `SetLimits` command (`QueueHandle::apply_engine_settings`), so none of them needs a restart. The timeout uses the running attempt's cancel flag, guarded by an `Arc::ptr_eq` identity check so a late timer can never fail a *different* attempt.

### Updates

**Channel** (Stable / Beta / Nightly) picks the update manifest (FR-085, [auto-updater.md §6](../04-architecture/auto-updater.md#6-channels)): Stable reads `releases/latest/download/latest.json`; Beta reads the `channel-beta` release's manifest, which carries every pre-release and moves on to the stable release of the same version; Nightly follows Beta, since no nightly builds are published. The note under it says exactly that. Switching to a lower channel (towards Stable) opens a confirmation first (FR-086): nothing is downgraded, and the next update comes once that channel publishes a version newer than the installed one. Cancel keeps the channel; a move to a less stable channel applies at once. **Check on startup** (`[updater].check_on_startup`) toggles the launch-time check the backend runs **30 s after launch** (`STARTUP_CHECK_DELAY`), **at most once every 24 h** (`MIN_CHECK_INTERVAL_HOURS`; a manual check on the About screen also stamps the time). A newer release that is not the skipped version raises a toast (FR-080/FR-084). The note under it says the check only reads the latest release number, sends nothing about the user, and never installs anything on its own; installing is the About screen's **Install now** ([about.md](./about.md)). Turned off, no request is made at launch.

### Privacy — consent only

Two switches, **off by default**: crash reports and anonymous usage metrics. They record the user's *consent* and survive restarts, but **this version transmits nothing** regardless of their value — no crash reports and no usage metrics leave the machine. The note states this plainly.

## Write semantics

The settings store (`src/stores/settingsStore.ts`) is **optimistic with rollback**:

- `update(next)` applies `next` to local state immediately (controls feel instant), calls `set_app_settings`, then reconciles state to whatever the backend saved — so a server-normalized/clamped value is reflected without a second round-trip.
- On failure it **rolls back** to the last value the backend confirmed (the idle state when the burst of writes began, or the response of a save that succeeded since) and shows an error toast, leaving the UI consistent with disk.
- Writes are **serialized and coalesced**: at most one `set_app_settings` is in flight. Updates made meanwhile collapse into one pending bundle (each update carries the whole bundle, so the newest already contains the earlier changes), sent when the in-flight write settles. The last write to reach `config.toml` is therefore always the newest value, and only a write that leaves the queue empty reconciles the visible state.
- `patch(partial)` is the convenience path most controls use: it shallow-merges a per-section patch onto the current settings and persists.

On the backend, `set_app_settings` takes the config lock, loads the full config, **replaces exactly the four wire sections** (keeping `printers` and `schema_version`) — except `engine.default_profile`, which it **never writes**: the stored value is kept and the reply carries it, so a stale cache (for example after `pluryprint profiles set-default` / `delete` while the window was open) can neither revert the default nor make every later save fail on an id that no longer exists. It validates and writes atomically. Validation returns `InvalidRequest` naming the offending field, and nothing is written when it fails. Bounds: concurrency `1..=8`, timeout `0` or `10..=3600`, size `1..=4096`, non-empty locale.

> **This is replace-sections, not a deep merge.** `set_app_settings` overwrites each of the four sections wholesale with what the client sends; it is not the field-level `update_settings` deep-merge that earlier drafts of [ipc-and-cli.md](../04-architecture/ipc-and-cli.md) described. The client is expected to send a complete `AppSettings` (the store always does, via `patch` merging onto the loaded value).

## Edge & empty states

- **Standalone web preview** — status `unavailable`: only Appearance is adjustable (local cache only); a note explains the rest is read from the desktop app, and every backend-backed control is dimmed and non-interactive: the control cell is `inert` (out of the tab order and the accessibility tree, so the keyboard cannot change it either) and marked `aria-disabled`. The same applies to Theme and Density while the settings are loading.
- **Loading** — a `Loading settings…` note while `get_app_settings` is in flight.
- **Error** — a `Couldn't load settings.` note with a **Retry** that re-runs `init` (re-runnable so one transient failure doesn't kill the screen for the session).
- **Config file missing** — the backend uses `Config::default()` and the first save writes the file.
- **Config file corrupt** (FR-101) — reads degrade to `Config::default()` but report it (`meta.config_unreadable`). The screen shows a warning banner — *Your settings file could not be read* — saying the values on show are defaults, not the saved settings, that changes cannot be saved until the file is fixed or removed, and naming the file. Every mutator refuses meanwhile: `set_app_settings` (and `set_default_profile`, the printer preferences) load the file with `Config::load_strict` and return the config-corrupt error instead of overwriting the user's printer preferences with defaults, so a change rolls back with an error toast.

## Per-platform notes

`config.toml` is a plain per-user file at the same location on every platform (see [data model § 1](../04-architecture/data-model.md#1-file-locations)); nothing in this screen is OS-specific. The one platform-adjacent behavior is that engine limits govern the cross-platform convert/print pipeline identically.

## IPC mapping

| UI surface                | IPC command                                   | Notes                                                        |
| ------------------------- | --------------------------------------------- | ----------------------------------------------------------- |
| Load all sections         | `get_app_settings() => AppSettings`           | Four sections; optional fields normalized client-side.       |
| Any control (except default profile) | `set_app_settings(settings) => AppSettings` | Replace-sections + validate; returns the saved value. |
| Default profile           | `set_default_profile(id) => void`             | Writes `config.toml` `[engine].default_profile`.             |
| Default-profile dropdown options | `list_profiles() => Profile[]`         | Via the profiles store.                                      |

## Implementation status

- **M0** — appearance was local-only; there was no persisted settings backend (superseded).
- **M1 (current)** — **settings are real and persisted.** Rust: `src-tauri/src/settings/mod.rs` (`Config`/`AppSettings` schema, atomic I/O, `CONFIG_LOCK`) + `src-tauri/src/ipc/settings.rs` (get/set + validation). Engine limits are consumed for real (`src-tauri/src/queue/{mod,dispatcher}.rs`, `src-tauri/src/ipc/jobs.rs`). Frontend: `settingsStore.ts`, `SettingsScreen.tsx`, `src/lib/theme.ts`. Integration coverage: `src-tauri/tests/settings_persist.rs` (FR-100…FR-103 — human-readable TOML, corrupt-file fallback, atomic write, embedded `schema_version`).
- **Follow-ups**
  - The old **`[shell_extension]` switches were removed** for the MVP — shell-integration toggles land with **M2**, not here.
  - **Nightly builds**: the channel exists and follows beta until nightly builds are published. Installing an update is the About screen's job ([about.md](./about.md)).
  - **`[engine] html_renderer`** is deliberately not on this screen: `"browser"`, set by hand in `config.toml`, brings back the headless Edge/Chrome renderer for one version (ADR-0012), and a settings save keeps whatever the file says.
  - **Telemetry transport** — the consent switches are wired but nothing is sent; a future build can honour them.
  - **Hot-reload / env overrides** of `config.toml` (via `notify` / `PLURYPRINT_*`) are deferred; the file is read on demand under `CONFIG_LOCK`.

## Open questions for v1+

- Where should a future "reset to defaults" live — per section, or one global action?

These are tracked as design discussions, not commitments.
