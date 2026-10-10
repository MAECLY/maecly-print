# Security Model

> **TL;DR** — MaeclyPrint signs its release artifacts, runs without admin privileges, isolates the high-risk shell-extension code in a tiny Rust COM DLL, keeps the webview's Rust-facing surface closed, and ships with telemetry **off** by default (and, in this version, inert). The only outbound network destination in the shipped app is **`api.github.com`**, for a read-only update *check* — there is no in-app updater and no `updates.maecly.com`.

## 1. Threat model

We start by being explicit about what we are defending against and what is out of scope.

### 1.1 In scope

| Threat                                                         | What we do about it                                                  |
| -------------------------------------------------------------- | -------------------------------------------------------------------- |
| Malicious update binary served by a compromised host           | **Structurally out of reach today**: the app never downloads or executes an update. The update path fetches release *metadata* only and hands the user a link; installation is a manual, signature-checked OS operation. |
| Modified installer downloaded from a tampered mirror           | Authenticode (Win EV cert) and Apple Developer ID notarization       |
| Hostile response from `api.github.com` (or a MITM on it)       | The response is parsed into at most three strings (tag, URL, notes) and never executed. A `200` that does not parse is an error, not "up to date". The link the user is offered is re-checked against the HTTPS allow-list before `open_external` will open it. |
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
- The shell-extension DLL: `shell_ext_win.dll` (the `cdylib` produced by the `maeclyprint-shellext` crate).
- The Windows installer produced by `tauri build` (NSIS, `installMode: currentUser`).
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

See [`auto-updater.md`](./auto-updater.md) for the full flow. The security-relevant summary for the **shipped** build:

- **The in-app updater installs only signed artifacts, and only on request.** `tauri-plugin-updater` downloads the artifact named by `latest.json` and verifies its minisign signature against the public key compiled in from `tauri.conf.json` (`plugins.updater.pubkey`) before a byte reaches the disk. A tampered artifact fails with `UpdateSignatureInvalid`. Install runs only after an explicit user action (About → Install now, `update install`), a major version needs a second confirmation, and release builds refuse non-`https` endpoints. The private key exists only offline and in the release secret `TAURI_SIGNING_PRIVATE_KEY` ([ADR-0008](../adr/0008-installer-nsis-per-user.md)).
- **Residual risk.** The manifest itself is not signed. Without `requireSignedVersion` (off until the Tauri CLI records the version in signatures), whoever can serve `latest.json` over the GitHub TLS endpoint could pair a newer version number with an older, genuinely signed artifact, which is a downgrade, not code execution. A leaked private key is the critical case: rotate it and ship the new public key in a release signed with the old one.
- The update *check* is a read-only `GET` to `api.github.com` for release metadata. The only thing it can influence is a version string, a URL and a markdown blob shown to the user.
- The URL is not opened automatically. Opening it goes through `open_external`, which enforces HTTPS and an exact-host allow-list (`github.com`, `maecly.com`, `*.maecly.com`; no userinfo, no port, no lookalike suffixes).
- Integrity of the artifact the user then installs rests on the **installer signature** — Authenticode on Windows, Developer ID + notarization on macOS (§2.1/§2.2) — verified by the OS, not by us.

Artifacts are minisign-signed; the manifest is not (see the residual risk above). Key rotation: generate a new pair, ship a release signed with the **old** key that carries the **new** public key, then sign everything after it with the new key.

## 3. Sandboxing and process boundaries

### 3.1 Shell-extension DLL

The DLL runs **in the address space of `explorer.exe`**, which is the most dangerous place possible for code: a crash brings down Explorer; a vulnerability exposes the entire user session.

It is written in **Rust**, not C++: the crate `maeclyprint-shellext` in `shell-ext-win/`, built as a `cdylib` implementing `IExplorerCommand` via the `windows` crate's `implement` feature.

Mitigations:

