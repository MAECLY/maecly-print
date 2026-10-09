# CI/CD

> **TL;DR.** GitHub Actions runs **two** workflows today: `ci.yml` (every PR and push to `main` — lint, test, coverage gates, unsigned build, GUI smoke, shell-extension lanes) and `dco.yml` (sign-off check). There is **no release workflow and no nightly workflow**: nothing in this repository signs, notarizes, publishes or tags a release. That pipeline is designed but unbuilt, and this document separates the two halves explicitly.

This document specifies what CI does today, then records the release pipeline design that has not been implemented, so the gap is visible rather than hidden.

## Workflows that exist

| Workflow | File | Trigger | Signs? | Publishes? |
|---|---|---|---|---|
| **CI** | `.github/workflows/ci.yml` | `pull_request` → `main`, `push` → `main`, `workflow_dispatch`, weekly `schedule` | No | No (artifacts kept 7 days for debugging) |
| **DCO** | `.github/workflows/dco.yml` | Pull requests | — | — |

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
4. **Bindings drift gate**: `git diff --exit-code -- src/ipc/bindings.ts`

Step 3 is itself the primary drift gate — `tests/ipc_surface.rs` regenerates `src/ipc/bindings.ts` from the tauri-specta command surface and fails when the committed copy is stale. Step 4 is belt-and-braces for the one case the test cannot see: a filtered run that never regenerated anything. Its failure message tells you exactly what to do (`UPDATE_BINDINGS=1 cargo test …` and commit the result).

### `macos` — macos-latest, blocking

The lane that covers what Windows CI structurally cannot: the CUPS backend (`platform/cups.rs`, `engine/print/cups.rs`) compiles and its tests run against the runner's real CUPS.

clippy → `cargo test` → the same bindings gate → `pnpm test`. Then two **advisory** steps (`continue-on-error: true`):

- `pnpm tauri build --debug`
- **Smoke launch (macOS)**: run `src-tauri/target/debug/maeclyprint`, poll `~/Library/Logs/MaeclyPrint/maeclyprint.jsonl*` for `app_started`, 60 s deadline, then kill it.

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

`-- --ignored` runs *only* the ignored tests; everything else already ran in the blocking lanes. The tests in question hit `api.github.com` and drive real `lpadmin`/CUPS — rate limits and third-party downtime must never fail a merge. See [testing-strategy.md](../08-development/testing-strategy.md) § *Ignored tests*.

### `build` — windows-latest, blocking, `needs: [frontend, backend]`

`pnpm tauri build --debug`, then **Smoke launch (Windows)**, which is the strictest GUI assertion in the pipeline and *does* gate:

- Sandboxes `APPDATA` and `LOCALAPPDATA` into `$RUNNER_TEMP/mp-smoke` so a fresh log directory is polled.
- Starts the binary and polls `maeclyprint.jsonl*` for **both** `app_started` and `main_window_resolved`, 60 s deadline.
- Throws if either never appears; the process is killed in a `finally` block either way.
- On failure uploads `windows-smoke-log`.

This catches missing runtime dependencies and startup panics that a compile-only build would sail past.

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

Steps: `cargo fmt --check` → `cargo clippy --target <t> --all-targets -- -D warnings` → release build with **`MAECLYPRINT_REQUIRE_RESOURCES=1`** → `cargo test` (x64 only) → **COM export check** → artifact upload (`if-no-files-found: error`).

Two of those deserve explanation:

