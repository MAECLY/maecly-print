# Auto-Updater

> **TL;DR** — MaeclyPrint uses the **Tauri Updater plugin** (MIT). The app polls a static manifest at `https://updates.maecly.com/maeclyprint/<channel>/<platform>.json`, verifies its signature with an embedded minisign public key, downloads the platform-specific binary (or delta), verifies that signature too, and replaces the running binary on the next launch. Hosting is on S3 + CloudFront (or Cloudflare R2). Three channels: `stable`, `beta`, `nightly`.

## 1. Why the Tauri Updater plugin

Every alternative we evaluated required either (a) a third-party dependency that we'd have to maintain across two platforms, or (b) tying our release pipeline to GitHub Releases as the only download host. The Tauri Updater plugin gives us:

- A single MIT-licensed Rust crate (`tauri-plugin-updater`) shipped by the Tauri team.
- **Cryptographic signing** out of the box. Tauri uses minisign-style Ed25519 signatures.
- **Static manifest URL.** No server needed; any HTTPS-capable static host works.
- **Optional delta updates** when both old and new binaries can be hashed by the publisher (saves ~70% of bandwidth on small updates).
- **Cross-platform** — same code path on Windows and macOS.
- A clean Tauri command (`invoke('plugin:updater|check')`) that the React UI can drive.

Alternatives we rejected and why are documented in ADR [`0004-auto-updater-tauri-builtin`](../adr/0004-auto-updater-tauri-builtin.md).

## 2. The manifest

### 2.1 URL convention

```
https://updates.maecly.com/maeclyprint/{channel}/{platform}.json
```

Where:

- `{channel}` is `stable`, `beta`, or `nightly`.
- `{platform}` is one of:
  - `windows-x86_64`
  - `windows-aarch64`
  - `darwin-aarch64`  (Apple Silicon)
  - `darwin-x86_64`   (Intel Mac)

A request to a stable Windows x64 build:

```
GET https://updates.maecly.com/maeclyprint/stable/windows-x86_64.json
```

### 2.2 Manifest schema

```json
{
  "version": "1.2.3",
  "notes": "- Fix: queue would stall on encrypted PDFs\n- Add: Pandoc converter for .epub\n- Improve: cold start by ~120 ms on Win 11",
  "pub_date": "2026-06-01T12:00:00Z",
  "platforms": {
    "windows-x86_64": {
      "signature": "untrusted comment: signature from minisign secret key\nRWQf6LRCGA9i...base64...==\ntrusted comment: timestamp:1717243200 file:maeclyprint_1.2.3_x64-setup.nsis.zip\nC2k...base64...==",
      "url": "https://updates.maecly.com/maeclyprint/stable/binaries/maeclyprint_1.2.3_x64-setup.nsis.zip"
    },
    "windows-aarch64": {
      "signature": "...",
      "url": "https://updates.maecly.com/maeclyprint/stable/binaries/maeclyprint_1.2.3_arm64-setup.nsis.zip"
    },
    "darwin-aarch64": {
      "signature": "...",
      "url": "https://updates.maecly.com/maeclyprint/stable/binaries/MaeclyPrint_1.2.3_aarch64.app.tar.gz"
    },
    "darwin-x86_64": {
      "signature": "...",
      "url": "https://updates.maecly.com/maeclyprint/stable/binaries/MaeclyPrint_1.2.3_x64.app.tar.gz"
    }
  }
}
```

Field semantics:

| Field            | Type                  | Notes                                                                                        |
| ---------------- | --------------------- | -------------------------------------------------------------------------------------------- |
| `version`        | semver string         | The new version. Compared against `Cargo.toml`'s `version` using `semver::Version`.          |
| `notes`          | markdown string       | Shown in the in-app update prompt.                                                            |
| `pub_date`       | RFC 3339 timestamp    | Informational only.                                                                          |
| `platforms.<key>.signature` | minisign signature blob | Signs the bytes of the binary at `url`. Verified with the embedded public key. |
| `platforms.<key>.url` | HTTPS URL              | Must be HTTPS; mixed content rejected.                                                       |

### 2.3 Manifest signing

The manifest itself is *also* signed (separate `<platform>.json.sig` next to it) and the updater plugin can require this. We configure it to **always require** manifest signatures so a host compromise that swaps the JSON cannot inject a malicious binary URL.

```
https://updates.maecly.com/maeclyprint/stable/windows-x86_64.json
https://updates.maecly.com/maeclyprint/stable/windows-x86_64.json.sig
```

## 3. Hosting strategy

