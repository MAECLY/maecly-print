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

- `bundle.windows.nsis.installMode` is `"currentUser"`. The app installs under `%LOCALAPPDATA%\Programs\MaeclyPrint\` with no elevation.
- `bundle.createUpdaterArtifacts` is `true`. A release build also produces the updater payload (the NSIS setup on Windows, `MaeclyPrint.app.tar.gz` on macOS) and a minisign `.sig` for each one.
- `plugins.updater.pubkey` in `tauri.conf.json` holds the project's **public** minisign key. The private key is never committed. Release builds receive it through the `TAURI_SIGNING_PRIVATE_KEY` secret (and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` when the key has one). See [ci-cd.md](../07-build-and-release/ci-cd.md#updater-signing).
- `plugins.updater.endpoints` is `https://github.com/MAECLY/maecly-print/releases/latest/download/latest.json`. A release publishes `latest.json` (version, notes, and a `url` + `signature` per platform) as a GitHub release asset. The `MAECLYPRINT_UPDATE_ENDPOINT` environment variable replaces the endpoint for local end-to-end tests. Release builds still refuse plain `http`.
- On Windows the plugin runs the downloaded setup with `installMode: "passive"` (a progress bar, no questions), then the app exits and the installer restarts it. On macOS the verified `.app` is swapped in place and the user is offered a restart. Installing is refused for a binary that is not inside an installed `.app` bundle, so a development build never replaces its own build directory.
- Installing only ever starts from an explicit user action: **About → Install now**, or `maeclyprint update install [--yes]`. A major version jump needs a second confirmation (a dialog in the GUI, `--yes` or a typed "yes" on the CLI).
- `requireSignedVersion` stays **off** for now. The Tauri CLI used here (2.11) writes no `version:` field into the signature's trusted comment, so turning it on would reject every artifact we can produce. Turn it on once the CLI records the version and every release users can update from has been re-signed.

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

- **Sparse MSIX identity (ADR-0005).** This ADR does not register the sparse package that the Windows 11 context-menu handler needs. The intended route is an NSIS installer hook (`bundle.windows.nsis.installerHooks`) running `Add-AppxPackage -Register -ExternalLocation`. That is the shell-integration milestone's work, and until it lands the modern context-menu entry depends on how the developer registered it.
- **Release pipeline.** `release.yml`, which builds with the secret and uploads the artifacts, `.sig` files and `latest.json`, is not written yet. Until it exists, no build can be updated in place.
- **Channels (FR-085).** One `latest.json` serves every channel. Beta and nightly feeds would be separate manifests chosen by `updater.channel`.

## Related

- ADR-0004 — the updater plugin this installer has to work with.
- ADR-0005 — the sparse MSIX identity this ADR leaves open.
- ADR-0006 — superseded by this ADR.
- [`../07-build-and-release/installer-windows.md`](../07-build-and-release/installer-windows.md), [`auto-update-flow.md`](../07-build-and-release/auto-update-flow.md), [`ci-cd.md`](../07-build-and-release/ci-cd.md).