- **Memory safety by construction.** The COM boundary is the only `unsafe` surface; the path handling above it is safe Rust.
- **Tiny code surface.** ~1,800 lines across fourteen small modules, with unit tests co-located and two integration tests (`handoff_roundtrip`, `strings_cache`). Auditable in an afternoon.
- **Panics never cross the COM boundary.** Every entry point is wrapped by `guard::guard()`, which `catch_unwind`s and returns an HRESULT. This is why the crate pins `panic = "unwind"` in **both** its `dev` and `release` profiles, and why it is deliberately *not* a workspace member: `src-tauri`'s `panic = "abort"` would silently turn `catch_unwind` into a no-op and a panic would take Explorer down.
- **Bounded file I/O only.** The DLL reads paths from `IShellItemArray` and writes one handoff JSON file under the **per-user data directory** (`<data dir>/handoff`), never a world-writable temp dir — so another user cannot plant a payload. It never opens, reads, or modifies the user's documents. The consumer independently refuses any handoff path that does not canonicalize inside that directory, and any payload that is stale, empty, over-long or of the wrong schema version.
- **The handoff directory is derived, not hardcoded.** Both sides resolve it through the same `directories::ProjectDirs::from("com", "Maecly", "MaeclyPrint")` call, which is what guarantees producer and consumer agree on the path.
- **No networking.** The crate has no HTTP dependency; its whole dependency set is `serde`/`serde_json`, `uuid`, `time`, `directories`, and the Windows bindings.
- **Hardened release profile:** `lto = true`, `codegen-units = 1`, `opt-level = "s"`, `strip = true`.

### 3.2 Tauri main process

Runs as the user; not elevated. Has read/write access to:

- `%APPDATA%\Maecly\MaeclyPrint\` and `%LOCALAPPDATA%\Maecly\MaeclyPrint\`.
- The user's documents (because we are spawned by the user; no extra access).

Does **not**:

- Bind to any port.
- Modify `HKLM` or `%PROGRAMFILES%`.
- Write to any other user's files.
- Self-elevate.

### 3.3 Webview

The webview's Rust-facing surface is bounded two ways. **First, the closed command set:** the only commands that exist are those we register in `tauri_specta::collect_commands!` (`src-tauri/src/lib.rs`) — there is no dynamic or third-party command surface, so a compromised webview cannot reach a command we did not write. (Tauri 2's capability system gates *core and plugin* permissions — window, webview, dialog, shell — **not** app-defined `#[tauri::command]`s; `capabilities/main.json` therefore lists only `core:*` defaults plus `dialog:default`. See [`ipc-and-cli.md`](./ipc-and-cli.md) § 2.5.) **Second, and the real backstop:** there is no attacker-controlled code in the webview to abuse that surface — a strict CSP loads only the bundled SPA and forbids remote content, so a hypothetical XSS has no remote script to run.

CSP for the main window, verbatim from `src-tauri/tauri.conf.json` (`app.security.csp`):

```
default-src 'self';
img-src 'self' data: asset: http://asset.localhost;
style-src 'self' 'unsafe-inline';
font-src 'self' data:;
connect-src 'self' ipc: http://ipc.localhost
```

Notes on what this does and does not say:

- `connect-src` allows **no external origin**. The update check is made by Rust, not by the webview, so the webview never needs to reach the network — and cannot.
- `style-src 'unsafe-inline'` is required by Tailwind's generated styles; `font-src … data:` covers the inlined bundled font.
- `asset:` / `http://asset.localhost` is Tauri's own protocol for serving local files to the webview.
- There is no explicit `script-src`, `object-src` or `frame-ancestors` directive; `default-src 'self'` covers scripts, and the remaining two are a hardening gap worth closing.

We never `loadUrl()` remote content. The webview only loads the bundled SPA.

### 3.4 Converter subprocesses

In this version the only conversion that runs is **image → PDF, and it runs in-process** in pure Rust (`engine/convert/image.rs`, the `image` + `lopdf` crates). No external converter is spawned, so the converter-CVE row in §1.1 is currently vacuous — a property worth keeping in mind when the Office/HTML/Markdown converters land.

