# ADR-0009: Rename the product from MaeclyPrint to Pluryprint

## Status

Accepted.

## Date

2026-10-09

## Context

The product shipped its pre-alpha builds as **MaeclyPrint**. The name did not describe the app. "Maecly" is the founder's name and the name of the publisher; it says nothing about what the app does, and "MaeclyPrint" read as "Maecly's printer" or a print shop rather than a tool that prints many files at once. It was also awkward to say and to spell, which hurts word of mouth and search.

The name had spread into every layer, in several spellings (`MaeclyPrint`, `maeclyprint`, `MAECLYPRINT_`, `Maecly Print`, `com.maecly.print`, `com.maecly.maeclyprint`). Some of those are only text. Others are the **runtime identity of installed copies**, and changing them moves or orphans state:

- the `ProjectDirs` application segment, which names the config, data, cache and log folders (`%APPDATA%\Maecly\MaeclyPrint\`, `~/Library/Application Support/com.Maecly.MaeclyPrint/`, `~/Library/Logs/MaeclyPrint/`);
- the log file prefix `maeclyprint-`, which retention matches;
- the bundle identifier, which names the webview's storage folder (and so its `localStorage`), the single-instance lock and the macOS bundle id;
- `productName`, from which Tauri's NSIS installer derives the install folder, the Uninstall key, `HKCU\Software\Maecly\MaeclyPrint` and the shortcuts;
- admin policy locations (`HKLM\SOFTWARE\Policies\Maecly\MaeclyPrint`, `ProgramData\MaeclyPrint\policy.json`, and the macOS and Linux equivalents);
- `MAECLYPRINT_*` environment variables that scripts and CI set;
- the Finder service ("Print with MaeclyPrint", its `NSPortName`) and the Explorer verb.

The bundle identifier was also inconsistent: `tauri.conf.json` said `com.maecly.print`, the data folders came out as `com.Maecly.MaeclyPrint`, and the docs used `com.maecly.maeclyprint`.

## Decision

We rename the product to **Pluryprint** ("plural print": many files, one action), with one capital P. The publisher stays **Maecly**: the app is "Pluryprint by Maecly".

- Display name `Pluryprint`; wordmark `Plury` + accent-coloured `print`; About reads "Pluryprint by Maecly". The site lives at `https://pluryprint.maecly.com`.
- Bundle identifier `com.maecly.pluryprint`, used everywhere the docs name one. `productName` and the window title `Pluryprint`.
- Cargo packages `pluryprint` (lib `pluryprint_lib`) and `pluryprint-shellext`; binaries `pluryprint` and `pluryprint-cli`; npm package `pluryprint`.
- `ProjectDirs::from("com", "Maecly", "Pluryprint")`; log prefix `pluryprint-`; temp prefixes `pluryprint-*`; `localStorage` keys `pluryprint:*`; exported profiles `<name>.pluryprint-profile.toml`.
- Environment variables `PLURYPRINT_*`.
- Finder service and Explorer verb "Print with Pluryprint" / "Imprimir con Pluryprint".
- The organisation name stays wherever it names the organisation: `ProjectDirs`' `Maecly` segment, `publisher`, `CN=Maecly`, the `Policies\Maecly\` registry path, the `maecly.com` domains, the `@maecly-maintainers` team.
- The repository keeps its URL, `github.com/MAECLY/maecly-print`, so existing clones, links and the updater endpoint compiled into installed copies keep working.
- The Windows shell extension keeps its CLSID (`B269B8DC-044F-4D65-8F1E-8F20A2E653EA`). No installer has ever registered it (the sparse MSIX package is not built yet), so there is no installed base that a new GUID would protect, and keeping it avoids a stale handler on developer machines. Only the Rust constant is renamed.
- History is not rewritten: `CHANGELOG.md` entries, ADR-0001 to ADR-0008 and the `claude design/maeclyprint/` hand-off archive keep the old name. So do the CUPS test queues (`Maecly_Hold`, `Maecly_Hold_Win`), which are real queues on the development machines.

**Upgrading an existing install.** We migrate instead of starting fresh:

- On launch, before logging starts, the GUI and the CLI move what is still in the MaeclyPrint folders into the Pluryprint ones (`src-tauri/src/util/legacy.rs`). It moves entry by entry, merges folders present on both sides, **never overwrites what the user has**, and logs each move and each entry it had to leave. A file that holds only first-run defaults is set aside as `<name>.first-run.bak` for the old one: a first launch deferred by a running MaeclyPrint writes such files before the migration can run. Old `maeclyprint-<date>.jsonl` logs are renamed to `pluryprint-`. A copy across volumes is written to a temporary and renamed into place, so a failed copy leaves nothing behind.
- The move runs under three locks: its own `migration.lock` (one process at a time; the GUI keeps it until it holds its instance lock), Pluryprint's `instance.lock` (a running Pluryprint defers it) and MaeclyPrint's `instance.lock`, held for the whole move. A lock that cannot be checked counts as held. `PLURYPRINT_SKIP_MIGRATION=1` turns the step off.
- A run without failures writes `legacy-migration.done`; later launches skip the step, so entries it had to leave are reported once.
- The new bundle identifier gives the packaged webview an empty storage, so `config.toml` now carries the first-run flags (`[app].locale_detected`, `[app].onboarding_complete`) that used to live there; an upgrade sets them. Theme and density were already in `config.toml`.
- Machine policies are read from the MaeclyPrint locations after every Pluryprint one (registry or file), with the same trust checks and a warning: a user process cannot move what an admin deployed.
- A future sparse MSIX package must first remove any package registered under the old identity (`MaeclyPrint`), which would claim the same CLSID. No such package was ever shipped, so this is a requirement on `installers/windows/sparse-msix/`, recorded in `shell-ext-win/README.md` and `shell-extensions.md`.
- `MAECLYPRINT_*` variables are read when the `PLURYPRINT_*` one is unset, with one warning per variable.
- `maeclyprint:*` `localStorage` keys move to `pluryprint:*` before the first paint (useful in `pnpm dev` and the browser preview; see Negative below for the packaged app).
- Profile files exported as `.maecly-profile.toml` still import: the importer reads any `.toml`.
- The NSIS installer's `NSIS_HOOK_PREINSTALL` (`src-tauri/windows/installer-hooks.nsh`) runs the per-user MaeclyPrint uninstaller silently and in place (`_?=` with the unquoted folder from `HKCU\Software\Maecly\MaeclyPrint`), which keeps user data. Only when it succeeds does the hook delete any registration left behind: the Uninstall key, `HKCU\Software\Maecly\MaeclyPrint`, the `*\shell\MaeclyPrint` and `Directory\shell\MaeclyPrint` verbs, and the shortcuts. A per-machine MaeclyPrint MSI (built before `c1466b4`) needs elevation to remove: the hook offers it, and `msiexec` asks for administrator approval; a silent install leaves it in place.

## Alternatives Considered

| Option | Why rejected |
| --- | --- |
| **Keep MaeclyPrint** | The name does not say what the app does, and the cost of a rename only grows after the first public release. |
| **Rename only the display name** | The user-visible name would disagree with the binary, the CLI, the folders, the logs and the Finder service, which users see in bug reports and scripts. |
| **Rename and start fresh (no migration)** | Users would lose their profiles, settings and queued jobs on upgrade, and old folders would be orphaned on disk. |
| **Keep the old bundle identifier** | `com.maecly.print` was already inconsistent with the folders and the docs, and keeping it would leave the old name in the macOS bundle id forever. |
| **Rename the repository too** | Breaks every clone, link and the updater endpoint in installed copies; GitHub's redirect is not something to rely on for the update channel. |
| **New CLSID for the shell extension** | Protects nothing (no installed registrations exist) and would need its own cleanup. |

## Consequences

### Positive

- The product name says what it does, and one spelling is used everywhere.
- The bundle identifier, the data folders and the docs finally agree.
- An existing install keeps its settings, profiles, queue and logs, and admin policies keep applying.
- `scripts/rename/` records the substitutions, so the rename can be replayed onto a moved base.

### Negative

- The new bundle identifier gives the packaged app a new webview storage folder, so what `localStorage` cached starts empty once. The settings and the first-run flags live in `config.toml`, which migrates (the upgrade sets the flags), so onboarding does not show again and a chosen language is kept; the theme and density follow the saved setting once the backend loads, after a first paint with the defaults.
- The single-instance lock is derived from the identifier, so an old MaeclyPrint and a new Pluryprint can run at the same time until the old one is uninstalled. The data migration waits while the old app runs.
- On macOS, an in-place update from MaeclyPrint swaps the new bundle into the old `/Applications/MaeclyPrint.app` path. The Finder Services cache keeps the old "Print with MaeclyPrint" entry until it refreshes (log out, or `/System/Library/CoreServices/pbs -update`).
- Old names stay in the code on purpose (`util/legacy.rs`, `settings/policy.rs`, `shell-ext-win/build.rs`, `src/lib/legacyStorage.ts`, `src-tauri/windows/installer-hooks.nsh`). They can be removed once no supported version can upgrade from a MaeclyPrint build.

### Risks / Open Questions

- The NSIS hook could only be reviewed, not run, on the macOS development machine. Verify it on the Windows VM with a MaeclyPrint build installed before the first Pluryprint release.
- If a user's new folder already has a file of the same name that holds more than first-run defaults (for example after using a Pluryprint pre-release), the MaeclyPrint copy is left in place and logged once, not merged. That is the safe choice, but the user may have to pick by hand.