```
+------------------+   origin pull    +-------------------+   serves   +-----------+
| S3 / R2 bucket   |  <------------   | CloudFront / R2   |  -------->| End user  |
| updates.maecly   |                  | edge cache        |            | App       |
+------------------+                  +-------------------+            +-----------+
       ^
       | release pipeline upload
       | (GitHub Actions, signed before upload)
+------------------+
| build runner     |
+------------------+
```

- **Primary**: AWS S3 + CloudFront. ACL is private; CloudFront uses an Origin Access Identity. Edge caches binaries for 1 year (immutable filenames include version), manifests for 60 seconds.
- **Mirror (optional)**: Cloudflare R2 with a Workers route, mirrored from S3 nightly.
- **DNS**: `updates.maecly.com` CNAME → CloudFront distribution. ACM certificate, HTTPS only, HSTS.

The cost is small — manifests are <1 KB and binaries are 10-30 MB. A monthly active install base of 100k pulling one update per month is ~30 GB egress / month, well inside CloudFront's free tier or a few dollars on R2.

### 3.1 File layout in the bucket

```
s3://updates-maecly-com/maeclyprint/
├── stable/
│   ├── windows-x86_64.json
│   ├── windows-x86_64.json.sig
│   ├── windows-aarch64.json
│   ├── windows-aarch64.json.sig
│   ├── darwin-aarch64.json
│   ├── darwin-aarch64.json.sig
│   ├── darwin-x86_64.json
│   ├── darwin-x86_64.json.sig
│   └── binaries/
│       ├── maeclyprint_1.2.3_x64-setup.nsis.zip
│       ├── maeclyprint_1.2.3_x64-setup.nsis.zip.sig
│       ├── maeclyprint_1.2.3_arm64-setup.nsis.zip
│       ├── MaeclyPrint_1.2.3_aarch64.app.tar.gz
│       └── ...
├── beta/
│   └── ...
└── nightly/
    └── ...
```

## 4. Channels

| Channel  | Audience                           | Cadence            | Stability target           |
| -------- | ---------------------------------- | ------------------ | -------------------------- |
| `stable` | Default for all installs           | Every 4-6 weeks    | All tests + manual QA pass |
| `beta`   | Opt-in via Settings → Updates       | Weekly             | All tests pass             |
| `nightly`| Opt-in; built on `main` push        | Per merge to main  | CI green                   |

The channel is read from `~/.config/MaeclyPrint/config.toml`:

```toml
[updater]
channel = "stable"   # "stable" | "beta" | "nightly"
check_on_startup = true
startup_delay_seconds = 30
include_prerelease = false
```

Switching channels in the UI is non-destructive — it just changes the URL the plugin queries on the next check.

## 5. Tauri configuration

### 5.1 `tauri.conf.json` (excerpt)

```json
{
  "plugins": {
    "updater": {
      "active": true,
      "endpoints": [
        "https://updates.maecly.com/maeclyprint/{{channel}}/{{target}}-{{arch}}.json"
      ],
      "dialog": false,
      "pubkey": "untrusted comment: minisign public key\nRWQf6LRCGA9iPBuQc8s+u9xXqYxkB0tF5x3eO0X2c9aYx/D5ZtRq8s0H"
    }
  }
}
```

Notes:

- `dialog: false` — we render our own prompt UI instead of the Tauri-provided system dialog. This lets us match the rest of the app's React design.
- The `{{channel}}` token is interpolated at runtime from settings; we register a custom URL builder so the token is replaced by `stable`, `beta`, or `nightly`.
- `pubkey` is the *manifest* signing key. The binary signing key is verified against the same public key in our config (a single key signs both, simplifying key management).

### 5.2 Generating keys

```
tauri signer generate -w ~/.tauri/maeclyprint.key
```

This produces:

- `~/.tauri/maeclyprint.key` — secret. Stored in the release-runner's GitHub Actions secret as `TAURI_SIGNING_PRIVATE_KEY`.
- `~/.tauri/maeclyprint.key.pub` — public. Embedded in `tauri.conf.json`.

### 5.3 Signing during release

The release workflow runs:

```
TAURI_SIGNING_PRIVATE_KEY=$KEY \
TAURI_SIGNING_PRIVATE_KEY_PASSWORD=$PASS \
pnpm tauri build --target x86_64-pc-windows-msvc
```

Tauri produces the installer and a `.sig` file alongside it. The release script then:

1. Uploads `installer.zip` and `installer.zip.sig` to `s3://updates-maecly-com/maeclyprint/<channel>/binaries/`.
2. Generates the `<platform>.json` manifest with the signature inlined.
3. Signs the manifest itself: `minisign -S -m <platform>.json -s ~/.tauri/maeclyprint.key`.
4. Uploads `<platform>.json` and `<platform>.json.sig`.
5. Issues a CloudFront invalidation for `/maeclyprint/<channel>/*.json*`.

