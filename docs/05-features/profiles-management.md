# Profiles Management

> **TL;DR** — The **Profiles** screen: a master-detail view of every print profile Pluryprint knows about — three immutable built-ins plus any number of user-created customs — with an inline editor for the customs. You can create, clone, rename, edit, delete (with a 5-second undo), export any profile to a TOML file and import profiles from one, and mark exactly one profile as the app default. Built-ins are read-only but cloneable. Customs live in `profiles.toml`; the default lives in `config.toml`.

This document is the source of truth for the **content and behavior** of the Profiles screen (`src/screens/ProfilesScreen.tsx` + `src/screens/profiles/ProfileEditor.tsx`). The persistence and validation live in the [data model](../04-architecture/data-model.md#3-profilestoml--print-profiles) (`profiles::*`) and the [IPC & CLI contract](../04-architecture/ipc-and-cli.md); the wire shapes are generated into `src/ipc/bindings.ts`.

The screen is the GUI counterpart to the `list_profiles` / `get_profile` / `save_profile` / `delete_profile` / `set_default_profile` / `export_profile` / `import_profiles` IPC commands, and the headless `pluryprint profiles` CLI drives the same cores (see [ipc-and-cli.md](../04-architecture/ipc-and-cli.md)). It is one of the primary navigation destinations, sitting between **Queue** and **Settings** in the sidebar. A profile is a reusable bundle of print settings the user picks in the [Print Job](./print-job.md) dialog; it is a *starting point*, and the per-invocation overrides on the Print Job screen win over it (FR-040/041).

## Screen layout

A master-detail split, mirroring the Printers screen: a fixed list of profiles on the left, the selected profile's detail (or its editor) on the right.

```
┌─ Profiles ─────────  Saved combinations of printer + settings   [+ New profile] ┐
├──────────────────┬───────────────────────────────────────────────────────────────┤
│ 🔒 Quick    [def]│  Quick                       [built-in] [default]              │
│   color·simplex·A4│  id: builtin-quick                            [Clone] [★ Make …]│
│ 🔒 Duplex BW     │  ┌─────────────────────────────────────────────────────────────┐│
│   mono·long-edge·A4  Printer      <default>                                        ││
│ 🔒 Photo Color   │  Color mode   color                                            ││
│   color·simplex·A4│  Duplex       Single-sided                                     ││
│ ▤ Press A        │  Copies       1                                                ││
│   mono·long-edge·A4  Paper        A4                                              ││
│ ▤ Labels         │  …                                                             ││
│                  │  (custom profiles show the editor here instead)                ││
└──────────────────┴───────────────────────────────────────────────────────────────┘
```

- **Top bar** — title `Profiles`, subtitle `Saved combinations of printer + settings`, and two actions: **Import** and **New profile** (both enabled only when the backend list is `ready`).
- **Master list (280 px)** — one row per profile: a lock glyph for built-ins (a sliders glyph for customs), the name, a `default` badge on the current default, and a `{color} · {simplex|duplex} · {paper}` sub-line. Built-ins always come first, in the canonical order **Quick → Duplex BW → Photo Color**, then the customs sorted case-insensitively by name.
- **Detail pane** — a header (name, `built-in` / `unsaved` / `default` badges, id, and the action buttons) followed by either a **read-only field table** (a selected built-in) or the **inline editor** (a custom, a new draft, or a clone).

## Profile data model

Each profile is the wire [`Profile`] shape, translated to the display-cased `UiProfile` by `toUiProfile` (`src/lib/ipc.ts`). Two flags — `builtin` and `default` — are **server-computed**: whatever the client sends is ignored and the value returned always reflects the current truth.

| Field         | Type / wire enum                         | Example         | Notes                                                       |
| ------------- | ---------------------------------------- | --------------- | ---------------------------------------------------------- |
| `id`          | string                                   | `builtin-quick` | Built-ins: fixed slugs. Customs: a UUID v7, stable for life. |
| `name`        | string                                   | `Press A`       | 1–100 chars, unique case-insensitively across all profiles. |
| `builtin`     | boolean (**computed**)                   | `true`          | True for the three built-ins; ignored on input.             |
| `default`     | boolean (**computed**)                   | `false`         | True for the current app default; set via `set_default_profile`. |
| `printer`     | string                                   | `<default>`     | A printer id, or the `<default>` sentinel to defer to the app/OS default. |
| `color_mode`  | `color \| monochrome`                    | `color`         | UI vocabulary `color`.                                       |
| `duplex`      | `off \| long-edge \| short-edge`         | `long-edge`     | Profile vocabulary (distinct from the engine's `none/long/short`). |
| `copies`      | number `1..=999`                         | `1`             | Validated client-side and server-side.                      |
| `paper_size`  | string                                   | `a4`            | The classic four travel as `a4`/`letter`/`legal`/`a3` and are re-cased to `A4`/`Letter`/… for display. Any other printer paper name (`A6`, `4x6`) or `auto` is stored verbatim (`PaperSize` in `queue/job.rs`); the CLI (`profiles create --paper`) and an import can set one, the editor offers only the classic four. |
| `orientation` | `auto \| portrait \| landscape`          | `auto`          |                                                             |
| `scaling`     | `fit \| shrink \| none`                  | `fit`           | `none` = actual size (100 %).                               |
| `page_range`  | string \| absent                         | `1-5,10`        | Engine grammar; **absent means all pages** (no `"all"` sentinel). |
| `quality`     | `draft \| normal \| high`                | `normal`        |                                                             |
| `tray`        | string                                   | `auto`          | Free-text tray name, or `auto`.                             |
| `collate`     | boolean                                  | `true`          | Whole sets (1-2-3 1-2-3) vs. grouped pages. Defaults to `true`, so a profile saved before the field existed loads. Not in the editor; set by the CLI (`--collate` / `--no-collate`) and kept by every save. |

### The three built-ins (FR-021)

Hardcoded in `profiles::builtins()` (Rust) and mirrored in `src/lib/profiles.ts` for the offline fallback. They cannot be edited or deleted, only **cloned** into a fresh custom.

| id                    | Name          | Color        | Duplex      | Scaling            | Quality |
| --------------------- | ------------- | ------------ | ----------- | ------------------ | ------- |
| `builtin-quick`       | `Quick`       | color        | Single      | Fit                | Normal  |
| `builtin-duplex-bw`   | `Duplex BW`   | monochrome   | Long edge   | Fit                | Normal  |
| `builtin-photo-color` | `Photo Color` | color        | Single      | Actual size (100%) | High    |

All three share `printer = <default>`, `copies = 1`, `paper = A4`, `orientation = auto`, no page range, `tray = auto`. `builtin-quick` is also the **hardwired fallback default** when no valid `engine.default_profile` is configured.

## Editing

Built-ins render a read-only field table with a **Clone** button. Customs (and drafts) render the `ProfileEditor`, a controlled form over a working copy:

- **Name** — validated live (`validateProfileName`): non-empty when trimmed, ≤ 100 characters counted as Unicode code points (the backend's `chars().count()`, so an emoji counts once), and unique case-insensitively against every other profile (the edited profile excludes its own name, so a plain re-save keeps working).
- **Printer** — a dropdown of `System default` (the `<default>` sentinel) plus the installed printers from `list_printers`, minus those disabled in Pluryprint (`get_printer_prefs`), exactly as on Print Job. Each option is labelled with the printer's display name but its value is the printer **id** (the queue name the spooler needs; on CUPS the display name is `printer-info` and differs from it). A profile's own saved printer is always included even if it is currently offline or unlisted. A profile saved by an older build that holds a display name is shown as the matching printer and stored as its id on the next save (`resolvePrinterId`).
- **Color, Duplex, Orientation, Scaling, Quality, Paper** — selects over the fixed vocabularies above.
- **Copies** — buffered as text so an out-of-range keystroke shows an inline error (`Enter a whole number from 1 to 999.`) instead of being silently clamped; only a valid value is submitted.
- **Tray** — free text, defaulting to `auto`. A note under the grid says tray and quality are sent with every job: the tray must match the driver's bin name (on Windows it is sent as that bin's id), and on Windows Draft and High are applied by making them the printer's default for the user's account only while the job is sent; a quality the driver lacks is named in the print preview. See [print-settings.md](../03-requirements/print-settings.md).
- **Page range** — validated against a client mirror of the engine grammar (`isValidPageRange`); `Applies to PDFs and documents — leave blank for all pages.` A descending range is rejected inline.

**Save** is gated by a dirty check and validity: an unchanged or invalid form disables it. A brand-new draft is always considered dirty. A server-side rejection surfaces as a toast and leaves the editor open with its state intact so the user can correct and retry.

## Actions

| Action           | Where           | Behavior                                                                                       |
| ---------------- | --------------- | ---------------------------------------------------------------------------------------------- |
| **New profile**  | top bar         | Opens an unsaved draft (id `''`) seeded from `Quick` with a unique `New profile` name.          |
| **Clone**        | built-in detail | Opens a draft copying the built-in's settings, named `Copy of {name}` (de-duplicated, shortened to fit 100 characters). |
| **Save changes / Create profile** | editor | Upserts via `save_profile`; toasts `Saved "{name}"`, reloads the list, selects the saved profile. |
| **Cancel**       | editor          | Discards the draft / edits (resets the working copy to the stored values).                      |
| **Make default** | custom/built-in detail | Marks the profile the app default via `set_default_profile`; disabled when it already is. |
| **Delete**       | custom detail   | Confirmation dialog, then `delete_profile`; shows a **5-second Undo** toast (NFR-032).           |
| **Export**       | any profile's detail | Save dialog suggesting `<name>.pluryprint-profile.toml`, then `export_profile`; toasts `Exported "{name}"`. Disabled while the editor holds unsaved changes. |
| **Import**       | top bar         | Open dialog for one `.toml` file, then `import_profiles`; toasts `Imported N profiles` and selects the first one imported. Asks before discarding unsaved edits. |

**Delete + Undo (NFR-032).** Delete asks for confirmation, then removes the profile and offers a 5-second `Undo` toast. Undo re-saves the exact snapshot: because `save_profile` preserves an unknown non-empty id (see [upsert semantics](#ipc-mapping)), the restored profile comes back with **the same id**, so any reference to it survives the round-trip. Deleting the profile currently selected in the master list moves the selection to the app default.

## Import & export (FR-024)

- **Format.** An export is the same versioned document as `profiles.toml` (`schema_version` plus one `[[profile]]` table), holding the profile's settings only: the computed `builtin` / `default` flags are not part of the on-disk shape, so a file can never carry "I am the default" onto another machine. Built-ins can be exported too. The write is atomic (FR-026). Because the shapes match, several exports concatenated by hand import in one step.
- **Import is untrusted input.** The file is capped at **1 MiB** (`MAX_IMPORT_BYTES`), parsed and checked *before* `profiles.toml` is opened: a newer `schema_version` is refused, an empty document is refused, and **one invalid entry rejects the whole document**, so a rejected import never leaves a partial merge.
- **Collision policy — additive only.** Every imported profile gets a **fresh UUID v7** (ids from another machine mean nothing here, and a `builtin-*` id could otherwise squat a reserved slot), so an import can never overwrite a custom or touch a built-in. A name that collides case-insensitively — with a built-in, an existing custom or an earlier entry of the same file — is renamed `"{name} 2"`, `"{name} 3"`…, the same scheme as Clone, shortening the base so the result still fits 100 characters. Importing the same file twice yields two profiles.
- Import, like save, delete and set-default, refreshes the context-menu cache (`shell-menu.json`) so the new profiles are offered at once. Import/export is not available from the CLI.

## Persistence & edge states

- **Customs only on disk.** `profiles.toml` (in the config dir) holds *only* the user-created profiles as an array of `[[profile]]` tables plus a `schema_version`. The three built-ins live in code, and the default lives in `config.toml` — neither is written to `profiles.toml`.
- **Atomic writes (FR-026).** Every mutation writes a tempfile in the same directory and renames it into place, so an interrupted save never leaves a partial file.
- **Corrupt file.** Read commands (`list_profiles` / `get_profile`) degrade to **built-ins only** with a warning rather than failing. Mutating commands **refuse** to touch an unparseable file (returning an internal error) so hand-broken user data is never clobbered.
- **Duplicate names on load.** A hand-edited file with two case-insensitively equal names keeps the first and drops the rest with a warning.
- **Unlimited profiles (FR-027).** There is no cap; the integration suite exercises 500.
- **Deleted default.** The effective default resolves to the configured `engine.default_profile` when it still names a known profile, otherwise falls back to `builtin-quick` — a default deleted out from under the config never dangles. (Deleting the *current* default is refused outright; the user must switch first.)
- **Changed or deleted on disk.** Opening a saved custom re-reads it through `get_profile`. If another window, a hand edit or the CLI changed it, the row is refreshed and a clean editor is remounted on the fresh values, so Save cannot write stale values back; an editor holding real edits keeps them and warns that saving will replace the newer version. If the profile was deleted (`ProfileNotFound`), the list is reloaded, a toast says so, and the selection moves to the default, so a later Save cannot quietly re-create it.
- **Load failure.** When `list_profiles` fails the screen shows the error with a **Retry** button that re-runs the store's `init`.
- **Export with unsaved edits.** `export_profile` writes the stored version, so **Export** is disabled (with a tooltip saying why) while the editor holds unsaved changes. A file dialog that cannot open shows an error toast instead of doing nothing.
- **Standalone web preview.** Outside the Tauri host the store falls back to the static built-ins and marks itself `unavailable`: the detail pane shows a lock note, editing is disabled, and the Print Job profile dropdown still works.

## Default profile resolution

Exactly one profile is the app default (FR-022). The single source of truth is `config.toml`'s `[engine].default_profile` (a profile **id**, not a name). `set_default_profile` validates the id names a known profile and writes it under the config lock. `list_profiles` computes each row's `default` flag from it. When a job runs without an explicit profile choice, App resolves the effective profile as **explicit choice ?? app default ?? Quick**, and a Print Job selection that points at a since-deleted custom falls back to the default once the list is authoritative (`App.tsx`). A **context-menu** selection is different: it names a profile the user picked in Explorer or Finder, so App re-reads `profiles.toml` before resolving it (the CLI may have created it after the window loaded its list) and, when nothing matches by id or name, prints and queues nothing — an error toast names the profile and the files open on the Print Job screen ([context-menu-actions.md](./context-menu-actions.md#how-the-app-handles-each-action-implementation)).

## Per-platform notes

Profiles are OS-neutral: `profiles.toml` and the default id are plain config files at the same per-user location on every platform (see [data model § 1](../04-architecture/data-model.md#1-file-locations)). Only two things touch the OS: the **Printer** dropdown lists whatever `list_printers` returns for the host, and the `<default>` sentinel resolves at dispatch time to the app default printer, then the OS default (see [printers-management.md](./printers-management.md)).

## IPC mapping

| UI surface                     | IPC command                                   | Notes                                                                 |
| ------------------------------ | --------------------------------------------- | --------------------------------------------------------------------- |
| Master list                    | `list_profiles() => Profile[]`                | Built-ins first (canonical order), then customs sorted by name; `default` computed from `engine.default_profile`. |
| (detail / deep link)           | `get_profile(id) => Profile`                  | Built-in or custom, with computed flags; `ProfileNotFound` for an unknown id. |
| Save / Create / Clone / Undo   | `save_profile(p) => Profile`                  | **Upsert**: empty id → create (fresh UUID v7); unknown non-empty id → create *preserving that id* (undo); known custom id → update; built-in id → rejected. Returns the saved profile. |
| Delete                         | `delete_profile(id) => void`                  | Rejects a built-in and the current default; `ProfileNotFound` for an unknown id. |
| Make default                   | `set_default_profile(id) => void`             | Writes `config.toml` `[engine].default_profile`; id must name a known profile. |
| Export                         | `export_profile(id, path) => void`            | Writes the stored profile (built-in or custom) as a one-profile TOML document; `ProfileNotFound` for an unknown id. |
| Import                         | `import_profiles(path) => Profile[]`          | Adds every profile in the document with fresh ids and de-duplicated names; returns them. Size cap, schema check and all-or-nothing validation as above. |

The store follows a **reload-after-write** model: each mutation calls its command, then re-fetches the whole list, so the server's computed flags are always authoritative and the UI never drifts on a mis-guessed optimistic update. See [ipc-and-cli.md § profiles](../04-architecture/ipc-and-cli.md).

## Implementation status

- **M0** — front-end only; profiles were local presets in `src/lib/profiles.ts` (superseded).
- **M1 (current)** — **profiles are real, persisted, and end-to-end.** Rust: `src-tauri/src/profiles/mod.rs` (built-ins, validation, upsert/delete, atomic I/O) + `src-tauri/src/ipc/profiles.rs` (the seven commands, locking, corrupt-file policy). Frontend: `profilesStore.ts`, `ProfilesScreen.tsx`, `ProfileEditor.tsx`, and the `toUiProfile` / `toWireProfile` adapters. Integration coverage lives in `src-tauri/tests/profile_roundtrip.rs` (FR-020/021/022/026/027). The `src/lib/profiles.ts` built-ins remain only as the offline fallback and mirror the Rust `builtins()` exactly.
- **Follow-ups**
  - **Profile-specific keyboard shortcuts** from the GUI (FR-025).
  - **Hot-reload / env overrides** — `notify`-based reloading and `PLURYPRINT_*` env overrides of the config files are deferred. The backend reads the files on demand under a mutex, so it never holds a stale copy; the window's list is re-read after each of its own writes, when an editor opens on a saved custom, and before a context-menu selection is resolved, but a change made elsewhere (the CLI, a hand edit) is not pushed to an open Profiles screen.
  - **Non-classic paper sizes in the editor** — a profile can store any printer paper name (CLI, import), but the editor's Paper select offers only A4 / Letter / Legal / A3.
- **Done since M1** — import/export (FR-024, above); the context-menu label cache `shell-menu.json` (`src-tauri/src/shell_menu.rs`), rewritten by every profile mutation from the GUI or the CLI ([context-menu-actions.md](./context-menu-actions.md#localization)).

## Open questions for v1+

- Should a profile carry a per-format converter override (e.g. force Pandoc for `.docx`), landing alongside the M3 multi-format work?
- Should "Make default" be reachable from the Print Job profile dropdown, not just this screen?

These are tracked as design discussions, not commitments.