Processes the engine *does* spawn today are the **printing** hops — `lp` on macOS/Linux, `SumatraPDF.exe` on Windows — via `std::process::Command`:

- As the current user, never elevated.
- Bounded by a hard timeout enforced with the `wait-timeout` crate, so a hung child cannot wedge the queue; a timeout is reported as a print failure (exit code 9 on the CLI).
- With stdout/stderr captured, so the spooler's own message reaches the user rather than being invented by us.
- On Windows, with `CREATE_NO_WINDOW`, so a GUI-subsystem launch never flashes a console.

The image decoder is hardened against decompression bombs independently of any subprocess boundary: `image::Limits` caps decoded dimensions, and the emitted page is clamped to the 14,400 pt (200 in) PDF page ceiling.

The converters (LibreOffice, Chromium-class browser, Pandoc, `sips`) are a far larger attack surface than `lp`. They all go through `engine::convert::process::run_bounded`:

- each gets a **dedicated scratch folder** in the OS temp directory. It holds LibreOffice's throwaway `-env:UserInstallation` profile and the browser's private `--user-data-dir`, and it is removed on every exit path. The browser profile is seeded so that Edge does not try an empty-password logon for the user, which on Windows counts as a failed logon toward account lockout (see [print-engine.md](print-engine.md));
- each runs under a **deadline and the job's cancel flag**, and on either the **whole process tree** is killed (process group on Unix, `taskkill /T` on Windows);
- LibreOffice works on a private copy of the input, so a hostile file name never reaches its command line. The browser keeps its sandbox (no `--no-sandbox`).

**Still a gap:** environment minimization. Converters inherit the app's environment.

## 4. Permissions

### 4.1 No admin required

Installation:

- The bundler is Tauri's **NSIS** target with `installMode: "currentUser"` (`tauri.conf.json`), i.e. a per-user install under `%LOCALAPPDATA%` — not Inno Setup.
- Sparse MSIX is registered via `Add-AppxPackage -Register -ExternalLocation`, which is per-user.
- Registry entries (fallback shell ext) live in `HKCU\Software\Classes\*\shell\MaeclyPrint`.
- No services, no scheduled tasks, no drivers.

Operation:

- All read/write to user-writable locations.
- No `runas` prompts during normal use.

The one exception is **uninstalling the WebView2 Runtime** — but we don't ship that; we use the system one and bootstrap it on Win 10 if missing (the bootstrapper is a Microsoft binary that handles its own elevation).

### 4.2 macOS

- App installs to `/Applications` (user can drag-drop without admin if not present).
- All state in `~/Library/Application Support/com.Maecly.MaeclyPrint/` and friends.
- Finder Sync Extension requires the user to enable it in System Settings — explicit consent.

### 4.3 Enterprise policy (`policy.json`)

An organization can pin a small set of MaeclyPrint behavior with an optional, admin-managed `policy.json`. The app **reads it but never writes it** — it is intentionally an inbound-only control, so a compromised or misbehaving webview cannot alter organization settings. Today it carries a single field, the enforced default printer. Implemented in `src-tauri/src/settings/policy.rs`; surfaced to the UI via the `get_printer_policy` IPC command.

**Sources** (first hit wins). The UI calls the result *managed by your organization*, so a source is only believed when a standard user could not have written it:

| Platform | Sources, most authoritative first | Fallback (debug builds only) |
| -------- | --------------------------------- | ---------------------------- |
| Windows  | Registry `HKLM\SOFTWARE\Policies\Maecly\MaeclyPrint`, value `DefaultPrinter` (REG_SZ — Group Policy friendly); then `%ProgramData%\MaeclyPrint\policy.json`, **only if the file's owner is `BUILTIN\Administrators` or `SYSTEM`** | `<user config dir>\policy.json` |
| macOS    | `/Library/Application Support/MaeclyPrint/policy.json` | `~/Library/Application Support/com.Maecly.MaeclyPrint/policy.json` |
| Linux    | `/etc/maeclyprint/policy.json` | `<XDG config dir>/maeclyprint/policy.json` |

