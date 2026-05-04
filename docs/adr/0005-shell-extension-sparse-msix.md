# ADR-0005: Shell extension — Sparse MSIX + IExplorerCommand

## Status

Accepted

## Date

2026-05-03

## Context

The product UX of MaeclyPrint is "right-click N files in Explorer, pick **Print all with MaeclyPrint**, see a dialog". Two things make this harder than it sounds on Windows 11:

1. The default Win32 print verb registered against `HKCR\*\shell\Print` is **disabled by Explorer when more than ~15 files are selected**. This is a long-standing behavior intended to protect users from accidentally printing huge selections. It is also exactly what we want to overcome.
2. **Windows 11's modern context menu** does not include registry-only entries by default — they are demoted to "Show more options". Users will not discover the entry there.

To get a top-level entry in the modern menu *and* receive the full file list (no 15-file ceiling), we need to register a real shell extension implementing `IExplorerCommand`. Furthermore, Windows requires that the COM server be associated with a **packaged identity** to appear in the modern menu — a regular per-user COM registration in `HKCU\Software\Classes\CLSID\...` is not sufficient.

The packaged-identity options are:

- A **full MSIX package** for the entire app (heavyweight; changes how we distribute and install).
- A **Sparse MSIX package** that gives our externally installed app an MSIX-style identity without requiring MSIX installation semantics.

## Decision

We will ship MaeclyPrint with a **C++17 COM DLL implementing `IExplorerCommand`**, registered via a **Sparse MSIX package** that grants packaged identity to our externally installed (Inno Setup) app.

Specifically:

- The DLL (`MaeclyPrintShellExt.dll`) is a thin in-process COM server, ~500-1000 LOC, that:
  1. Receives `IShellItemArray` from Explorer.
  2. Resolves each item to an absolute file system path.
  3. Serializes the list as JSON.
  4. Spawns `MaeclyPrint.exe --invoke=context-menu --files=<json>` (or `--files-from=<temp>` for large selections).
  5. Returns immediately.
- The Sparse MSIX manifest declares the COM server (`<com:Extension Category="windows.comServer">`) and the menu handler (`<desktop4:Extension Category="windows.fileExplorerContextMenuHandler">`) for `Type="*"` (all files).
- Install-time PowerShell (`Add-AppxPackage -Path AppxManifest.xml -ExternalLocation <install-dir> -Register`) registers the sparse package, granting MaeclyPrint packaged identity.
- For Windows 10 (no modern menu) and as a backstop, we additionally register a simple `HKCU\Software\Classes\*\shell\MaeclyPrint` registry entry that uses `%1` substitution and routes via named pipe for selections > 1.

## Alternatives Considered

| Option                                  | Why rejected                                                                                                |
| --------------------------------------- | ----------------------------------------------------------------------------------------------------------- |
| **Registry-only handler** (`HKCU\Software\Classes\*\shell\MaeclyPrint`) | Works on Win 10 perfectly, but on Win 11 the entry is demoted to "Show more options" — invisible to most users. Acceptable as a *fallback*, not as the primary path. |
| **Full MSIX package** (Inno Setup replaced by MSIX install)             | Heavy: requires Microsoft Store-style deployment semantics, app must live under `WindowsApps`, updates routed through MSIX deployment APIs, harder to interact with the file system in expected ways. Loses control over installer UX. Reasonable for some apps, wrong for ours. |
| **Third-party "Context Menu Bridge" tools** (e.g., NilesoftShell, Custom Context Menu) | These force users to install yet another piece of software just to see our menu entry. Unacceptable UX. |
| **AutoHotkey-style hotkey hack** (e.g., bind `Ctrl+Shift+P` to current selection) | Doesn't solve the discoverability problem. Doesn't survive a reboot without a startup item. Doesn't appear in the menu at all. |
| **System tray drop target** (drag files onto a tray icon) | Doesn't fit the existing user behavior of right-click. Useful as an *additional* entry point, not as the primary one. |
| **Browser-style file picker** (open MaeclyPrint, then File → Open) | Defeats the entire premise of the product. |

## Consequences

### Positive

- **Top-level entry in the Win 11 modern context menu.** This is the entire reason for choosing this path.
- **Full file selection received** (no 15-file ceiling) because we use the proper `IShellItemArray` interface.
- **Per-user install.** Sparse MSIX with `-ExternalLocation` does not require admin and does not write to `HKLM` or `%PROGRAMFILES%`.
- **Installer remains user-friendly** (Inno Setup), not a black-box MSIX deployment. We get the best of both worlds: native installer UX + packaged identity.
- **DLL is tiny and auditable.** Confined to ~500-1000 LOC of carefully reviewed C++. Crash risk minimized; security surface minimal.
- **Sandboxing is natural.** The DLL does no file I/O and no networking; it serializes paths and spawns. Even a remote-code-execution-grade vulnerability in the DLL would be limited to "spawn the main exe with attacker-controlled args" — and the main exe validates each path.
- **Survives Tauri exe updates.** The DLL only knows the install directory; the new exe lives at the same path after update.

### Negative

- **C++ in the codebase.** We otherwise have only Rust + TypeScript. Contributors who want to touch the DLL need a Visual Studio C++ toolchain. Mitigated by the DLL being small, well-documented, and rarely changed.
- **Sparse MSIX is less common than full MSIX.** Some Windows admin tools may not show our app in their UIs the way they would for a full MSIX. Acceptable.
- **Re-registration on updates.** Every update must re-run `Add-AppxPackage -Register` because the install path may change subtly. Handled in the Inno Setup post-install script.
- **PowerShell required at install time.** Inno Setup invokes PowerShell to register the sparse package. Available on every Win 10/11 machine but is one more moving part.

### Risks / Open Questions

- **Future Win 11 changes.** Microsoft has tweaked context-menu behavior several times in 22H2 and 23H2. We pin our test matrix to specific Win 11 builds and have a CI job that loads the menu programmatically and asserts our entry is present.
- **ARM64 builds.** The DLL must be built for ARM64 separately and shipped as a separate artifact. Trivial in our build matrix.
- **Anti-virus interactions.** Aggressive AV products sometimes flag newly-deployed DLLs. EV signing (per ADR-0004 and the security model) mitigates this but does not eliminate it. We monitor false-positive reports and submit to AV vendors as needed.
- **Sparse MSIX manifest schema evolution.** The manifest references `desktop4` and `desktop5` namespaces, which could be deprecated over a multi-year horizon. Risk is low; Microsoft maintains backward compat.

## Related

- ADR-0001 (stack) — Tauri runs the main exe spawned by the DLL.
- ADR-0006 (Inno Setup installer) — installs the DLL and registers the sparse package.
- ADR-0007 (Win first, Mac second) — Mac uses Quick Action / Finder Sync, not COM.
- [`../04-architecture/shell-extensions.md`](../04-architecture/shell-extensions.md) — full implementation reference with code samples.
