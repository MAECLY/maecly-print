# Context Menu Actions

> **TL;DR** — UX design of the right-click submenu MaeclyPrint adds to Windows Explorer (and later macOS Finder): which items appear, when, what they do, and how the layout differs by selection type.

This document is the source of truth for the **content and behavior** of the context menu integration. Implementation lives in the [shell-extensions architecture doc](../04-architecture/shell-extensions.md); UX behavior lives here.

## Submenu structure

The MaeclyPrint integration adds a single top-level entry to the context menu, expanding into a submenu:

```
┌─ Right-click on selection ─────────────────┐
│  Open                                       │
│  Open with…                                 │
│  Share                                      │
│  Copy                                       │
│  ────────────────────────                   │
│  🖨  MaeclyPrint               ▸           │  ◀── our entry
│  ────────────────────────                   │
│  Properties                                 │
└─────────────────────────────────────────────┘
                   │
                   ▼
┌─ MaeclyPrint submenu ──────────────────────┐
│  ▶  Print with default profile              │
│     ────────────────────                    │
│  ◧  Print with profile…           ▸         │
│  ⚙  Configure print job…                    │
│     ────────────────────                    │
│  📋  Add to print queue                     │
│     ────────────────────                    │
│  🪟  Open MaeclyPrint                       │
└─────────────────────────────────────────────┘
```

The icon column above is illustrative; final iconography lives in the design assets folder.

## Items in detail

### Print with default profile

- **When shown:** always (when at least one supported file is in the selection).
- **Action:** silent print of all supported files in the selection using the user's default profile. No GUI shown. A toast appears on completion or failure.
- **Behavior on mixed selection:** unsupported files are skipped; toast lists them.
- **Behavior on folder selection:** if the folder contains supported files at the top level, those are printed. If the user wants recursive behavior, they should use **Configure print job…** which exposes the recursion checkbox.
- **Keyboard shortcut hint:** none in MVP. v1 may allow per-profile global hotkeys.

### Print with profile ▸

- **When shown:** always (if the user has more than one profile).
- **Behavior:** cascades into a submenu listing the user's profiles (built-in first, then custom alphabetical). Selecting one is equivalent to **Print with default profile** but with that profile.
- **Truncation:** if the user has more than 10 profiles, the menu shows 9 + "More…" which opens the GUI on the Profiles page with a "Choose for this job" mode.

### Configure print job…

