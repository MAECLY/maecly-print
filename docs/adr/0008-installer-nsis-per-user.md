# ADR-0008: Installer — Tauri's per-user NSIS bundle, with signed updater artifacts

## Status

Accepted. Supersedes [ADR-0006](./0006-installer-inno-setup.md).

## Date

2026-10-09

## Context

[ADR-0006](./0006-installer-inno-setup.md) chose Inno Setup 6 plus a sparse MSIX package as the Windows installer. That decision was never implemented. `installers/windows/` holds only a `.gitkeep`, there is no `.iss` script, and every Windows build the project has ever produced is the NSIS installer from Tauri's own bundler (`bundle.windows.nsis.installMode: "currentUser"`). [installer-windows.md](../07-build-and-release/installer-windows.md) recorded the contradiction and asked for a decision.

The decision is now forced by the updater. FR-082 (verify the signature of every downloaded update), FR-083 (install without administrator rights) and FR-084's "install now" are implemented with `tauri-plugin-updater`, as [ADR-0004](./0004-auto-updater-tauri-builtin.md) planned. On Windows that plugin installs an update by running **the NSIS setup produced by the same bundler**, in passive mode, and it needs that bundler's signed updater artifacts (`createUpdaterArtifacts`). An Inno Setup installer is not something the plugin can drive, so keeping ADR-0006 would mean either a second, hand-written update path or two different installers for first install and for updates.

Constraints carried over unchanged:

