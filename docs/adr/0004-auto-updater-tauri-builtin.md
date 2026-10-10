# ADR-0004: Auto-updater — Tauri Updater plugin

## Status

Accepted

## Date

2026-05-03

## Context

A polished desktop app needs a reliable auto-update story. MaeclyPrint is distributed outside any app store, so the OS will not update it for us. Users expect to:

- Be notified when a new version is available.
- Download and install the update without admin rights.
- Trust that the update has not been tampered with.
- Continue working if the update fails.

Constraints:

- We do not want to depend on GitHub Releases as the only download host. We want to host updates on our own infrastructure (`updates.maecly.com`).
- We must support delta updates eventually (full binaries are 10-30 MB; over millions of installs that adds up).
- Cryptographic signatures on every download must be verifiable by the embedded public key.
- The same updater must work on Windows and macOS.
- Per ADR-0001, our app framework is Tauri 2; we should prefer first-party Tauri solutions when available.

## Decision

We will use the **`tauri-plugin-updater`** crate (MIT-licensed, maintained by the Tauri team) as the auto-updater for MaeclyPrint.

Specifically:

- Updates are configured via static **JSON manifests** at `https://updates.maecly.com/maeclyprint/{channel}/{platform}.json`.
- Manifests are signed (separate `.sig` file) with **minisign**; the public key is embedded in the application binary.
- Binaries are independently signed with minisign; the signature is included inline in the manifest.
- The Tauri Updater plugin handles download, signature verification, atomic install, and relaunch.
- We render our own UI for the update prompt (custom React, not the Tauri-supplied dialog).
- Three channels are supported: `stable`, `beta`, `nightly`. The user picks one in Settings.
- A silent check fires 30 s after app launch and on demand from the **About** dialog.

## Alternatives Considered

| Option                                | Why rejected                                                                                                |
| ------------------------------------- | ------------------------------------------------------------------------------------------------------------ |
| **Velopack** (.NET, Squirrel-style)   | Excellent tool, but it is a .NET app — adds the .NET runtime to our build environment. Designed for .NET apps; using it from a Tauri app means another sidecar process to manage. |
| **Squirrel.Windows** (legacy)         | Older, Windows-only, less actively maintained. NuGet client. We need cross-platform; Squirrel.Mac is a separate codebase. |
| **NetSparkle / WinSparkle**           | C#/C++ libraries; great APIs, but again not first-party for Tauri and not cross-platform from one library. WinSparkle's manifest format is XML-only (less flexible than JSON). |
| **Custom updater** (rolled in-house)  | Plenty of pitfalls: signature verification, atomic file replacement on Windows (file in use), MSIX re-registration, macOS `.app` swap, retry semantics. Would consume weeks of dev time and never be as well-tested as the Tauri plugin. |
| **GitHub Releases as host (no separate manifest server)** | Couples our release pipeline to GitHub's availability and rate limits. Loses control over CDN behavior. The Tauri plugin can use GitHub Releases directly, but we can also use it with our own host — and the latter is preferable for branding and reliability. |
| **App store distribution only** (Mac App Store, Microsoft Store) | We considered this for macOS. Rejected because (a) GPL-family licences (GPLv3 then, AGPL-3.0 since ADR-0010) have known friction with the Mac App Store, (b) Microsoft Store reduces our control, (c) the app stores are slow at update review. We can revisit later for the Microsoft Store as an *additional* distribution channel. |
| **Tauri's built-in dialog (instead of custom UI)** | The plugin's default dialog is functional but doesn't match our React design system. We use the plugin's *backend* and render our own React UI. |

## Consequences

### Positive

- **First-party for Tauri.** Maintained by the same team that maintains the framework. Bug reports go to one place.
- **MIT-licensed.** Compatible with our AGPL-3.0 codebase (MIT can be combined into AGPL-3.0 outputs; GPLv3 when this ADR was written).
- **Signed updates with minisign (Ed25519).** Strong cryptography, well-understood library.
- **Cross-platform with one configuration.** Same `tauri.conf.json` block; only the binary URL pattern differs per platform.
- **Static-host friendly.** A JSON file on S3 + CloudFront is the entire server. No Lambdas, no Node service, no ops burden.
- **Independent of GitHub Releases.** We can push updates without touching GitHub if we ever need to.
- **Delta updates supported.** Plugin can apply binary diffs when both source and target are known and signed.
- **Atomic install.** Plugin handles "file is in use" by deferring to next launch on Windows.

### Negative

- **Coupled to Tauri's release cadence.** When Tauri ships a major version, we may need to wait for the plugin to follow. Mitigated by Tauri's track record of LTS support for plugins.
- **Manifest must be regenerated and signed on each release.** Adds ~30 s to the release pipeline. Automated in CI.
- **Custom UI work.** We don't get a free dialog; we have to build our own. We were going to anyway because of branding consistency.
- **Public key is baked in.** Rotating keys requires a coordinated release that ships an interim binary accepting both old and new pubkeys. Manageable; documented in [`../04-architecture/auto-updater.md`](../04-architecture/auto-updater.md).

### Risks / Open Questions

- **CDN cost growth.** Currently negligible at small install bases; needs revisiting at millions of installs. Cloudflare R2 is a strong alternative if AWS becomes too expensive.
- **macOS notarization on every release.** Mandatory for newer macOS; adds 5-15 min to the release pipeline. Already planned.
- **Sparse MSIX re-registration on update.** When updating on Windows, we need to re-register the sparse package (the binary path changes if the version is in the path). The Inno Setup script handles this, but it's a step we mustn't forget. Documented in the release runbook.
- **What happens if the user turns off network for weeks?** The app keeps working on its current version. On reconnect, the next startup check picks up. No forced updates ever.

## Related

- ADR-0001 (Tauri stack) — this decision is enabled by Tauri.
- ADR-0007 (Win first, Mac second) — explains the staggered rollout schedule for the Mac update path.
- [`../04-architecture/auto-updater.md`](../04-architecture/auto-updater.md) — implementation details, manifest schema, hosting layout.
- [`../04-architecture/security-model.md`](../04-architecture/security-model.md) — broader signing and key management context.
