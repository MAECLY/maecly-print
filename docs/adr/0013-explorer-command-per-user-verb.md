# ADR-0013: The Explorer command ships as a per-user registry verb; the sparse package waits for a certificate

## Status

Accepted

## Date

2026-10-10

## Context

[ADR-0005](./0005-shell-extension-sparse-msix.md) chose a C++ COM DLL implementing `IExplorerCommand`, given a packaged identity by a sparse MSIX package registered from an Inno Setup installer, so that the entry would appear in the Windows 11 **top-level** context menu. By round 3 three of those premises had changed:

- The DLL is Rust (`shell-ext-win/`, `shell_ext_win.dll`), with its own release profile so a panic is caught at the COM boundary.
- The installer is Tauri's per-user NSIS bundle ([ADR-0008](./0008-installer-nsis-per-user.md)), not Inno Setup.
- Releases carry no Apple or Microsoft certificate ([ADR-0011](./0011-release-channel.md)). Outside Developer Mode, Windows registers a sparse package only when it is signed with a certificate the machine trusts, so the sparse package cannot be shipped (decision D3).

The same `IExplorerCommand` handler can also be registered without any package, as an `ExplorerCommandHandler` verb under `HKCU\Software\Classes`. Explorer shows such a verb in the Windows 10 menu and in the Windows 11 classic menu (**Show more options**, Shift+F10), and hands it the whole selection, so the ~15-file limit of the built-in Print verb does not apply.

## Decision

We will register the Explorer command **per user, as a registry verb**, from the NSIS installer hooks (`src-tauri/windows/installer-hooks.nsh`), and keep the sparse package designed but unbuilt until a code-signing certificate exists:

- `HKCU\Software\Classes\CLSID\{B269B8DC-044F-4D65-8F1E-8F20A2E653EA}\InprocServer32` points at `shell_ext_win.dll` in the install folder; `*\shell\Pluryprint` and `Directory\shell\Pluryprint` name that CLSID as their `ExplorerCommandHandler`. No `HKLM` key and no elevation.
- The installer carries the DLL for x64 and ARM64 and installs the one for the **operating system's** native architecture: Explorer on Windows on ARM is a native ARM64 process and cannot load an x64 DLL, even when the x64 installer runs there emulated. A CI build without both DLLs fails.
- A loaded DLL is renamed aside on update and deleted later; uninstall removes the keys and the DLL.
- The sparse package (`AppxManifest.xml`, packaging script, conditional `Add-AppxPackage -ExternalLocation`) stays as the design in [shell-extensions.md §2.5](../04-architecture/shell-extensions.md), to be built when a certificate is bought. The DLL is already the `IExplorerCommand` it would declare.

## Alternatives Considered

| Option | Why rejected |
|---|---|
| Ship the sparse package signed with a self-signed certificate | The certificate would have to be installed into the machine's trusted store, which needs elevation and teaches users to trust a certificate from a download. |
| Full MSIX for the whole app | Changes distribution and updates (no per-user NSIS, no Tauri updater), and still needs a trusted certificate. |
| A classic `IContextMenu` handler | Tried in round 3: it shows in the same classic menu and brings nothing over the `IExplorerCommand` verb. |
| Registry-only `%1` command per file | One process per file and no submenu of profiles; the selection limit applies again. |

## Consequences

### Positive

- The right-click entry works for a normal user right after a per-user install, on Windows 10 and Windows 11, for any number of files. Verified on Windows 11 24H2 ARM64 with UAC on, with real right-clicks.
- No certificate, no elevation, nothing installed outside the user's profile.

### Negative

- On Windows 11 the entry is one click deeper, under **Show more options**. The UI says so (Home, the first-run tour, Settings).
- With UAC turned off, Explorer runs elevated and ignores COM classes registered under `HKCU`, so the entry does not appear. That is a Windows rule for every per-user shell extension.

### Risks / Open Questions

- The x64 installer running emulated on Windows on ARM has not been tested; the architecture choice is covered by the hooks' unit test and the native ARM64 install.
- No Windows 10 machine has run the verb; it is the same registration the Windows 11 classic menu uses.
- FR-002 (the top-level Windows 11 entry) stays open until the sparse package ships.

## Related

- ADR-0005 — superseded by this ADR.
- ADR-0008 — the per-user NSIS installer that registers the verb.
- ADR-0011 — why there is no certificate.
- [`../04-architecture/shell-extensions.md`](../04-architecture/shell-extensions.md) — the DLL, the keys and the round-3 verification.