`%ProgramData%` is not admin-only: its default ACL lets any user create a folder there and own it. A `policy.json` a standard user created is therefore owned by that user and **ignored** (logged as `policy_untrusted`); otherwise any user could impose a "managed" default printer on every other account on the machine. A user cannot assign a file's ownership to Administrators without privileges they do not have.

Two more rules close the gaps around that check:

- **The folder must be admin-owned too.** `%ProgramData%\MaeclyPrint` must itself be owned by Administrators or SYSTEM. A standard user who created that folder first can rename and replace files inside it, so an admin-owned file in a user-owned folder is still ignored. Create the folder as an administrator.
- **One handle for the check and the read.** The file is opened once. Its owner is read from the open handle (`GetSecurityInfo`), and the content is read from that same handle. If the path were checked and read separately, someone who controls the folder could show their own file during the read and put the admin's file back before the owner check.

On macOS and Linux the location is admin/root-only, so it is the main check. The open handle must also be a regular file that is not world-writable, in a folder that is not world-writable.

The per-user config-dir fallback lets developers and testers exercise the locked state without admin rights. It is compiled into **debug builds only**, so in a release build a user cannot fake a managed state for themselves either.

**Schema** — every field optional, so an empty or partial file still deserializes:

```json
{ "default_printer": "<queue id>" }
```

**Read-only semantics.** When `default_printer` is set, the Printers screen shows a lock note and disables **Set default**; policy **wins over user preferences**, giving the effective-default precedence `policy.default_printer ?? prefs.default_printer ?? OS default`. Because the file is only ever read, the lock cannot be lifted from inside the app — only by an admin editing (or removing) the file.

**Failure behavior.** A missing file is not an error — policy is optional and absence means "unmanaged". A malformed file is **logged with a warning and ignored** (degrades to no-policy); it never blocks app startup. The loader is total: it always returns a value, so the UI can rely on a response.

Policy is read **at page load** (each time the Printers screen mounts). There is no daemon watching the file for live changes yet — a change takes effect on the next load/rescan. A background watch is a documented follow-up.

## 5. Network egress audit

MaeclyPrint makes exactly **two** possible requests, both to GitHub's public API, both unauthenticated `GET`s, and the second only on one branch:

```
GET https://api.github.com/repos/<owner>/<repo>/releases/latest
GET https://api.github.com/repos/<owner>/<repo>            # only if the first returned 404
```

`<owner>/<repo>` is compiled in from the crate's `repository` field; it is not configurable at runtime. Headers are `User-Agent: MaeclyPrint/<version>`, `Accept: application/vnd.github+json`, `X-GitHub-Api-Version: 2022-11-28`. The request bodies are empty and carry no identifier of any kind. The client is `ureq` with a 10-second timeout; a system proxy is honoured from `HTTPS_PROXY`/`HTTP_PROXY` (either case) when set.

Frequency: at most once per 24 h automatically (30 s after launch, off the main thread), plus whenever the user explicitly asks. Disabling `[updater].check_on_startup` removes the automatic one entirely — and the check is skipped *before* any socket is opened, which is asserted by test.

That is everything. The app:

- Does not phone home with usage stats. **This version has no telemetry transmitter at all** — the two consent flags in `[telemetry]` are recorded and never read by any sending code (documented on `TelemetryCfg` itself).
- Does not download or install updates.
- Does not load fonts, scripts or styles from CDNs — the CSP forbids it and the one webfont is bundled.
- Does not check for updates of converters.
- Does not communicate with printer manufacturers' clouds.
- Does not contact any Maecly-operated host. Links to `maecly.com`/`github.com` are opened in the **user's browser**, on an explicit click, through the `open_external` allow-list.

There is no `crash.maecly.com` and no metrics endpoint. If either is ever built, this section and [`../09-legal/privacy.md`](../09-legal/privacy.md) must change in the same commit.

## 6. Privacy

### 6.1 What we never collect

Nothing is collected, because nothing is transmitted. In particular the app never sends file contents, file names, printer names, or any user/host identifier — there is no code path that sends anything at all.

