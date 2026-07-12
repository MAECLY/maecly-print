# Security Model

> **TL;DR** — MaeclyPrint signs every artifact (installer, binary, update manifest), runs without admin privileges, isolates the high-risk shell-extension code in a tiny C++ DLL, narrows the webview to a vetted IPC surface via Tauri capabilities, and ships with telemetry **off** by default. The only outbound network destination in default configuration is `updates.maecly.com`.

## 1. Threat model

We start by being explicit about what we are defending against and what is out of scope.

### 1.1 In scope

| Threat                                                         | What we do about it                                                  |
| -------------------------------------------------------------- | -------------------------------------------------------------------- |
| Malicious update binary served by a compromised CDN host       | minisign signature verification on both the manifest and the binary, both signed offline |
| Modified installer downloaded from a tampered mirror           | Authenticode (Win EV cert) and Apple Developer ID notarization       |
| Malicious context-menu invocation tricking the user into running arbitrary code | Shell DLL only handles paths; main exe validates each path before processing |
| Webview compromise (XSS in a render path) escalating to arbitrary IPC calls | Closed command set (`collect_commands!`, no dynamic surface) + CSP + no remote content loaded |
| Local user without admin rights wanting to install / use the app | Install to `%LOCALAPPDATA%`, register COM in HKCU, no service required |
| File-content exfiltration via logs                             | Logs never include file content; paths only at INFO+ when explicitly enabled |
| User wanting to audit network behavior                         | Single outbound endpoint by default; everything else is opt-in        |
| Supply-chain attack on a Rust crate                            | `cargo audit` in CI, pinned `Cargo.lock`, deny unmaintained crates    |
| Supply-chain attack on an npm package                          | `pnpm audit`, lockfile-only installs in CI, dependency review on bumps |
| Privilege-escalation exploit in a converter (e.g., LibreOffice CVE) | Converters spawned as the current user, killed at timeout, never as SYSTEM |

### 1.2 Out of scope

