# CI/CD

> **TL;DR.** `ci.yml` is the lean pull-request gate: frontend lint, typecheck, format and tests on ubuntu, and Rust fmt, clippy and the full test suite on Windows and macOS. It builds no bundle and reads no secret. `release.yml` builds a signed release into a **draft** on every `v*` tag (after checking the signing secret, the versions and `THIRD-PARTY-NOTICES.md`), and `release-channels.yml` moves the beta channel when a person publishes it: see [releasing.md](./releasing.md), the runbook, and [ADR-0011](../adr/0011-release-channel.md). Nothing is notarized or Authenticode-signed (decision D2), there is no nightly workflow, and no release has run yet. There is no sign-off check: contributions come in under the [CLA](../../CLA.md), which the pull request template asks for.

This document specifies what CI does; [releasing.md](./releasing.md) covers the release workflows.

## Workflows that exist

| Workflow | File | Trigger | Signs? | Publishes? |
|---|---|---|---|---|
| **CI** | `.github/workflows/ci.yml` | `pull_request`, `push` → `main`, `workflow_dispatch` | No | No |
| **release** | `.github/workflows/release.yml` | `push` of a `v*` tag; `workflow_dispatch` (build only) | Update artifacts (minisign); no OS code signing | A **draft** release, published by a person |
| **release-channels** | `.github/workflows/release-channels.yml` | `release: published` | No | Overwrites `latest.json` on the `channel-beta` prerelease, forward only |
| **Pages** | `.github/workflows/pages.yml` | `push` → `main` touching `site/` | No | The landing page |

Only `ci.yml` runs on a pull request. There is no scheduled workflow.

```yaml
concurrency:
  group: ${{ github.workflow }}-${{ github.ref }}
  cancel-in-progress: true
```

If a new commit lands on a PR, the in-flight run for the previous commit is cancelled.

## `ci.yml` job by job

Two jobs, both blocking, and nothing else. `src-tauri/tests/release_pipeline.rs` (`ci_is_the_lean_pull_request_gate`) pins this shape, so a lane added back by accident fails the Rust tests.

### `frontend` — ubuntu-latest

`pnpm install --frozen-lockfile --ignore-scripts`, then:

1. `pnpm lint` (ESLint, including `eslint-plugin-jsx-a11y`)
2. `pnpm typecheck` (`tsc --noEmit`)
3. `pnpm format:check` (Prettier)
4. `pnpm test` (Vitest)

> Note that `e2e/tsconfig.json` is standalone: `pnpm typecheck` does **not** type-check the E2E specs. They are checked when `pnpm test:e2e` compiles them, which CI does not run (see [testing-strategy.md](../08-development/testing-strategy.md)).

### `backend` — matrix `windows-latest` + `macos-latest`

The two platforms Pluryprint ships for. The Rust code is heavily `cfg`-gated — WinSpool, SumatraPDF and the Explorer handoff on Windows, the CUPS backend (`platform/cups.rs`, `engine/print/cups.rs`) on macOS — so each half only compiles, and its tests only run, on its own OS. `fail-fast: false` keeps one platform's failure from hiding the other's. `PLURYPRINT_SKIP_MIGRATION=1` is set for the job, so a test run never moves a MaeclyPrint-era data directory ([ADR-0009](../adr/0009-rename-to-pluryprint.md)).

1. `cargo fmt --check` for `src-tauri` and `shell-ext-win` — Windows only; formatting does not depend on the platform
2. `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`
3. `cargo test --manifest-path src-tauri/Cargo.toml` — the full suite, never filtered
4. Windows only: `cargo clippy` and `cargo test` for `shell-ext-win/`

Step 3 is also the **bindings drift gate**: `tests/ipc_surface.rs` regenerates `src/ipc/bindings.ts` from the tauri-specta command surface and fails when the committed copy is stale. Its failure message says what to do (`UPDATE_BINDINGS=1 cargo test …` and commit the result). It only runs in a full test run, which is why the step takes no filter.

On Windows, `src-tauri/build.rs` links the Common-Controls v6 manifest into **every** artifact (`/MANIFEST:EMBED /MANIFESTINPUT:windows-app-manifest.xml`), test harnesses included. With Tauri's default (a manifest resource linked into `[[bin]]` targets only) the lib's test binary died with `STATUS_ENTRYPOINT_NOT_FOUND` (0xc0000139) before running a test.

The Explorer context-menu DLL is its own crate: `shell-ext-win/` is deliberately **not** a Cargo workspace member, because its release profile must keep `panic = "unwind"` (so `catch_unwind` at the COM boundary contains a panic instead of taking Explorer down), which `src-tauri`'s `panic = "abort"` would override. `--manifest-path src-tauri/Cargo.toml` never touches it, hence step 4. Its COM layer only builds for Windows. No `src-tauri` test needs the built DLL: the installer-hook and bundle tests read the NSIS script and use a placeholder file.

### What CI does not run, and where it went