- Per-user install, no UAC prompt (ADR-0005's per-user model, FR-083).
- Every update verified against a key the app ships with before anything on disk changes (FR-082).
- A major version is never installed without an explicit, separate opt-in (FR-087).
- `SumatraPDF.exe` is not committed. Every packaging job runs `scripts/fetch-sumatrapdf.ps1` first.

## Decision

We will ship Windows builds as **Tauri's NSIS installer in `currentUser` mode** and macOS builds as Tauri's `.app`/`.dmg`, and we will **sign updater artifacts** for both.

Concretely:

- `bundle.windows.nsis.installMode` is `"currentUser"`. The app installs under `%LOCALAPPDATA%\MaeclyPrint\` (Tauri's per-user default, `$LOCALAPPDATA\${PRODUCTNAME}`; verified on Windows 11) with no elevation, and its uninstall entry is under HKCU. `bundle.targets` lists `nsis` and no `msi`: the WiX MSI Tauri can also build is per-machine by default, so it is not produced.
- `bundle.createUpdaterArtifacts` is `true`. A release build also produces the updater payload (the NSIS setup on Windows, `MaeclyPrint.app.tar.gz` on macOS) and a minisign `.sig` for each one.
- `plugins.updater.pubkey` in `tauri.conf.json` holds the project's **public** minisign key. The private key is never committed. Release builds receive it through the `TAURI_SIGNING_PRIVATE_KEY` secret and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`. Release keys must have a password: a passwordless key makes a Windows PowerShell build hang at Tauri's password prompt, because PowerShell cannot set an empty environment variable (see ci-cd.md). See [ci-cd.md](../07-build-and-release/ci-cd.md#updater-signing).
- `plugins.updater.endpoints` is `https://github.com/MAECLY/maecly-print/releases/latest/download/latest.json`. A release publishes `latest.json` (version, notes, and a `url` + `signature` per platform) as a GitHub release asset. The `MAECLYPRINT_UPDATE_ENDPOINT` environment variable replaces the endpoint for local end-to-end tests. Release builds still refuse plain `http`.
- On Windows the plugin runs the downloaded setup with `installMode: "passive"` (a progress bar, no questions), then the app exits and the installer restarts it. On macOS the verified `.app` is swapped in place and the user is offered a restart. Installing is refused for a binary that is not inside an installed `.app` bundle, so a development build never replaces its own build directory.
- Installing only ever starts from an explicit user action: **About → Install now**, or `maeclyprint update install [--yes]`. A major version jump needs a second confirmation (a dialog in the GUI, `--yes` or a typed "yes" on the CLI).
- `plugins.updater.requireSignedVersion` is `true` (since the move to Tauri 2.12). The manifest is not signed, so on its own its `version` does not prove which release its `url` and `signature` point at. The release workflow signs every update artifact with `tauri signer sign --app-version <version>`, which writes a `version:` field into the signature's trusted comment (covered by the signature), and the updater refuses an artifact whose signed version is missing or differs from the version the manifest announces. This was off while the Tauri CLI (2.11) wrote no such field. It only constrains releases the app updates *to*: 0.0.1, signed without a version, is never an update target for a build that has the flag on.

CI builds for pull requests have no signing key. They bundle with `--config src-tauri/tauri.ci.conf.json`, which turns `createUpdaterArtifacts` off. Without that override Tauri refuses to bundle ("A public key has been found, but no private key").

## Alternatives Considered

| Option | Why rejected |
| --- | --- |
| **Implement ADR-0006 (Inno Setup)** | `tauri-plugin-updater` cannot run an Inno Setup installer. We would need our own download, verify and install code for Windows, which is the security-critical part ADR-0004 chose not to write. |
| **Inno Setup for first install, NSIS for updates** | Two installers that both write the same install directory and uninstall entry, and can drift. Users would end up with different installs depending on how they got the current version. |
| **WiX / MSI** | The plugin supports MSI, but a per-user MSI needs `ALLUSERS=2`/`MSIINSTALLPERUSER` tricks, and MSI authoring is heavier than we need. Still a possible extra artifact for enterprise deployment later. |
| **Unsigned updates, verify by HTTPS only** | Violates FR-082. TLS proves who served the file, not who built it. |

## Consequences

### Positive

- One installer format per OS, produced by the same `pnpm tauri build`, and it is the one the updater knows how to run.
- FR-082 and FR-083 are met by configuration plus a small policy layer (`src-tauri/src/update/install.rs`) rather than custom installer code.
- A tampered or re-signed artifact is rejected before anything is written, and the user is told so in plain words (`UpdateSignatureInvalid`).

### Negative

- NSIS customisation (extra pages, custom actions) goes through Tauri's NSIS template and installer hooks, which are less flexible than Inno Setup's Pascal Script.
- The updater private key is now a release-critical secret. If it is lost, installed copies can no longer be updated in place. If it leaks, an attacker who can also serve `latest.json` could ship a signed update. Store it offline as well as in the CI secret.

### Risks / Open Questions

- **Context menu (ADR-0005).** The installer hooks (`src-tauri/windows/installer-hooks.nsh`) install `shell_ext_win.dll` next to `maeclyprint.exe` and register it per user, under HKCU only: the in-process CLSID and an `ExplorerCommandHandler` verb on files and folders. That entry shows in the Windows 10 menu and in the Windows 11 classic menu (*Show more options*, Shift+F10), with no item limit. Uninstall removes the keys; a DLL Explorer still holds is renamed aside (to `%TEMP%`, or next to itself) and deleted at the next logon through an HKCU `RunOnce` value, because a per-user setup cannot schedule a delete for the next restart (that writes HKLM). The Windows 11 **top-level** entry additionally needs the sparse package (`Add-AppxPackage -Register -ExternalLocation`), and outside Developer Mode Windows only registers a sparse package that is signed with a certificate the machine trusts. That waits on the code-signing certificate (NFR-021); until then the top-level entry is not shipped. The per-user install and the classic menu entry were verified with UAC on from a medium-integrity session (installer-windows.md); with UAC off Explorer runs elevated and ignores HKCU COM registrations, so the entry does not show there.
- **Release pipeline.** `release.yml`, which builds with the secret and uploads the artifacts, `.sig` files and `latest.json`, is not written yet. Until it exists, no build can be updated in place.
- **Channels (FR-085).** One `latest.json` serves every channel. Beta and nightly feeds would be separate manifests chosen by `updater.channel`.

## Related

- ADR-0004 — the updater plugin this installer has to work with.
- ADR-0005 — the sparse MSIX identity this ADR leaves open.
- ADR-0006 — superseded by this ADR.
- [`../07-build-and-release/installer-windows.md`](../07-build-and-release/installer-windows.md), [`auto-update-flow.md`](../07-build-and-release/auto-update-flow.md), [`ci-cd.md`](../07-build-and-release/ci-cd.md).