## 6. UX flow inside the app

### 6.1 Silent check on startup

```
T = 0          App launches.
T = 30 s       Background task fires: invoke('plugin:updater|check').
T = ~31 s      Manifest downloaded + verified.
                If version > current: emit `update-available` event.
                If version == current: no-op.
                If error: log, no UI.
```

We delay 30 s so that a context-menu invocation that needs to print *now* is not slowed by an HTTPS round trip.

### 6.2 Notification badge

When `update-available` fires, the React shell shows a small dot on the menu/avatar in the upper-right. Clicking it opens the **Updates** panel:

```
+------------------------------------------------+
| Updates                                        |
|                                                |
| MaeclyPrint 1.2.3 is available.               |
| (You are on 1.2.0.)                            |
|                                                |
| What's new:                                    |
|   - Fix: queue would stall on encrypted PDFs  |
|   - Add: Pandoc converter for .epub           |
|   - Improve: cold start by ~120 ms on Win 11  |
|                                                |
| [ Download and Install ]  [ Remind me later ] |
|                                                |
| Channel: [ stable v ]    [ Check for updates ]|
+------------------------------------------------+
```

### 6.3 Installation

Clicking **Download and Install** invokes `plugin:updater|download_and_install`. Progress is streamed to the UI as `download-progress` events. When complete:

- **Windows**: the new installer runs (NSIS or MSI, silent mode), the current process exits, and the new binary is launched on the user's confirmation. Sparse MSIX is re-registered.
- **Mac**: the tarball is extracted into a temporary directory, the running `.app` is replaced atomically, and the app relaunches.

### 6.4 Manual check

The **About** dialog has a `Check for updates` button that does the same thing as the startup task but synchronously, with a spinner.

## 7. Security properties

| Property                           | How we guarantee it                                          |
| ---------------------------------- | ------------------------------------------------------------- |
| Authenticity of manifest           | minisign signature on `<platform>.json` verified against embedded public key. |
| Authenticity of binary             | Separate minisign signature on the binary; key may be the same or different. |
| Transport security                 | HTTPS only (`http://` URLs in manifest are rejected by the plugin). |
| Downgrade attack resistance        | Plugin only installs versions strictly > current.            |
| Update host compromise (S3 swap)   | Attacker cannot forge signatures without the secret key, which never leaves CI/HSM. |
| TOFU on first install              | Public key is shipped *inside the binary*; first-run trust is bootstrapped by the installer signature (EV cert on Windows, Developer ID on Mac). |
| Replay (older signed binary)       | Version comparison rejects.                                  |

## 8. Failure modes

| Failure                            | Handling                                                                                |
| ---------------------------------- | ---------------------------------------------------------------------------------------- |
| Manifest URL DNS fails             | Log; show "Couldn't reach update server" in About panel; retry on next startup.          |
| Manifest 5xx                       | Treat as transient; backoff (1 min, 5 min, 30 min, capped at 12 h).                      |
| Manifest signature invalid         | **Hard fail.** Log a warning and *never* prompt. This is a tampered host situation.     |
| Binary signature invalid           | **Hard fail.** Delete the partial download. Show user "Update was corrupted; please retry." |
| Disk full during download          | Abort, clean partial. User-facing error.                                                 |
| Process replace fails (file lock)  | Defer to next launch; show banner "Update queued — restart MaeclyPrint to apply."        |

## 9. Operational checklist for releases

1. Bump version in `Cargo.toml`, `package.json`, and `tauri.conf.json`.
2. Update `CHANGELOG.md` (release notes are extracted from this for the manifest).
3. Push tag `v1.2.3`.
4. CI: build for all four targets, sign installers, upload binaries + sigs to S3.
5. CI: generate four `<platform>.json` manifests with the platform's binary signature inlined.
6. CI: sign each manifest with minisign.
7. CI: upload manifests + manifest signatures to the right channel folder.
8. CI: CloudFront invalidate `/maeclyprint/<channel>/*.json*`.
9. Manual: smoke-test by a maintainer on each platform (auto-update from N-1 → N).

## 10. Related documents

- [`security-model.md`](./security-model.md) — broader signing and key management context.
- [`tech-stack.md`](./tech-stack.md) — why Tauri, which gives us this updater for free.
- ADR [`0004-auto-updater-tauri-builtin`](../adr/0004-auto-updater-tauri-builtin.md) — the decision rationale.