| Check | Where it lives now |
|---|---|
| Installers, the NFR-006 size budget, both Explorer DLLs in each bundle | `release.yml` `build` (every Windows target) |
| COM exports of the shipped DLLs (`DllGetClassObject`, `DllCanUnloadNow`) | `release.yml` `build`, step *Explorer DLLs export the COM entry points* |
| `THIRD-PARTY-NOTICES.md` is current (`scripts/licenses/generate-notices.sh --check`) | `release.yml` `guard`, so no release is built from stale notices; run it by hand after a dependency change |
| Coverage (Vitest V8, `cargo llvm-cov`) | Local only: `pnpm test:coverage`, `cargo llvm-cov` |
| `#[ignore]`d tests (live update manifest, real CUPS queue) | Local only: `cargo test -- --ignored` |
| E2E (WebdriverIO + tauri-driver) and smoke launches | Local only: `pnpm test:e2e` on Windows; the VM scripts under `scripts/vm/` |
| Windows on ARM | `release.yml` cross-compiles the ARM64 app and DLL on every tag |

The COM export check that an earlier `shell-ext-win` CI lane ran was itself broken: it tested `$exports -notmatch $symbol` with `$exports` being `dumpbin`'s output as an **array** of lines, and on an array PowerShell's `-notmatch` returns the non-matching lines instead of a yes or no, so it threw on every DLL. The DLL was fine: `lib.rs` exports both functions with `#[no_mangle] pub unsafe extern "system"`, and the linker's `LNK4104 ... should be PRIVATE` warning in the same log names them in the export table. The release check joins the output into one string first.

## Caching

The `backend` job uses `actions/cache@v6` directly (not `Swatinem/rust-cache`) over `~/.cargo/{registry/index,registry/cache,git/db}` plus `src-tauri/target/` and `shell-ext-win/target/`, keyed on `${{ runner.os }}-cargo-` and both lockfiles. pnpm modules are cached by `actions/setup-node@v7` with `cache: pnpm`. The release jobs use no cache: their `setup-node` steps set `package-manager-cache: false`.

## What does **not** exist yet

Everything in this section is **unimplemented**. It is recorded as design intent. Do not cite it as a description of the repository.