- A user with administrator access on their own machine. (No defense against the local admin; that is the OS's job.)
- Targeted attacks against a specific user with physical access.
- Cryptanalytic attacks on Ed25519 / SHA-256 / minisign.
- Print-content interception by the local network (if the user prints to a network printer over an insecure protocol, that is the spooler's concern, not ours).
- Side-channel attacks (timing, EM, etc.).

## 2. Code signing

### 2.1 Windows: EV cert + Authenticode

From day 1 (`v0.1.0` GA), all release artifacts are signed with an **Extended Validation** code-signing certificate. We use Sectigo or DigiCert depending on price; both produce certs that count toward Microsoft SmartScreen reputation immediately, removing the "Unknown publisher" warning.

Signed artifacts:

- The main executable: `MaeclyPrint.exe`.
- The shell-extension DLL: `MaeclyPrintShellExt.dll`.
- The Inno Setup installer: `maeclyprint_<version>_x64-setup.exe`.
- The Sparse MSIX package: `MaeclyPrint.msix` (signed via `signtool`).

Signing happens in the GitHub Actions release workflow on a Windows runner with the EV key residing in an Azure Key Vault HSM accessed through `signtool` and the Azure Code Signing extension. The key never lives on disk in CI.

Cost is ~$300/year for an EV cert. For nightly builds we sign with a self-signed cert (the user gets a SmartScreen warning; nightly users accept this).

### 2.2 macOS: Developer ID + notarization (M5+)

From the first macOS release:

- The `.app` bundle and `.appex` (Finder Sync Extension) are signed with **Apple Developer ID Application**.
- The `.dmg` is signed with **Apple Developer ID Installer**.
- Both are notarized via `notarytool`, then stapled.

Signing keys live in macOS Keychain on the macOS runner, fetched from a 1Password / GitHub Actions secret at the start of the job, scrubbed at the end.

### 2.3 Update integrity

See [`auto-updater.md`](./auto-updater.md) for the full flow. Summary:

- Manifests (`<platform>.json`) are signed with **minisign** (Ed25519).
- Binaries are independently signed with **minisign**, signature is included inline in the manifest.
- The minisign **public key is embedded in the application binary** itself (in `tauri.conf.json` and compiled into the updater plugin). It cannot be replaced without re-installing.
- The minisign **secret key** never leaves the release runner's encrypted secret store.
- A compromised CDN cannot serve a malicious binary because the attacker cannot forge a signature; the updater plugin will refuse to install.

Key rotation policy: keys are rotated every 24 months or immediately on suspected compromise. Rotation requires a coordinated release that ships an updated binary with both old and new pubkeys (accepting either), followed by a release a few weeks later that drops the old key.

## 3. Sandboxing and process boundaries

### 3.1 Shell-extension DLL

The C++ DLL runs **in the address space of `explorer.exe`**, which is the most dangerous place possible for code: a crash brings down Explorer; a vulnerability exposes the entire user session.

Mitigations:

- **Tiny code surface.** The whole DLL is ~500-1000 LOC. Auditable in an afternoon.
- **No allocations larger than necessary.** Pre-sized buffers; bounded JSON growth; explicit bounds checks.
- **No file I/O.** The DLL reads paths from `IShellItemArray` and writes only command-line arguments or a single tempfile. It never opens, reads, or modifies the user's documents.
- **No networking.** Outright forbidden by code review.
- **`/guard:cf`, `/CETCOMPAT`, `/DYNAMICBASE`, `/HIGHENTROPYVA`, `/NXCOMPAT`, `/SAFESEH`** all enabled.
- **`__declspec(noinline)` + structured exception filters** on the public entry points to prevent SEH-based exploits from escalating.
- **Fuzz testing** of the path-serialization code with `cargo-fuzz` (against a Rust port of the routine).
- **No third-party libraries.** Only `windowsapp.lib` and `wil` (Microsoft's Windows Implementation Library — header-only).

### 3.2 Tauri main process

Runs as the user; not elevated. Has read/write access to:

- `%APPDATA%\MaeclyPrint\` and `%LOCALAPPDATA%\MaeclyPrint\`.
- The user's documents (because we are spawned by the user; no extra access).

Does **not**:

- Bind to any port.
- Modify `HKLM` or `%PROGRAMFILES%`.
- Write to any other user's files.
- Self-elevate.

### 3.3 Webview

The webview's Rust-facing surface is bounded two ways. **First, the closed command set:** the only commands that exist are those we register in `tauri_specta::collect_commands!` (`src-tauri/src/lib.rs`) — there is no dynamic or third-party command surface, so a compromised webview cannot reach a command we did not write. (Tauri 2's capability system gates *core and plugin* permissions — window, webview, dialog, shell — **not** app-defined `#[tauri::command]`s; `capabilities/main.json` therefore lists only `core:*` defaults plus `dialog:default`. See [`ipc-and-cli.md`](./ipc-and-cli.md) § 2.5.) **Second, and the real backstop:** there is no attacker-controlled code in the webview to abuse that surface — a strict CSP loads only the bundled SPA and forbids remote content, so a hypothetical XSS has no remote script to run.

CSP for the main window:

```
default-src 'self';
script-src 'self';
style-src 'self' 'unsafe-inline';     /* tailwind */
img-src 'self' data: blob:;
connect-src 'self' ipc: tauri: https://updates.maecly.com;
font-src 'self';
object-src 'none';
frame-ancestors 'none';
```

We never `loadUrl()` remote content. The webview only loads the bundled SPA.

### 3.4 Converter subprocesses

Each converter is spawned with `tokio::process::Command`:

- As the current user (no elevation).
- With a hard timeout (default 60 s).
- With its working directory set to `cache_dir` (so any incidental temp files go there).
- With env minimized to `PATH`, `TEMP`, `TMP`, `LOCALAPPDATA`, `APPDATA`, `USERPROFILE`, `HOME` — not the full inherited env.
- With stdout and stderr captured (length-bounded).
- With `kill_on_drop(true)` so leaving scope (panic, cancellation) terminates the child.

LibreOffice runs with `-env:UserInstallation=...` to a temp profile so we don't pollute the user's normal LibreOffice profile.

## 4. Permissions

### 4.1 No admin required

Installation:

- Inno Setup installs to `%LOCALAPPDATA%\Programs\MaeclyPrint` (per-user install).
- Sparse MSIX is registered via `Add-AppxPackage -Register -ExternalLocation`, which is per-user.
- Registry entries (fallback shell ext) live in `HKCU\Software\Classes\*\shell\MaeclyPrint`.
- No services, no scheduled tasks, no drivers.

Operation:

- All read/write to user-writable locations.
- No `runas` prompts during normal use.

The one exception is **uninstalling the WebView2 Runtime** — but we don't ship that; we use the system one and bootstrap it on Win 10 if missing (the bootstrapper is a Microsoft binary that handles its own elevation).

### 4.2 macOS

- App installs to `/Applications` (user can drag-drop without admin if not present).
- All state in `~/Library/Application Support/MaeclyPrint/` and friends.
- Finder Sync Extension requires the user to enable it in System Settings — explicit consent.

### 4.3 Enterprise policy (`policy.json`)

An organization can pin a small set of MaeclyPrint behavior with an optional, admin-managed `policy.json`. The app **reads it but never writes it** — it is intentionally an inbound-only control, so a compromised or misbehaving webview cannot alter organization settings. Today it carries a single field, the enforced default printer. Implemented in `src-tauri/src/settings/policy.rs`; surfaced to the UI via the `get_printer_policy` IPC command.

**Locations** (first hit wins — the machine-wide system path is checked before the per-user fallback):

| Platform | System path (admin-writable, preferred)                | Fallback (dev/testing)                     |
| -------- | ------------------------------------------------------ | ------------------------------------------ |
| Windows  | `%ProgramData%\MaeclyPrint\policy.json`                | `<user config dir>\MaeclyPrint\policy.json` |
| macOS    | `/Library/Application Support/MaeclyPrint/policy.json` | `~/Library/Application Support/MaeclyPrint/policy.json` |
| Linux    | `/etc/maeclyprint/policy.json`                         | `<XDG config dir>/maeclyprint/policy.json` |

The per-user config-dir fallback exists so developers and testers can exercise the locked state without machine-wide admin rights; in a real deployment the admin owns the system path.

**Schema** — every field optional, so an empty or partial file still deserializes:

```json
{ "default_printer": "<queue id>" }
```

**Read-only semantics.** When `default_printer` is set, the Printers screen shows a lock note and disables **Set default**; policy **wins over user preferences**, giving the effective-default precedence `policy.default_printer ?? prefs.default_printer ?? OS default`. Because the file is only ever read, the lock cannot be lifted from inside the app — only by an admin editing (or removing) the file.

**Failure behavior.** A missing file is not an error — policy is optional and absence means "unmanaged". A malformed file is **logged with a warning and ignored** (degrades to no-policy); it never blocks app startup. The loader is total: it always returns a value, so the UI can rely on a response.

Policy is read **at page load** (each time the Printers screen mounts). There is no daemon watching the file for live changes yet — a change takes effect on the next load/rescan. A background watch is a documented follow-up.

## 5. Network egress audit

By default, MaeclyPrint makes exactly **one kind of network request**:

```
GET https://updates.maecly.com/maeclyprint/<channel>/<platform>.json
GET https://updates.maecly.com/maeclyprint/<channel>/<platform>.json.sig
GET https://updates.maecly.com/maeclyprint/<channel>/binaries/<file>
GET https://updates.maecly.com/maeclyprint/<channel>/binaries/<file>.sig
```

That is everything. We:

- Do not phone home with usage stats.
- Do not load fonts or scripts from CDNs.
- Do not check for updates of converters.
- Do not communicate with printer manufacturers' clouds.

If the user opts in (Settings → Privacy):

- **Crash reports** (M3+) — sent to Sentry-compatible endpoint at `https://crash.maecly.com/`. Stack traces only; no file paths or content.
- **Anonymous usage metrics** (post-1.0) — daily ping with version, OS, locale, install age. No identifiers.

Both are off by default and require an explicit toggle.

## 6. Privacy

### 6.1 What we never collect

- File contents (ever).
- File names (in default logging level).
- Printer names with brand/model identifiers (in opt-in metrics).
- IP addresses (Sentry endpoint sees them but is configured to scrub).
- User identifiers, hostname, MAC address.

### 6.2 Logs

Local-only. Live under `%LOCALAPPDATA%\MaeclyPrint\logs\` (Windows) or `~/Library/Logs/MaeclyPrint/` (macOS). The user can delete them at any time. They are **not** uploaded automatically — even with crash reporting on, the report is a stack trace, not the log file. A separate "Send logs to maintainers" button exists in the diagnostics screen which uploads to a one-shot URL after explicit confirmation.

Default log level is `info`. At `info`:

- Job IDs, profile IDs, printer names, exit codes, durations.
- File extensions (e.g., "converted .docx → PDF in 1.4 s") but **not** filenames.

At `debug`:

- File paths included.
- Converter stderr tails included.

At `trace`:

- IPC payloads (still scrubbed for any `password`/`token` fields).

### 6.3 Telemetry consent

Settings → Privacy panel:

```
[ ] Send anonymized crash reports to MaeclyPrint maintainers
    (off by default; you can change this any time)

[ ] Send anonymous usage metrics
    (off by default; you can read what is sent)

What you'll never send:
  - File contents or filenames
  - Personal identifiers
  - Anything outside this app

[ Read the full privacy notice ]
```

## 7. Dependency hygiene

- `Cargo.lock` and `pnpm-lock.yaml` are committed. CI installs are lockfile-only.
- `cargo deny check` runs in CI: blocks unmaintained crates, GPL-incompatible licenses, advisories.
- `cargo audit` runs nightly against RustSec.
- `pnpm audit --audit-level high` runs in CI.
- Renovate bot opens PRs for dep bumps, with grouping (e.g., all `@radix-ui/*` together).
- Dependabot security updates are auto-merged once CI is green; non-security bumps require human review.

## 8. Vulnerability disclosure

A `SECURITY.md` at the repo root documents:

- The contact email: `security@maecly.com` (PGP key fingerprint published).
- A 90-day coordinated disclosure window by default.
- Acknowledgement targets: 72 h.
- A hall-of-fame credit list for reporters.
- Scope: this repo and the published binaries. Out of scope: `updates.maecly.com` infrastructure (report directly), the SumatraPDF / LibreOffice / Pandoc projects (report upstream).

Critical issues are patched out-of-cycle and pushed to the `stable` channel within 48 h.

## 9. Build reproducibility

- Rust release builds use `--locked` and pinned toolchain (`rust-toolchain.toml`).
- Frontend builds use `pnpm install --frozen-lockfile`.
- Build environment is documented (Windows runner image, Node version, pnpm version, Rust version).
- We do **not** claim bit-for-bit reproducibility yet — Tauri bundling has non-determinism we have not yet eliminated. Tracked as a future goal.

## 10. Hardening checklist (per release)

Before tagging a release, the maintainer verifies:

- [ ] `cargo audit` clean.
- [ ] `cargo deny check` clean.
- [ ] `pnpm audit --audit-level high` clean.
- [ ] All artifacts signed (Authenticode / Apple Developer ID / minisign).
- [ ] Manifest signed and uploaded to S3.
- [ ] CSP unchanged (or change reviewed).
- [ ] Capability allowlist unchanged (or change reviewed).
- [ ] No new outbound URLs added without privacy-doc update.
- [ ] No new logging that could include user content without `--debug`.

## 11. Related documents

- [`auto-updater.md`](./auto-updater.md) — update integrity in detail.
- [`shell-extensions.md`](./shell-extensions.md) — the DLL boundary.
- [`ipc-and-cli.md`](./ipc-and-cli.md) — capability allowlist.
- [`data-model.md`](./data-model.md) — what we persist and where.
- ADR [`0005-shell-extension-sparse-msix`](../adr/0005-shell-extension-sparse-msix.md) — the install model that keeps us out of admin.
