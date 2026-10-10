# CI/CD

> **TL;DR.** `ci.yml` runs on every PR and push to `main` (lint, test, coverage gates, unsigned build, GUI smoke, shell-extension lanes, the third-party notices check, and on `main` the release-mode x64 and ARM64 installers). `release.yml` builds a signed release into a **draft** on every `v*` tag, and `release-channels.yml` moves the beta channel when a person publishes it: see [releasing.md](./releasing.md), the runbook, and [ADR-0011](../adr/0011-release-channel.md). Nothing is notarized or Authenticode-signed (decision D2), there is no nightly workflow, and no release has run yet: the two signing secrets are not set. There is no sign-off check: contributions come in under the [CLA](../../CLA.md), which the pull request template asks for.

This document specifies what CI does; [releasing.md](./releasing.md) covers the release workflows.

## Workflows that exist

| Workflow | File | Trigger | Signs? | Publishes? |
|---|---|---|---|---|
| **CI** | `.github/workflows/ci.yml` | `pull_request` → `main`, `push` → `main`, `workflow_dispatch`, weekly `schedule` | No | No (artifacts kept 7 days for debugging) |
| **release** | `.github/workflows/release.yml` | `push` of a `v*` tag; `workflow_dispatch` (build only) | Update artifacts (minisign); no OS code signing | A **draft** release, published by a person |
| **release-channels** | `.github/workflows/release-channels.yml` | `release: published` | No | Overwrites `latest.json` on the `channel-beta` prerelease, forward only |
| **Pages** | `.github/workflows/pages.yml` | `push` → `main` touching `site/` | No | The landing page |

`workflow_dispatch` and `schedule` exist on `ci.yml` for exactly one job: the advisory `ignored-tests` lane. Every other job carries `if: github.event_name != 'schedule'`, so a scheduled run executes that lane and nothing else.

```yaml
concurrency:
  group: ${{ github.workflow }}-${{ github.ref }}
  cancel-in-progress: true
```

If a new commit lands on a PR, the in-flight run for the previous commit is cancelled.

## `ci.yml` job by job

### `frontend` — ubuntu-latest, blocking

`pnpm install --frozen-lockfile`, then:

1. `pnpm lint` (ESLint, including `eslint-plugin-jsx-a11y`)
2. `pnpm format:check` (Prettier)
3. `pnpm typecheck` (`tsc --noEmit`)
4. `pnpm test:coverage` with the coverage gate passed on the command line:

```
--coverage.thresholds.lines=55
--coverage.thresholds.statements=55
--coverage.thresholds.functions=65
--coverage.thresholds.branches=75
```

The thresholds live in the workflow, not in `vite.config.ts`, so the gate is CI-owned and a local `pnpm test:coverage` never fails on a number. They sit just under the measured tree (lines/statements 61.4%, functions 71.4%, branches 79.8%) — a regression gate, raised as the coverage campaign lands tests. The report is uploaded as the `frontend-coverage` artifact regardless of outcome (`if: always()`, 7-day retention).

> Note that `e2e/tsconfig.json` is standalone: `pnpm typecheck` does **not** type-check the E2E specs. They are checked when `pnpm test:e2e` compiles them.

### `backend` — windows-latest, blocking

Cargo registry, git db and `src-tauri/target/` are cached on `hashFiles('src-tauri/Cargo.lock')`.

1. `cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check`
2. `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`
3. `cargo test --manifest-path src-tauri/Cargo.toml`
4. `cargo check --target aarch64-pc-windows-msvc --all-targets` — Windows on ARM is type-checked; an x64 runner cannot run its tests
5. **Bindings drift gate**: `git diff --exit-code -- src/ipc/bindings.ts`

Step 3 runs on Windows only because `src-tauri/build.rs` links the Common-Controls v6 manifest into **every** artifact (`/MANIFEST:EMBED /MANIFESTINPUT:windows-app-manifest.xml`), test harnesses included. With Tauri's default (a manifest resource linked into `[[bin]]` targets only) the lib's test binary died with `STATUS_ENTRYPOINT_NOT_FOUND` (0xc0000139) before running a test, and no `#[cfg(windows)]` test had ever run in CI.

