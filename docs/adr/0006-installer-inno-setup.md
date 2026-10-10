# ADR-0006: Installer — Inno Setup (+ Sparse MSIX for shell extension)

> Superseded by [ADR-0008](./0008-installer-nsis-per-user.md): the Windows installer is Tauri's per-user NSIS bundle, because the updater has to run it.

## Status

Superseded by ADR-0008

## Date

2026-05-03

## Context

We need to ship MaeclyPrint to Windows users in a way that:

- Installs without administrator rights (per ADR-0005's per-user model).
- Places the binaries somewhere the auto-updater can replace them (per ADR-0004).
- Runs PowerShell at install time to register the Sparse MSIX package (per ADR-0005).
- Bundles SumatraPDF (~25 MB) and our own binaries (~10 MB).
- Optionally lets the user opt into installing converters (LibreOffice, ImageMagick, Pandoc) via download.
- Generates an installer that can be code-signed with our EV certificate.
- Generates an uninstaller that completely removes our footprint (binaries, sparse MSIX registration, registry entries, but **not** user data unless explicitly chosen).
- Is itself open source / freely usable for an open source project.

The realistic candidates on Windows are: NSIS, Inno Setup, WiX (MSI), MSI directly, and pure MSIX deployment.

## Decision

We will use **Inno Setup 6.x** to build the primary Windows installer, augmented with a **Sparse MSIX package** registered at install time for the shell-extension identity.

Concretely:

- The user downloads `maeclyprint_<version>_x64-setup.exe`, ~30 MB.
- Inno Setup installs to `%LOCALAPPDATA%\Programs\MaeclyPrint\` (per-user, no admin).
- Inno Setup writes:
  - `MaeclyPrint.exe` (the Tauri main binary)
  - `MaeclyPrintShellExt.dll` (the C++ COM DLL)
  - `redist\SumatraPDF\` (bundled SumatraPDF binaries)
  - `AppxManifest.xml` and a small `Assets/` folder for the sparse package
  - Start Menu shortcut + uninstall registry entry
- A post-install PowerShell step registers the sparse MSIX package via `Add-AppxPackage -Register -ExternalLocation`.
- The fallback registry-only shell-extension entry under `HKCU\Software\Classes\*\shell\MaeclyPrint` is also written, gated on Windows version (Win 10 only by default; Win 11 users can opt in via "Always show in classic menu too").
- Uninstall reverses all of the above. User data in `%APPDATA%\Maecly\MaeclyPrint\` and `%LOCALAPPDATA%\Maecly\MaeclyPrint\` is preserved unless the user checks "Also remove my settings and queue history".

For the **post-1.0 Microsoft Store distribution** (later, optional), we will produce a *full MSIX* package as a separate artifact. That is not in scope for this ADR.

## Alternatives Considered

| Option                               | Why rejected                                                                                              |
| ------------------------------------ | --------------------------------------------------------------------------------------------------------- |
| **NSIS**                             | Capable and free, but its scripting language is quirky and harder to maintain than Inno's Pascal-like script. Less polished default UX. Many of our competitors use NSIS; it works but Inno produces nicer installers with less effort. |
| **WiX Toolset → MSI**                | Generates real MSI installers (good for enterprise admins), but XML-heavy authoring, steeper learning curve, harder to do per-user installs without admin tricks (ALLUSERS=2 etc.). Heavier than we need. |
| **Plain MSIX** (no Inno Setup)       | Forces our app under `WindowsApps`, which has restricted file access semantics. Auto-updater would route through MSIX deployment APIs (not Tauri's). Loses our control over installer UX and updater. Acceptable as a *secondary* artifact for Microsoft Store; not as the primary install. |
| **Custom Rust installer** (e.g., Cargo Bundle)  | Tauri's bundler can produce installers but the Win NSIS template has limitations and the MSI template is rigid. We need custom logic (PowerShell post-install, sparse MSIX, optional converter download), and Inno Setup is the easiest path to that. |
| **Velopack** (also handles installs)         | Mostly an updater; its installer story is weaker than Inno Setup. Per ADR-0004 we don't use Velopack anyway. |
| **InstallShield / Advanced Installer (commercial)** | Powerful, but commercial licenses are not aligned with an open source project's ethos and budget. Overkill. |

## Consequences

### Positive

- **Per-user install with no admin.** Installs cleanly into `%LOCALAPPDATA%\Programs\MaeclyPrint\`.
- **Custom logic is easy.** Inno Setup's Pascal Script lets us run PowerShell, conditionally install components, prompt the user, etc., with little ceremony.
- **Polished UX out of the box.** Modern wizard, dark/light theme support, multi-language strings, license display.
- **Code-signing-friendly.** `signtool sign /f cert.pfx /p $PASS Output.exe` works on the generated installer; CI signs both the installer and its embedded executables.
- **Free and open source.** No license cost.
- **Sparse MSIX registration is a one-line PowerShell command.** Easily integrated into the post-install step.
- **Uninstaller is generated automatically.** With our cleanup script appended.
- **Bundled SumatraPDF stays in the install directory** alongside our binary, which the auto-updater knows how to update atomically.

### Negative

- **Inno Setup is Windows-only**, but that is fine — we use Tauri's bundler for the Mac `.dmg`. They are different installers for different OSes.
- **Pascal Script is a niche skill.** Mitigated by the script being relatively short and well-commented.
- **CI build needs Inno Setup compiler installed on the Windows runner.** Adds ~50 MB to the runner image. Acceptable.
- **Not an MSI.** Some enterprise IT departments only deploy MSIs. We will publish an MSI variant for those customers in a later milestone, generated separately by WiX from the same payload.

### Risks / Open Questions

- **Sparse MSIX registration failure.** If `Add-AppxPackage` fails (rare, but possible on policy-locked machines), the modern context menu entry will not appear; the registry fallback still works. The installer reports the failure but does not abort the install.
- **PowerShell execution policy.** Some users have restrictive policies. We invoke PowerShell with `-ExecutionPolicy Bypass -File <script>` from Inno Setup, which works in all common configurations.
- **WebView2 Runtime on Win 10.** We bundle the Microsoft-supplied bootstrapper and run it if WebView2 is missing. The bootstrapper handles its own UAC prompt if it needs to install machine-wide. Win 11 has WebView2 preinstalled.
- **Inno Setup 7.** When the upstream releases v7, we'll evaluate it; 6.x is stable and well-supported.

## Related

- ADR-0004 (auto-updater) — must replace files in the Inno Setup install directory atomically.
- ADR-0005 (sparse MSIX shell ext) — registered by the Inno post-install step.
- ADR-0007 (Win first, Mac second) — Mac uses a `.dmg` from Tauri's bundler instead.
- [`../07-build-and-release/`](../07-build-and-release/) — full release pipeline.