- `MAECLYPRINT_REQUIRE_RESOURCES=1` turns a missing `rc.exe` — and therefore a DLL with no embedded `<msix>` manifest — into a build failure. Such a DLL builds fine, registers fine, and is never loaded by Explorer. That is the most expensive way for this component to go wrong, and it is invisible without this flag.
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
| `.github/workflows/release.yml` | Not written. No tag-triggered pipeline; a `v*` tag currently does nothing. |
| `.github/workflows/nightly.yml` | Not written. There are no nightly builds and no nightly channel. |
| Code signing of any kind | No signing step exists anywhere. No EV certificate, no Apple Developer ID, no notarization, no stapling. Artifacts from `ci.yml` are unsigned debug builds. |
| Updater bundle signing in CI | The app side is done: `tauri.conf.json` carries `plugins.updater.pubkey` and `createUpdaterArtifacts`, and the app verifies and installs signed updates (see [Updater signing](#updater-signing)). No workflow signs or publishes them yet. |
| Artifact publication (Cloudflare R2, channel manifests) | No upload step, no manifest generator, no bucket. |
| GitHub Release creation | Not automated. |
| `installers/windows/build.ps1`, `installers/macos/*.sh` | The `installers/` tree contains only `.gitkeep` files. |
| `scripts/sign-*.ps1`, `scripts/upload-to-r2.sh`, `scripts/update-manifest.sh`, `scripts/build-mac-universal.sh`, `scripts/setup-apple-certs.sh`, `scripts/verify-windows-signatures.ps1` | None exist. `scripts/` holds `fetch-sumatrapdf.ps1` and `fetch-sumatrapdf.sh` and nothing else. |
| Universal macOS binary / arch matrix | `ci.yml` builds only the runner's native architecture. There is no `aarch64-apple-darwin` × `x86_64-apple-darwin` matrix and no `lipo` step. The only cross-architecture matrix in the repo is `shell-ext-win`'s x64/ARM64 pair. |
| `cargo audit` / `pnpm audit` (NFR-065) | No scheduled dependency-audit workflow. Dependabot (`.github/dependabot.yml`) is the only dependency automation. |
| `gitleaks` secret scanning (NFR-027) | Not configured. |
| Reproducible-build verification (NFR-064) | Not automated. |

### Updater signing

Releases are signed so the installed app can verify an update before installing it (FR-082, [ADR-0008](../adr/0008-installer-nsis-per-user.md)). The key pair is minisign, generated once with `pnpm tauri signer generate`:

- **Public key** — committed, in `src-tauri/tauri.conf.json` → `plugins.updater.pubkey`. Every build embeds it and accepts only artifacts signed by the matching private key.
- **Private key** — **never committed**. A release build reads it from the `TAURI_SIGNING_PRIVATE_KEY` secret (the key file's contents), plus `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` if the key has a password. With both set, `pnpm tauri build` writes a `.sig` next to every updater artifact: the NSIS setup on Windows, `MaeclyPrint.app.tar.gz` on macOS. Keep an offline backup. If the key is lost, installed copies cannot be updated in place. If it leaks, rotate it and ship the new public key through a release signed with the old one.
- **Manifest** — the release uploads `latest.json` as a release asset: `version`, `notes`, `pub_date`, and a `platforms` map (`windows-x86_64`, `darwin-aarch64`, `darwin-x86_64`) of `{ url, signature }`, where `signature` is the contents of the matching `.sig`. The app reads it from `https://github.com/MAECLY/maecly-print/releases/latest/download/latest.json`.

PR and `main` builds have no key and must not get one. With `createUpdaterArtifacts` on, Tauri refuses to bundle without a private key, so `ci.yml` builds with `--config src-tauri/tauri.ci.conf.json`, which turns updater artifacts off for that build only.

To test an update end to end without publishing, serve a `latest.json` and a signed artifact locally, then point a **debug** build at it with `MAECLYPRINT_UPDATE_ENDPOINT=http://127.0.0.1:<port>/latest.json`. Release builds only accept `https`. A copy of the artifact with one byte changed must fail with "failed its signature check and was NOT installed" (exit code 11 from `update install`).

`BUILD_DATE` (About, issue reports) is the build script's run date, re-stamped on every source change. A reproducible release pins it with `SOURCE_DATE_EPOCH`.

### Required secrets

**None** for `ci.yml`. A release pipeline needs `TAURI_SIGNING_PRIVATE_KEY` (and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` if the key has one), as described above. `ci.yml` reads no `secrets.*` value; every job runs on the default `GITHUB_TOKEN`. That is a direct consequence of the table above: nothing signs, uploads or publishes. The secret inventory a release pipeline would need (EV signing credentials, Apple certificates and notarization identifiers, the Ed25519 updater key, object-storage keys) is specified in [code-signing.md](./code-signing.md); it should be created when the pipeline that consumes it is written, not before.

### Design intent for the release pipeline

When `release.yml` is written, these constraints hold:

- **Tag-triggered** on `v*`, with the channel derived from the tag (`-beta.`/`-rc.` → beta, otherwise stable).
- **Manual approval gate.** The publishing job binds to a GitHub Environment (`production`) with required reviewers, so signed binaries never reach a CDN without a human confirming that builds succeeded, signatures verified and notarization passed. This is the primary defence against a compromised PR reaching users.
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