`cargo test` is itself the primary drift gate — `tests/ipc_surface.rs` regenerates `src/ipc/bindings.ts` from the tauri-specta command surface and fails when the committed copy is stale. Step 5 is belt-and-braces for the one case the test cannot see: a filtered run that never regenerated anything. Its failure message tells you exactly what to do (`UPDATE_BINDINGS=1 cargo test …` and commit the result).

### `macos` — macos-latest, blocking

The lane that covers what Windows CI structurally cannot: the CUPS backend (`platform/cups.rs`, `engine/print/cups.rs`) compiles and its tests run against the runner's real CUPS.

clippy → `cargo test` → the same bindings gate → `pnpm test`. Then two **advisory** steps (`continue-on-error: true`):

- `pnpm tauri build --debug`
- **Smoke launch (macOS)**: run `src-tauri/target/debug/pluryprint`, poll `~/Library/Logs/Pluryprint/pluryprint-*.jsonl` (the name `logging/mod.rs` writes) for `app_started`, 60 s deadline, then kill it.

Both are best-effort because WKWebView has no WebDriver and a headless runner has no guaranteed window server.

### `rust-coverage` — macos-latest, blocking

`taiki-e/install-action@cargo-llvm-cov`, then:

```
cargo llvm-cov --manifest-path src-tauri/Cargo.toml \
  --lcov --output-path lcov.info \
  --fail-under-lines 70
```

macOS rather than Windows because the CUPS backend compiles and runs there, and because an ubuntu lane would have to install `webkit2gtk` for the Tauri crate. Measured on this lane at the time of writing: 80.4% lines, 82.4% regions, 67.8% functions; the 70% threshold matches the documented target with headroom so unrelated PRs are not blocked by noise. `lcov.info` is uploaded as the `rust-coverage` artifact.

### `ignored-tests` — macos-latest, advisory, scheduled only

`if: github.event_name == 'schedule' || github.event_name == 'workflow_dispatch'`, `continue-on-error: true`, weekly on Mondays at 05:00 UTC.

```
cargo test --manifest-path src-tauri/Cargo.toml -- --ignored
```

`-- --ignored` runs *only* the ignored tests; everything else already ran in the blocking lanes. The tests in question read the published update manifests on github.com and drive real `lpadmin`/CUPS — rate limits and third-party downtime must never fail a merge. See [testing-strategy.md](../08-development/testing-strategy.md) § *Ignored tests*.

### `build` — windows-latest, blocking, `needs: [frontend, backend]`

`scripts/fetch-sumatrapdf.ps1 -Arch x64` and `scripts/build-shell-ext.ps1 -Arch all` (the NSIS hooks refuse to build a CI installer without both DLLs), `pnpm tauri build --debug`, then **Smoke launch (Windows)**, which is the strictest GUI assertion in the pipeline and *does* gate:

- Starts the binary and polls the real log directory, `%LOCALAPPDATA%\Maecly\Pluryprint\data\logs\pluryprint-*.jsonl` (only files written after the launch), for **both** `app_started` and `main_window_resolved`, 60 s deadline. Overriding `APPDATA`/`LOCALAPPDATA` does not sandbox it — `directories` resolves known folders through `SHGetKnownFolderPath`, which ignores those variables — which is why an earlier version of this step could never pass.
- Throws if either never appears; the process is killed in a `finally` block either way.
- **Smoke CLI exit codes (Windows)**: `pluryprint-cli.exe --version` must exit 0 and `pluryprint-cli.exe frobnicate` must exit 2 as seen by the shell (FR-009).
- On failure uploads `windows-smoke-log`.

This catches missing runtime dependencies and startup panics that a compile-only build would sail past.

### `installers` — windows-latest, push to `main` and manual runs, matrix x64 / ARM64

The release-mode NSIS installers, built as `release.yml` builds them but unsigned (`tauri.ci.conf.json`): SumatraPDF for the installer's architecture, the Explorer DLL for both architectures under `resources/shellext/<arch>/`, then `pnpm tauri build --target <triple> --bundles nsis`. The ARM64 one is cross-compiled (NFR-070). The job writes the installer size to the summary against NFR-006's 15 MB (a warning here, an error in `release.yml`), checks the bundle carries SumatraPDF, its licence, its source offer and both DLLs, and uploads the installer as `pluryprint-installer-<arch>`. Not on PRs: two release builds per PR would cost more runner minutes than they save.