### 6.2 Logs

Local-only. Files are named `maeclyprint-<UTC date>.jsonl` (and `maeclyprint-<date>.<n>.jsonl` for each size roll within one day) under `%LOCALAPPDATA%\Maecly\MaeclyPrint\data\logs\` (Windows) or `~/Library/Logs/MaeclyPrint/` (macOS). The user can delete them at any time, and the About screen has a button that reveals the folder.

Rotation and retention are implemented by `logging::RollingJsonl`, not by `tracing-appender`'s rolling appender (which only rotates on a clock boundary and never deletes): the current file rolls when the **UTC date changes or it passes 10 MB, whichever comes first**, and a sweep at startup and on every roll deletes anything older than **14 days** and then keeps only the **7 newest** files. Both bounds apply; whichever bites first wins. There is no `[logging]` section in `config.toml` — these are the whole policy.

Logs are **never** uploaded. There is no "send logs to maintainers" upload path in this version; `report_issue` opens a prefilled GitHub issue in the browser with an environment block, and the user pastes whatever they choose.

Default log level is `info` (`EnvFilter` default `info,maeclyprint=debug`, overridable with `RUST_LOG`). At `info`:

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

**Both switches are inert in this version.** They persist consent (`[telemetry].crash_reports`, `[telemetry].anonymous_usage_metrics`) so a future build can honour a choice already made; no code reads them to decide whether to send, because no code sends. Turning them on changes nothing about the app's network behaviour.

## 7. Dependency hygiene

- `Cargo.lock` and `pnpm-lock.yaml` are committed. CI installs are lockfile-only.
- `cargo deny check` runs in CI: blocks unmaintained crates, GPL-incompatible licenses, advisories.
- `cargo audit` runs nightly against RustSec.
- `pnpm audit --audit-level high` runs in CI.
- Dependabot security updates are auto-merged once CI is green; non-security bumps require human review.
- `tauri-specta` / `specta` are pinned with `=` because they are pre-release; a wildcard would let a breaking rc land silently.
- The `objc2*` crates are version-constrained to the ones tao/wry already pull in. Taking a different major would build a **second** copy of the objc2 tree into the same binary — two incompatible views of the Objective-C runtime — which is a correctness and safety hazard, not a preference.

## 8. Vulnerability disclosure

A `SECURITY.md` at the repo root documents:

- The contact email: `security@maecly.com` (PGP key fingerprint published).
- A 90-day coordinated disclosure window by default.
- Acknowledgement targets: 72 h.
- A hall-of-fame credit list for reporters.
- Scope: this repo and the published binaries. Out of scope: the SumatraPDF / LibreOffice / Pandoc projects (report upstream) and GitHub's own infrastructure.

Critical issues are patched out-of-cycle and pushed to the `stable` channel within 48 h.

## 9. Build reproducibility

- Rust release builds use `--locked`. The minimum toolchain is pinned as `rust-version = "1.78"` in both `Cargo.toml`s; there is **no** `rust-toolchain.toml` in the repo today, so CI's Rust version is whatever the workflow installs — pinning it is an open hardening item.
- Frontend builds use `pnpm install --frozen-lockfile`.
- Build environment is documented (Windows runner image, Node version, pnpm version, Rust version).
- We do **not** claim bit-for-bit reproducibility yet — Tauri bundling has non-determinism we have not yet eliminated. Tracked as a future goal.

## 10. Hardening checklist (per release)

Before tagging a release, the maintainer verifies:

- [ ] `cargo audit` clean.
- [ ] `cargo deny check` clean.
- [ ] `pnpm audit --audit-level high` clean.
- [ ] All artifacts signed (Authenticode / Apple Developer ID).
- [ ] The bundled SumatraPDF matches the pinned version **and checksum** in `scripts/fetch-sumatrapdf.*`, and its upstream `LICENSE` was copied next to the executable (GPLv3 §6 obligation — see [`../09-legal/third-party-licenses.md`](../09-legal/third-party-licenses.md)).
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