- **When shown:** always (when at least one supported file is in the selection).
- **Action:** opens the GUI window on the **Print Job** page, with the selected files pre-loaded and the default profile's settings pre-filled.
- **User can:** review file list, remove individual files, change profile, override individual settings, then click **Print** or **Save and print**.
- **GUI start time:** must comply with [NFR-001](../03-requirements/non-functional-requirements.md#performance) (<500 ms p95).

### Add to print queue

- **When shown:** v0.3+ only. Hidden in MVP/v0.2.
- **Action:** enqueues the files using the default profile but does **not** start printing yet. The user can review the queue and start it manually.
- **Use case:** the user wants to compose a large batch from multiple folders before sending it.

### Open MaeclyPrint

- **When shown:** always; also in the background context menu (right-click on empty Explorer space).
- **Action:** opens the GUI to the **Home** page.
- **No selection required:** this is the only item available when right-clicking the Explorer background.

## Selection contexts

The submenu adapts to the type of selection.

### Single supported file

```
🖨  MaeclyPrint                    ▸
   ▶  Print with default profile
   ◧  Print with "Quick"            (auto-fills profile name)
   ⚙  Configure print job…
   🪟  Open MaeclyPrint
```

### Multiple supported files

```
🖨  MaeclyPrint                    ▸
   ▶  Print 23 files (default profile)
   ◧  Print 23 files with profile…  ▸
   ⚙  Configure print job…
   📋  Add 23 files to queue
   🪟  Open MaeclyPrint
```

The count is rendered into the label so users see exactly what they're about to do.

### Mixed selection (some unsupported)

```
🖨  MaeclyPrint                    ▸
   ▶  Print 18 of 23 files (5 unsupported)
   ⚙  Configure print job…
   ⚠  5 files cannot be printed (.exe, .zip)
   🪟  Open MaeclyPrint
```

The warning row is non-interactive (greyed out, info-only). The actionable items operate on the supported subset.

### Folder selection

```
🖨  MaeclyPrint                    ▸
   ▶  Print files in folder (top level)
   ▶  Print files in folder (recursive)
   ⚙  Configure print job…
   🪟  Open MaeclyPrint
```

If the folder is empty of supported files at the top level, the first row reads "No printable files" and is greyed out.

### Background (no selection)

```
🖨  MaeclyPrint                    ▸
   🪟  Open MaeclyPrint
```

Just the launcher.

## Filtering

The shell extension filters the selection before displaying counts:

- The list of supported extensions is read from the application's manifest at install time.
- Filtering is by extension only (not MIME or content sniffing) for performance — the menu must appear instantly.
- If filtering would result in an empty supported subset, the **Print** items are hidden entirely; only **Open MaeclyPrint** remains.

## Performance constraints

The shell extension code path runs synchronously in Explorer's UI thread. To stay imperceptible:

- The DLL's `EnumSubCommands` and `GetTitle` implementations must return in <50 ms total even for selections of 1000 items.
- Profile names are read from a cached file refreshed on profile changes; the cache lives in `%LOCALAPPDATA%\MaeclyPrint\submenu-cache.json`.
- The filter pass is `O(N)` over the selection but uses a hash set of supported extensions for `O(1)` lookup per file.
- No I/O beyond the cache file is permitted.

## Differences across menu environments

### Windows 11 modern menu (default)

- Submenu appears at the same level as Open / Share / Copy.
- Iconography rendered with system theme adaptation (light/dark).
- Achieved via the Sparse MSIX + IExplorerCommand approach. See [shell-extensions.md](../04-architecture/shell-extensions.md).

### Windows 11 classic menu (Show more options)

- The same submenu also appears here as a fallback registered via `HKCU\Software\Classes\*\shell\MaeclyPrint`.
- Identical content, slightly different icon rendering (standard 16×16 ICO vs SVG-equivalent).

### Windows 10

- Has only the classic menu; the registry-based registration handles it natively.
- Same submenu content.

### macOS Finder (M5+)

- Quick Action approach in M5: a Service in `~/Library/Services/` named "Print with MaeclyPrint" appears under the **Services** submenu of the right-click context menu. Single item only — no cascading.
- Finder Sync Extension approach (deferred to v1.1+): adds a proper submenu equivalent to Windows.
- macOS shortcuts conventionally appear under **Quick Actions** in modern Finder, which is acceptable but slightly less discoverable than Windows.

### Linux (potential v2)

- File-manager-specific extensions: Nautilus, Dolphin, Files. Each has a different mechanism. Out of scope until community demand.

## Visual design notes

- Icons follow the host OS style: PNG with transparency at 16/24/32/48 px on Windows; PDF or SVG on macOS.
- Submenu uses bold for the default action ("Print with default profile") to draw the eye.
- Disabled items are present (greyed) rather than hidden when they explain a state ("No printable files").
- Counts in labels use `n` for the number, never plural-vs-singular special cases (Spanish/German locale variants handled in i18n strings).

## Localization

Every label in the submenu is a string key in `i18n/{lang}.json`:

```json
{
  "context_menu.root_label": "MaeclyPrint",
  "context_menu.print_default": "Print with default profile",
  "context_menu.print_default_count": "Print {count} files (default profile)",
  "context_menu.print_with_profile": "Print with profile…",
  "context_menu.configure": "Configure print job…",
  "context_menu.add_to_queue": "Add to print queue",
  "context_menu.open_app": "Open MaeclyPrint",
  "context_menu.unsupported_warning": "{count} files cannot be printed ({extensions})",
  "context_menu.no_printable": "No printable files in selection"
}
```

Cached translations live in `%LOCALAPPDATA%\MaeclyPrint\submenu-cache-{lang}.json`. Language change in Settings refreshes the cache.

## Telemetry note

Context menu invocations are NOT counted, tracked, or reported anywhere. The shell extension touches only the cache file and spawns the main executable.

## Open questions for v1+

- Should we support a per-folder profile override? (Right-click a folder → "Always use profile X for this folder.")
- Should the cascade auto-collapse when there are very few profiles? (If only 2 profiles total, skip the submenu and inline both?)
- Should we offer a "Print and email" composite action for office workflows?

These are tracked as Discussions in the GitHub repo, not commitments.