### `notices` — ubuntu-latest, blocking

Runs `scripts/licenses/generate-notices.sh --check` with cargo-about 0.9.2: `THIRD-PARTY-NOTICES.md` must equal what the lockfiles generate, so a dependency change cannot ship without its notices.

### `e2e` — windows-latest, non-blocking, `needs: [frontend, backend]`

`timeout-minutes: 35`, `continue-on-error: true` — driving a real WebView2 GUI through pre-1.0 `tauri-driver` is inherently flake-prone.

1. `cargo install tauri-driver --locked`
2. **Provision `msedgedriver`**: read the installed WebView2 Evergreen Runtime version from `HKLM:\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}` (`pv`), download `edgedriver_win64.zip` for exactly that version, and export the path as `NATIVE_DRIVER`. The driver must match the runtime; a mismatch is the single most common E2E failure.
3. `pnpm build` + `cargo build --manifest-path src-tauri/Cargo.toml` — the binary directly, rather than a full `tauri build` which would also produce installers the run does not need.
4. `pnpm test:e2e` (`wdio run e2e/wdio.conf.ts`, which spawns `tauri-driver --native-driver $NATIVE_DRIVER`).
5. Uploads `e2e-artifacts` (failure screenshots) with `if: always()`.

### `shell-ext-win` — windows-latest, blocking, matrix

The Explorer context-menu DLL needs its own lane: `shell-ext-win/` is deliberately **not** a Cargo workspace member, because its release profile must keep `panic = "unwind"` (so `catch_unwind` at the COM boundary actually contains a panic instead of taking Explorer down), which `src-tauri`'s `panic = "abort"` would override. `--manifest-path src-tauri/Cargo.toml` never touches it.

| Target | Tests run? |
|---|---|
| `x86_64-pc-windows-msvc` | yes |
| `aarch64-pc-windows-msvc` | no — build only; an x64 runner cannot execute an ARM64 test binary |

Steps: `cargo fmt --check` → `cargo clippy --target <t> --all-targets -- -D warnings` → release build with **`PLURYPRINT_REQUIRE_RESOURCES=1`** → `cargo test` (x64 only) → **COM export check** → artifact upload (`if-no-files-found: error`).

Two of those deserve explanation:

- `PLURYPRINT_REQUIRE_RESOURCES=1` turns a missing `rc.exe` — and therefore a DLL with no embedded `<msix>` manifest — into a build failure. Such a DLL builds fine, registers fine, and is never loaded by Explorer. That is the most expensive way for this component to go wrong, and it is invisible without this flag.
- The **COM export check** runs `dumpbin /EXPORTS` (located via `vswhere`) and asserts `DllGetClassObject` and `DllCanUnloadNow` are present. Without them Explorer cannot instantiate the handler at all, and nothing else in the lane would notice. `DllMain` is deliberately **not** on the list: it is an entry point, and whether it also appears in the export table is a toolchain detail. If `dumpbin` cannot be found the step warns and exits 0 rather than failing the lane over a runner-image change — the build above is still the gate.

### `shell-ext-portable` — ubuntu-latest, blocking

Guards the claim the crate is built on: everything except the COM layer is plain Rust, so the handoff schema and the menu model can be developed and tested on a machine that is not Windows.

- `cargo test --manifest-path shell-ext-win/Cargo.toml` (host)
- `cargo check --manifest-path shell-ext-win/Cargo.toml --target x86_64-pc-windows-msvc --all-targets` — type-checks the whole crate, COM layer included, against the real `windows` bindings. No linker is involved, so this works from any host.

If this lane breaks, a macOS or Linux contributor can no longer work on the extension at all.

## Caching