| Missing | Status |
|---|---|
| `.github/workflows/nightly.yml` | Not written. There are no nightly builds: the app's **nightly** channel setting follows the beta manifest (FR-085). |
| OS code signing | No Authenticode certificate, no Apple Developer ID, no notarization, no stapling (decision D2). macOS bundles are signed ad hoc. Update artifacts **are** signed, by `release.yml`. |
| A release that has run | `release.yml` exists but has never run: `TAURI_SIGNING_PRIVATE_KEY` is not set, and the repository is private until the first stable release. |
| Artifact publication outside GitHub (Cloudflare R2, own domain) | Not planned for now: releases and manifests live on GitHub Releases ([ADR-0011](../adr/0011-release-channel.md)). |
| `installers/windows/build.ps1`, `installers/macos/*.sh` | The `installers/` tree contains only `.gitkeep` files. |
| `scripts/sign-*.ps1`, `scripts/upload-to-r2.sh`, `scripts/update-manifest.sh`, `scripts/build-mac-universal.sh`, `scripts/setup-apple-certs.sh`, `scripts/verify-windows-signatures.ps1` | None exist. What `scripts/` does hold for releases: `fetch-sumatrapdf.{ps1,sh}`, `build-shell-ext.ps1`, `bump-version.sh`, `licenses/generate-notices.sh` and `release/release_tools.py` (the manifest and signature helper `release.yml` runs). |
| Universal macOS binary | Not built: `release.yml` builds Apple silicon and Intel as separate downloads (Versorium's layout). |
| `cargo audit` / `pnpm audit` (NFR-065) | No scheduled dependency-audit workflow. Dependabot (`.github/dependabot.yml`) is the only dependency automation. |
| `gitleaks` secret scanning (NFR-027) | Not configured. |
| Reproducible-build verification (NFR-064) | Not automated. |

### Updater signing

Releases are signed so the installed app can verify an update before installing it (FR-082, [ADR-0008](../adr/0008-installer-nsis-per-user.md)). The key pair is minisign, generated once with `pnpm tauri signer generate`:

- **Public key** — committed, in `src-tauri/tauri.conf.json` → `plugins.updater.pubkey`. Every build embeds it and accepts only artifacts signed by the matching private key.
- **Private key** — **never committed**. A release build reads it from the `TAURI_SIGNING_PRIVATE_KEY` secret (the key file's contents), plus `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` if the key has a password. `release.yml` keeps it away from the builds: they sign with a throwaway key, and the `sign` job runs `tauri signer sign` with the real one on the NSIS setup (Windows) and `Pluryprint.app.tar.gz` (macOS), writing the `.sig` files the release carries ([releasing.md](./releasing.md) §5). Keep an offline backup. If the key is lost, installed copies cannot be updated in place. If it leaks, rotate it and ship the new public key through a release signed with the old one.
- **The private key must have a password.** Tauri asks for the password whenever `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` is not set, and a passwordless key can only be used by setting that variable to an empty string. Windows PowerShell cannot do that: `$env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = ""` *deletes* the variable (verified with `Test-Path env:...` on 5.1), so `pnpm tauri build` finishes both bundles and then waits forever at the password prompt, with no `.sig` written. In a non-interactive job (a scheduled task, CI) that is a hang, not an error. Generate release keys with a non-empty password (`pnpm tauri signer generate -w <file>` asks for one), and run signing builds with `CI=true` and a job timeout so a missing password fails instead of hanging. If an existing key has no password, sign the artifact explicitly instead, from `cmd.exe`: `pnpm tauri signer sign -f <key file> -p "" <artifact>`. Windows PowerShell 5.1 drops empty arguments to native programs, so the same line typed there loses the `""`; `cmd.exe` passes it through.
- **Manifest** — the release uploads `latest.json` as a release asset: `version`, `notes`, `pub_date`, and a `platforms` map (`windows-x86_64`, `windows-aarch64`, `darwin-aarch64`, `darwin-x86_64`; the updater falls back from `{os}-{arch}-{installer}` to these) of `{ url, signature }`, where `signature` is the contents of the matching `.sig`. The stable channel reads it from `https://github.com/MAECLY/maecly-print/releases/latest/download/latest.json`, the beta channel from the `channel-beta` release (see [releasing.md](./releasing.md) §3).

PR and `main` builds have no key and must not get one; `ci.yml` builds no bundle at all. With `createUpdaterArtifacts` on, Tauri refuses to bundle without a private key, so an unsigned local build uses `--config src-tauri/tauri.ci.conf.json`, which turns updater artifacts off for that build only.

To test an update end to end without publishing, serve a `latest.json` and a signed artifact locally, then point a **debug** build at it with `PLURYPRINT_UPDATE_ENDPOINT=http://127.0.0.1:<port>/latest.json`. Release builds only accept `https`. The variable also redirects `update check`, the About screen's check and the launch-time check, which read that manifest's `version` and `notes` exactly as they read the channel's published manifest (`update::fetch_configured_latest`), so the whole flow can be exercised against one test server. A copy of the artifact with one byte changed must fail with "failed its signature check and was NOT installed" (exit code 11 from `update install`).

`BUILD_DATE` (About, issue reports) is the build script's run date, re-stamped on every source change. A reproducible release pins it with `SOURCE_DATE_EPOCH`.

### Required secrets

**None** for `ci.yml`, which reads no `secrets.*` value. `release.yml` needs `TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` (release keys must have a password; see above), and its `guard` job fails a tag build when the key is not set. Only one step of its `sign` job reads them; the four builds get no secret and a read-only token. The full list of secrets and repository settings is in [releasing.md](./releasing.md) §2.

### Constraints the release pipeline keeps

- **Tag-triggered** on `v*`, with the channel derived from the tag (any `-` suffix → prerelease, beta channel; otherwise stable).
- **Manual approval gate.** The workflow only ever builds a **draft**; a person publishes it after the checks in [releasing.md](./releasing.md) §6. On the free plan with a private repository, Environments with required reviewers are not available; the draft is the gate.
- **No signing in CI.** PR and `main` builds must never have access to signing material; forks and contributors cannot accidentally sign.
- **`scripts/fetch-sumatrapdf.ps1` runs before any Windows packaging step.** `SumatraPDF.exe` is not committed (see `src-tauri/resources/sumatrapdf/README.md`), so a packaging job that skips this produces an installer whose every print job fails with `DependencyMissing`.
- **Least privilege and pinned actions.** Every workflow states its permissions (`ci.yml` reads only; in `release.yml` only `publish` has `contents: write`), every action is pinned to a full commit SHA with its version in a comment, and every checkout sets `persist-credentials: false`. `pnpm install` runs with `--ignore-scripts` everywhere. The release jobs use no cache. `src-tauri/tests/release_pipeline.rs` fails when any of this regresses; `actionlint` and `zizmor` report nothing on the workflows (2026-10-10).
- **No self-hosted runners.** GitHub-hosted runners are sufficient at our scale, and a self-hosted runner with access to signing secrets is a supply-chain liability. If cost becomes a concern, narrow the PR matrix rather than moving off hosted runners.
- **Notices before a release.** The `guard` job regenerates `THIRD-PARTY-NOTICES.md` with cargo-about 0.9.2 and fails the tag build when the committed copy differs.

## Artifacts

`ci.yml` uploads none. The release builds' artifacts are described in [releasing.md](./releasing.md).

## Status checks required for merge

Branch protection is a GitHub repository setting, not something this file can assert. Both jobs are designed to gate: `frontend`, and `backend` on both matrix entries (`Backend (windows-latest)`, `Backend (macos-latest)`).

## See also

- [installer-windows.md](./installer-windows.md)
- [installer-macos.md](./installer-macos.md)
- [code-signing.md](./code-signing.md)
- [auto-update-flow.md](./auto-update-flow.md)
- [../06-roadmap/release-strategy.md](../06-roadmap/release-strategy.md)
- [../08-development/release-checklist.md](../08-development/release-checklist.md)
- [../08-development/testing-strategy.md](../08-development/testing-strategy.md)