Every Rust lane uses `actions/cache@v4` directly (not `Swatinem/rust-cache`) over `~/.cargo/{bin,registry/index,registry/cache,git/db}` plus the relevant `target/` directory. Keys are `${{ runner.os }}-cargo-<purpose>-${{ hashFiles('<crate>/Cargo.lock') }}` with a `${{ runner.os }}-cargo-` restore prefix. Distinct purposes (`build`, `e2e`, `llvmcov`, `shellext-<target>`, `ignored`) keep lanes with different feature sets and profiles from thrashing one another's cache.

pnpm modules are cached by `actions/setup-node@v4` with `cache: pnpm`, keyed on the lockfile hash.

## What does **not** exist yet

Everything in this section is **unimplemented**. It is recorded as design intent. Do not cite it as a description of the repository.

| Missing | Status |
|---|---|
| `.github/workflows/nightly.yml` | Not written. There are no nightly builds and no nightly channel. |
| OS code signing | No Authenticode certificate, no Apple Developer ID, no notarization, no stapling (decision D2). macOS bundles are signed ad hoc. Update artifacts **are** signed, by `release.yml`. |
| A release that has run | `release.yml` exists but has never run: `TAURI_SIGNING_PRIVATE_KEY` is not set, and the repository is private until the first stable release. |
| Artifact publication outside GitHub (Cloudflare R2, own domain) | Not planned for now: releases and manifests live on GitHub Releases ([ADR-0011](../adr/0011-release-channel.md)). |
| `installers/windows/build.ps1`, `installers/macos/*.sh` | The `installers/` tree contains only `.gitkeep` files. |
| `scripts/sign-*.ps1`, `scripts/upload-to-r2.sh`, `scripts/update-manifest.sh`, `scripts/build-mac-universal.sh`, `scripts/setup-apple-certs.sh`, `scripts/verify-windows-signatures.ps1` | None exist. `scripts/` holds `fetch-sumatrapdf.ps1` and `fetch-sumatrapdf.sh` and nothing else. |
| Universal macOS binary | Not built: `release.yml` builds Apple silicon and Intel as separate downloads (Versorium's layout). `ci.yml` builds macOS only for the runner's architecture. |
| `cargo audit` / `pnpm audit` (NFR-065) | No scheduled dependency-audit workflow. Dependabot (`.github/dependabot.yml`) is the only dependency automation. |
| `gitleaks` secret scanning (NFR-027) | Not configured. |
| Reproducible-build verification (NFR-064) | Not automated. |

### Updater signing

Releases are signed so the installed app can verify an update before installing it (FR-082, [ADR-0008](../adr/0008-installer-nsis-per-user.md)). The key pair is minisign, generated once with `pnpm tauri signer generate`:

- **Public key** — committed, in `src-tauri/tauri.conf.json` → `plugins.updater.pubkey`. Every build embeds it and accepts only artifacts signed by the matching private key.
- **Private key** — **never committed**. A release build reads it from the `TAURI_SIGNING_PRIVATE_KEY` secret (the key file's contents), plus `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` if the key has a password. With both set, `pnpm tauri build` writes a `.sig` next to every updater artifact: the NSIS setup on Windows, `Pluryprint.app.tar.gz` on macOS. Keep an offline backup. If the key is lost, installed copies cannot be updated in place. If it leaks, rotate it and ship the new public key through a release signed with the old one.
- **The private key must have a password.** Tauri asks for the password whenever `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` is not set, and a passwordless key can only be used by setting that variable to an empty string. Windows PowerShell cannot do that: `$env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = ""` *deletes* the variable (verified with `Test-Path env:...` on 5.1), so `pnpm tauri build` finishes both bundles and then waits forever at the password prompt, with no `.sig` written. In a non-interactive job (a scheduled task, CI) that is a hang, not an error. Generate release keys with a non-empty password (`pnpm tauri signer generate -w <file>` asks for one), and run signing builds with `CI=true` and a job timeout so a missing password fails instead of hanging. If an existing key has no password, sign the artifact explicitly instead, from `cmd.exe`: `pnpm tauri signer sign -f <key file> -p "" <artifact>`. Windows PowerShell 5.1 drops empty arguments to native programs, so the same line typed there loses the `""`; `cmd.exe` passes it through.
- **Manifest** — the release uploads `latest.json` as a release asset: `version`, `notes`, `pub_date`, and a `platforms` map (`windows-x86_64`, `windows-aarch64`, `darwin-aarch64`, `darwin-x86_64`, plus the `-nsis`/`-app` variants) of `{ url, signature }`, where `signature` is the contents of the matching `.sig`. The stable channel reads it from `https://github.com/MAECLY/maecly-print/releases/latest/download/latest.json`, the beta channel from the `channel-beta` release (see [releasing.md](./releasing.md) §3).

PR and `main` builds have no key and must not get one. With `createUpdaterArtifacts` on, Tauri refuses to bundle without a private key, so `ci.yml` builds with `--config src-tauri/tauri.ci.conf.json`, which turns updater artifacts off for that build only.

To test an update end to end without publishing, serve a `latest.json` and a signed artifact locally, then point a **debug** build at it with `PLURYPRINT_UPDATE_ENDPOINT=http://127.0.0.1:<port>/latest.json`. Release builds only accept `https`. The variable also redirects `update check`, the About screen's check and the launch-time check, which read that manifest's `version` and `notes` exactly as they read the channel's published manifest (`update::fetch_configured_latest`), so the whole flow can be exercised against one test server. A copy of the artifact with one byte changed must fail with "failed its signature check and was NOT installed" (exit code 11 from `update install`).

`BUILD_DATE` (About, issue reports) is the build script's run date, re-stamped on every source change. A reproducible release pins it with `SOURCE_DATE_EPOCH`.

### Required secrets

**None** for `ci.yml`, which reads no `secrets.*` value. `release.yml` needs `TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` (release keys must have a password; see above), and its `guard` job fails a tag build when the key is not set. The full list of secrets and repository settings is in [releasing.md](./releasing.md) §2.

### Constraints the release pipeline keeps

- **Tag-triggered** on `v*`, with the channel derived from the tag (any `-` suffix → prerelease, beta channel; otherwise stable).
- **Manual approval gate.** The workflow only ever builds a **draft**; a person publishes it after the checks in [releasing.md](./releasing.md) §6. On the free plan with a private repository, Environments with required reviewers are not available; the draft is the gate.
- **No signing in CI.** PR and `main` builds must never have access to signing material; forks and contributors cannot accidentally sign.
- **`scripts/fetch-sumatrapdf.ps1` runs before any Windows packaging step.** `SumatraPDF.exe` is not committed (see `src-tauri/resources/sumatrapdf/README.md`), so a packaging job that skips this produces an installer whose every print job fails with `DependencyMissing`.
- **No self-hosted runners.** GitHub-hosted runners are sufficient at our scale, and a self-hosted runner with access to signing secrets is a supply-chain liability. If cost becomes a concern, narrow the PR matrix rather than moving off hosted runners.

## Artifact retention

| Artifact | Job | Retention |
|---|---|---|
| `frontend-coverage` | `frontend` | 7 days |
| `rust-coverage` (`lcov.info`) | `rust-coverage` | 7 days |
| `windows-smoke-log` (on failure) | `build` | 7 days |
| `e2e-artifacts` (screenshots) | `e2e` | 7 days |
| `shell-ext-win-<target>` (the DLL) | `shell-ext-win` | 7 days |

Long-term artifact retention is a release-pipeline concern and does not apply yet.

## Status checks required for merge

Branch protection is a GitHub repository setting, not something this file can assert. The jobs that are *designed* to gate are: `frontend`, `backend`, `macos`, `rust-coverage`, `build`, `shell-ext-win` (both matrix entries) and `shell-ext-portable`. `e2e` and `ignored-tests` are advisory by construction (`continue-on-error: true`) and must not be added as required checks.

## See also

- [installer-windows.md](./installer-windows.md)
- [installer-macos.md](./installer-macos.md)
- [code-signing.md](./code-signing.md)
- [auto-update-flow.md](./auto-update-flow.md)
- [../06-roadmap/release-strategy.md](../06-roadmap/release-strategy.md)
- [../08-development/release-checklist.md](../08-development/release-checklist.md)
- [../08-development/testing-strategy.md](../08-development/testing-strategy.md)
