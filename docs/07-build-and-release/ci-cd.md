# CI/CD

> **TL;DR.** GitHub Actions runs three workflows: `ci.yml` on every PR (test/lint/build, no signing), `nightly.yml` every night (signed nightly with self-signed cert), and `release.yml` on every `v*` tag (full signed + notarized release pipeline). The matrix covers Windows x64 + ARM64 and macOS aarch64 + x86_64 (Universal binary).

This document specifies the three GitHub Actions workflows, the build matrix, required secrets, caching strategy, artifact retention, and manual approval gates.

## Workflows overview

| Workflow | File | Trigger | Signs? | Publishes? |
|---|---|---|---|---|
| **CI** | `.github/workflows/ci.yml` | PRs, pushes to `main` | No | No (artifacts kept 7 days for debugging) |
| **Nightly** | `.github/workflows/nightly.yml` | Cron `0 3 * * *` (03:00 UTC daily) | Self-signed | Yes (to nightly channel) |
| **Release** | `.github/workflows/release.yml` | Tag matching `v*` | EV (Win) + Apple Dev ID (Mac) | Yes (to stable or beta channel) |

## CI workflow (`ci.yml`)

Runs on every pull request and every push to `main`. Goal: catch breakage before merge.

```yaml
name: CI

on:
  pull_request:
  push:
    branches: [main]

concurrency:
  group: ${{ github.workflow }}-${{ github.ref }}
  cancel-in-progress: true

env:
  CARGO_TERM_COLOR: always
  RUST_BACKTRACE: 1

jobs:
  rust-checks:
    name: Rust (${{ matrix.os }})
    runs-on: ${{ matrix.os }}
    strategy:
      fail-fast: false
      matrix:
        os: [windows-latest, macos-14]
    steps:
      - uses: actions/checkout@v4
      - name: Install Rust (stable)
        uses: dtolnay/rust-toolchain@stable
        with:
          components: clippy, rustfmt
      - name: Cache cargo registry + target
        uses: Swatinem/rust-cache@v2
        with:
          workspaces: src-tauri
      - name: cargo fmt --check
        working-directory: src-tauri
        run: cargo fmt --all -- --check
      - name: cargo clippy
        working-directory: src-tauri
        run: cargo clippy --all-targets --all-features -- -D warnings
      - name: cargo test
        working-directory: src-tauri
        run: cargo test --all-features
      - name: cargo audit
        working-directory: src-tauri
        run: |
          cargo install cargo-audit --locked || true
          cargo audit

  frontend-checks:
    name: Frontend
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: pnpm/action-setup@v4
        with: { version: 9 }
      - uses: actions/setup-node@v4
        with:
          node-version: 20
          cache: pnpm
      - run: pnpm install --frozen-lockfile
      - run: pnpm lint
      - run: pnpm prettier --check .
      - run: pnpm typecheck
      - run: pnpm test --reporter=verbose

  build:
    name: Build (${{ matrix.os }} / ${{ matrix.arch }})
    needs: [rust-checks, frontend-checks]
    runs-on: ${{ matrix.runner }}
    strategy:
      fail-fast: false
      matrix:
        include:
          - os: windows
            arch: x86_64
            runner: windows-latest
            target: x86_64-pc-windows-msvc
          - os: windows
            arch: aarch64
            runner: windows-latest
            target: aarch64-pc-windows-msvc
          - os: macos
            arch: x86_64
            runner: macos-14
            target: x86_64-apple-darwin
          - os: macos
            arch: aarch64
            runner: macos-14
            target: aarch64-apple-darwin
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with: { targets: ${{ matrix.target }} }
      - uses: pnpm/action-setup@v4
        with: { version: 9 }
      - uses: actions/setup-node@v4
        with: { node-version: 20, cache: pnpm }
      - uses: Swatinem/rust-cache@v2
        with: { workspaces: src-tauri, key: ${{ matrix.target }} }
      - run: pnpm install --frozen-lockfile
      - name: Tauri build (no signing)
        run: pnpm tauri build --target ${{ matrix.target }} --ci
      - name: Upload artifacts
        uses: actions/upload-artifact@v4
        with:
          name: ${{ matrix.os }}-${{ matrix.arch }}-unsigned
          path: |
            src-tauri/target/${{ matrix.target }}/release/bundle/
          retention-days: 7
```

Key choices:

- **`concurrency.cancel-in-progress: true`:** if a new commit lands on a PR, cancel the in-flight CI for the previous commit. Saves runner minutes.
- **`fail-fast: false`:** if Windows fails, still finish macOS to give full feedback.
- **Build job depends on `rust-checks` and `frontend-checks`:** no point building if linting fails.
- **No signing in CI:** the EV cert and Apple Dev ID are not exposed to PR builds. Forks and contributors cannot accidentally sign.
- **7-day artifact retention:** enough to debug a failing PR; not so long it bloats storage.

## Nightly workflow (`nightly.yml`)

Runs every night at 03:00 UTC. Builds, self-signs, uploads to nightly channel.

```yaml
name: Nightly

on:
  schedule:
    - cron: '0 3 * * *'
  workflow_dispatch:

permissions:
  contents: read

jobs:
  detect-changes:
    name: Detect changes since last nightly
    runs-on: ubuntu-latest
    outputs:
      should_build: ${{ steps.check.outputs.should_build }}
      version: ${{ steps.version.outputs.version }}
    steps:
      - uses: actions/checkout@v4
        with: { fetch-depth: 0 }
      - id: check
        run: |
          # Look up last nightly tag
          LAST_TAG=$(git tag --list 'nightly-*' --sort=-creatordate | head -n1 || echo "")
          if [ -z "$LAST_TAG" ]; then
            echo "should_build=true" >> $GITHUB_OUTPUT
            echo "First nightly run" >&2
            exit 0
          fi
          if [ "$(git rev-list --count $LAST_TAG..HEAD)" -gt 0 ]; then
            echo "should_build=true" >> $GITHUB_OUTPUT
          else
            echo "should_build=false" >> $GITHUB_OUTPUT
            echo "No new commits since $LAST_TAG; skipping" >&2
          fi
      - id: version
        run: |
          BASE_VER=$(grep '^version' src-tauri/Cargo.toml | head -n1 | sed 's/.*"\(.*\)"/\1/')
          DATE=$(date -u +%Y%m%d)
          SHA=$(git rev-parse --short HEAD)
          echo "version=${BASE_VER}-nightly.${DATE}.${SHA}" >> $GITHUB_OUTPUT

  build-windows:
    needs: detect-changes
    if: needs.detect-changes.outputs.should_build == 'true'
    runs-on: windows-latest
    strategy:
      fail-fast: false
      matrix:
        target: [x86_64-pc-windows-msvc, aarch64-pc-windows-msvc]
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with: { targets: ${{ matrix.target }} }
      - uses: pnpm/action-setup@v4
        with: { version: 9 }
      - uses: actions/setup-node@v4
        with: { node-version: 20, cache: pnpm }
      - uses: Swatinem/rust-cache@v2
        with: { workspaces: src-tauri, key: ${{ matrix.target }} }
      - run: pnpm install --frozen-lockfile
      - name: Override version
        run: |
          (Get-Content src-tauri/tauri.conf.json) `
            -replace '"version": ".*"', '"version": "${{ needs.detect-changes.outputs.version }}"' `
            | Set-Content src-tauri/tauri.conf.json
      - name: Build
        run: pnpm tauri build --target ${{ matrix.target }}
      - name: Sign with self-signed nightly cert
        env:
          NIGHTLY_CERT_PASSWORD: ${{ secrets.NIGHTLY_CERT_PASSWORD }}
        run: ./scripts/sign-nightly.ps1
      - name: Sign updater bundle (Tauri Ed25519)
        env:
          TAURI_SIGNING_PRIVATE_KEY: ${{ secrets.TAURI_SIGNING_PRIVATE_KEY }}
          TAURI_SIGNING_PRIVATE_KEY_PASSWORD: ${{ secrets.TAURI_SIGNING_PRIVATE_KEY_PASSWORD }}
        run: pnpm tauri signer sign --password "$env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD" src-tauri/target/${{ matrix.target }}/release/bundle/nsis/*.nsis.zip
      - uses: actions/upload-artifact@v4
        with:
          name: nightly-windows-${{ matrix.target }}
          path: src-tauri/target/${{ matrix.target }}/release/bundle/

  build-macos:
    needs: detect-changes
    if: needs.detect-changes.outputs.should_build == 'true'
    runs-on: macos-14
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with: { targets: 'x86_64-apple-darwin,aarch64-apple-darwin' }
      - uses: pnpm/action-setup@v4
        with: { version: 9 }
      - uses: actions/setup-node@v4
        with: { node-version: 20, cache: pnpm }
      - run: pnpm install --frozen-lockfile
      - name: Build both arches and lipo
        run: ./scripts/build-mac-universal.sh
      - name: Sign + notarize (skipped for nightly; no notarization for nightlies)
        env:
          APPLE_DEVELOPER_ID_APPLICATION_P12_BASE64: ${{ secrets.APPLE_DEVELOPER_ID_APPLICATION_P12_BASE64 }}
          APPLE_P12_PASSWORD: ${{ secrets.APPLE_P12_PASSWORD }}
        run: |
          ./scripts/setup-apple-certs.sh
          ./installers/macos/codesign.sh
      - uses: actions/upload-artifact@v4
        with:
          name: nightly-macos-universal
          path: dist/

  upload-and-publish:
    needs: [detect-changes, build-windows, build-macos]
    runs-on: ubuntu-latest
    steps:
      - uses: actions/download-artifact@v4
        with: { path: artifacts/ }
      - name: Upload to R2 (nightly channel)
        env:
          R2_ACCESS_KEY: ${{ secrets.R2_ACCESS_KEY }}
          R2_SECRET_KEY: ${{ secrets.R2_SECRET_KEY }}
          R2_ENDPOINT: ${{ secrets.R2_ENDPOINT }}
        run: ./scripts/upload-to-r2.sh nightly ${{ needs.detect-changes.outputs.version }}
      - name: Update nightly.json manifest
        env:
          R2_ACCESS_KEY: ${{ secrets.R2_ACCESS_KEY }}
          R2_SECRET_KEY: ${{ secrets.R2_SECRET_KEY }}
          R2_ENDPOINT: ${{ secrets.R2_ENDPOINT }}
        run: ./scripts/update-manifest.sh nightly ${{ needs.detect-changes.outputs.version }}
      - name: Tag the commit
        run: |
          git tag nightly-${{ needs.detect-changes.outputs.version }}
          git push origin nightly-${{ needs.detect-changes.outputs.version }}
```

Key choices:

- **Skip if no new commits:** the `detect-changes` job exits the workflow early if `main` hasn't moved. Saves runners.
- **No notarization for nightlies on macOS:** notarization adds 10–30 minutes per build. We accept the Gatekeeper warning for nightlies (users are warned in the channel description).
- **Self-signed cert on Windows:** SmartScreen warning expected.
- **Tag the commit:** lets us reference exactly which `main` SHA produced each nightly.

## Release workflow (`release.yml`)

Triggered when a tag matching `v*` is pushed. Full signed pipeline with manual approval gate before publishing.

```yaml
name: Release

on:
  push:
    tags: ['v*']

permissions:
  contents: write    # to create the GitHub Release

jobs:
  determine-channel:
    runs-on: ubuntu-latest
    outputs:
      channel: ${{ steps.channel.outputs.channel }}
      version: ${{ steps.channel.outputs.version }}
    steps:
      - id: channel
        run: |
          TAG="${GITHUB_REF#refs/tags/}"
          VERSION="${TAG#v}"
          if echo "$VERSION" | grep -qE '\-(beta|rc)\.'; then
            echo "channel=beta" >> $GITHUB_OUTPUT
          else
            echo "channel=stable" >> $GITHUB_OUTPUT
          fi
          echo "version=$VERSION" >> $GITHUB_OUTPUT

  build-windows:
    needs: determine-channel
    runs-on: windows-latest
    strategy:
      fail-fast: false
      matrix:
        target: [x86_64-pc-windows-msvc, aarch64-pc-windows-msvc]
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with: { targets: ${{ matrix.target }} }
      - uses: pnpm/action-setup@v4
        with: { version: 9 }
      - uses: actions/setup-node@v4
        with: { node-version: 20, cache: pnpm }
      - uses: Swatinem/rust-cache@v2
        with: { workspaces: src-tauri, key: release-${{ matrix.target }} }
      - run: pnpm install --frozen-lockfile
      - name: Build Tauri release
        run: pnpm tauri build --target ${{ matrix.target }}
      - name: Build Inno Setup installer
        run: ./installers/windows/build.ps1 -Version "${{ needs.determine-channel.outputs.version }}" -Channel release
      - name: Sign all binaries with EV cert (SSL.com Cloud HSM)
        env:
          SSL_COM_USERNAME: ${{ secrets.SSL_COM_USERNAME }}
          SSL_COM_PASSWORD: ${{ secrets.SSL_COM_PASSWORD }}
          SSL_COM_CREDENTIAL_ID: ${{ secrets.SSL_COM_CREDENTIAL_ID }}
          SSL_COM_TOTP_SECRET: ${{ secrets.SSL_COM_TOTP_SECRET }}
        run: ./scripts/sign-windows-ev.ps1
      - name: Verify signatures
        run: ./scripts/verify-windows-signatures.ps1
      - name: Sign updater bundle (Tauri Ed25519)
        env:
          TAURI_SIGNING_PRIVATE_KEY: ${{ secrets.TAURI_SIGNING_PRIVATE_KEY }}
          TAURI_SIGNING_PRIVATE_KEY_PASSWORD: ${{ secrets.TAURI_SIGNING_PRIVATE_KEY_PASSWORD }}
        run: pnpm tauri signer sign --password "$env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD" src-tauri/target/${{ matrix.target }}/release/bundle/nsis/*.nsis.zip
      - uses: actions/upload-artifact@v4
        with:
          name: release-windows-${{ matrix.target }}
          path: |
            installers/windows/output/
            src-tauri/target/${{ matrix.target }}/release/bundle/

  build-macos:
    needs: determine-channel
    runs-on: macos-14
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with: { targets: 'x86_64-apple-darwin,aarch64-apple-darwin' }
      - uses: pnpm/action-setup@v4
        with: { version: 9 }
      - uses: actions/setup-node@v4
        with: { node-version: 20, cache: pnpm }
      - run: pnpm install --frozen-lockfile
      - name: Build universal
        run: ./scripts/build-mac-universal.sh
      - name: Sign + notarize + staple
        env:
          APPLE_DEVELOPER_ID_APPLICATION_P12_BASE64: ${{ secrets.APPLE_DEVELOPER_ID_APPLICATION_P12_BASE64 }}
          APPLE_DEVELOPER_ID_INSTALLER_P12_BASE64: ${{ secrets.APPLE_DEVELOPER_ID_INSTALLER_P12_BASE64 }}
          APPLE_P12_PASSWORD: ${{ secrets.APPLE_P12_PASSWORD }}
          APPLE_ID: ${{ secrets.APPLE_ID }}
          APPLE_TEAM_ID: ${{ secrets.APPLE_TEAM_ID }}
          APPLE_APP_PASSWORD: ${{ secrets.APPLE_APP_PASSWORD }}
        run: |
          ./scripts/setup-apple-certs.sh
          ./installers/macos/codesign.sh
          ./installers/macos/pkgbuild.sh ${{ needs.determine-channel.outputs.version }}
          ./installers/macos/create-dmg.sh ${{ needs.determine-channel.outputs.version }}
          ./installers/macos/notarize.sh dist/MaeclyPrint-${{ needs.determine-channel.outputs.version }}.pkg
          ./installers/macos/notarize.sh dist/MaeclyPrint-${{ needs.determine-channel.outputs.version }}.dmg
      - name: Sign updater bundle (Tauri Ed25519)
        env:
          TAURI_SIGNING_PRIVATE_KEY: ${{ secrets.TAURI_SIGNING_PRIVATE_KEY }}
          TAURI_SIGNING_PRIVATE_KEY_PASSWORD: ${{ secrets.TAURI_SIGNING_PRIVATE_KEY_PASSWORD }}
        run: pnpm tauri signer sign --password "$TAURI_SIGNING_PRIVATE_KEY_PASSWORD" dist/*.app.tar.gz
      - uses: actions/upload-artifact@v4
        with:
          name: release-macos-universal
          path: dist/

  publish:
    needs: [determine-channel, build-windows, build-macos]
    runs-on: ubuntu-latest
    environment: production    # triggers manual approval gate
    steps:
      - uses: actions/checkout@v4
      - uses: actions/download-artifact@v4
        with: { path: artifacts/ }
      - name: Upload all artifacts to R2
        env:
          R2_ACCESS_KEY: ${{ secrets.R2_ACCESS_KEY }}
          R2_SECRET_KEY: ${{ secrets.R2_SECRET_KEY }}
          R2_ENDPOINT: ${{ secrets.R2_ENDPOINT }}
        run: ./scripts/upload-to-r2.sh ${{ needs.determine-channel.outputs.channel }} ${{ needs.determine-channel.outputs.version }}
      - name: Update channel manifest
        env:
          R2_ACCESS_KEY: ${{ secrets.R2_ACCESS_KEY }}
          R2_SECRET_KEY: ${{ secrets.R2_SECRET_KEY }}
          R2_ENDPOINT: ${{ secrets.R2_ENDPOINT }}
        run: ./scripts/update-manifest.sh ${{ needs.determine-channel.outputs.channel }} ${{ needs.determine-channel.outputs.version }}
      - name: Create GitHub Release
        uses: softprops/action-gh-release@v2
        with:
          name: MaeclyPrint ${{ needs.determine-channel.outputs.version }}
          body_path: RELEASE_NOTES.md
          files: |
            artifacts/release-windows-x86_64-pc-windows-msvc/output/*.exe
            artifacts/release-windows-aarch64-pc-windows-msvc/output/*.exe
            artifacts/release-macos-universal/*.pkg
            artifacts/release-macos-universal/*.dmg
          prerelease: ${{ needs.determine-channel.outputs.channel == 'beta' }}
```

## Manual approval gate

The `publish` job is bound to a GitHub Environment named `production`. The environment is configured (in repo Settings → Environments) with:

- **Required reviewers:** the two maintainers.
- **Wait timer:** 0 minutes.
- **Deployment protection rules:** only the `main` branch (or release branches) can deploy.

When the workflow reaches the `publish` job, GitHub pauses and notifies reviewers. A reviewer clicks "Approve and deploy" after verifying:

- All build jobs succeeded.
- All signature verifications passed.
- Notarization succeeded.
- The release notes are accurate.

Without manual approval, signed binaries never go to the CDN. This protects against supply-chain attacks where a compromised PR could otherwise sneak through.

## Required secrets

| Secret | Used for |
|---|---|
| `SSL_COM_USERNAME`, `SSL_COM_PASSWORD`, `SSL_COM_CREDENTIAL_ID`, `SSL_COM_TOTP_SECRET` | Windows EV signing via SSL.com Cloud HSM |
| `NIGHTLY_CERT_PASSWORD` | Self-signed nightly cert password |
| `APPLE_DEVELOPER_ID_APPLICATION_P12_BASE64`, `APPLE_DEVELOPER_ID_INSTALLER_P12_BASE64`, `APPLE_P12_PASSWORD` | Apple cert import |
| `APPLE_ID`, `APPLE_TEAM_ID`, `APPLE_APP_PASSWORD` | Notarization |
| `TAURI_SIGNING_PRIVATE_KEY`, `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | Updater bundle signing (Ed25519) |
| `R2_ACCESS_KEY`, `R2_SECRET_KEY`, `R2_ENDPOINT` | Cloudflare R2 upload |

All secrets are repo-scoped. Forks do not inherit them; PRs from forks cannot trigger the release workflow.

## Caching strategy

| Cache | Key | Saves |
|---|---|---|
| **`Swatinem/rust-cache`** | `${{ matrix.target }}` | Cargo registry + `target/` directory. Save: ~5 minutes per Rust build (down from 8 to 3 min). |
| **pnpm via `actions/setup-node`** | `pnpm-lock.yaml` hash | Node modules tarball. Save: ~1 minute per build. |
| **Tauri build cache** | Implicit via `target/` cache | Frontend build output. Save: ~30 seconds per build. |

We do not cache:

- Build outputs that include version strings (would require invalidation per build).
- Signing tools (small, cheap to download per run, and we want guaranteed freshness).

## Artifact retention

| Workflow | Retention |
|---|---|
| CI | 7 days (debugging) |
| Nightly | 14 days (rollback window for nightly users) |
| Release | 90 days on GitHub Actions; **forever** on R2 (the canonical store) |

Old releases on R2 are retained at least 12 months to support users on stale versions who want to update.

## Build matrix details

| Runner | Architecture | Builds for |
|---|---|---|
| `windows-latest` | x86_64 | Windows x86_64 (native), Windows aarch64 (cross-compile) |
| `macos-14` | aarch64 (M-series) | macOS aarch64 (native), macOS x86_64 (cross-compile via `rustup target add`) |
| `ubuntu-latest` | x86_64 | Frontend lint/test, manifest upload (no Tauri builds) |

We do **not** use `macos-13` (Intel) as a separate runner; Apple Silicon runners can build x86_64 slices via Rust's cross-compilation, and the resulting binary runs identically on Intel Macs (verified in QA).

For aarch64 Windows: `windows-latest` is x86_64 but Rust cross-compiles to aarch64 cleanly. We run integration tests for aarch64 on a separate (manual) test pass with an ARM64 Windows VM.

## Status checks required for merge

The following checks are required on PRs to `main`:

- `Rust (windows-latest)` (rust-checks job)
- `Rust (macos-14)`
- `Frontend`
- `Build (windows / x86_64)`
- `Build (macos / aarch64)`

Other build matrix entries are run but not required (to allow occasional ARM64-specific failures while we hunt them down without blocking unrelated PRs).

## Self-hosted runners

We do **not** use self-hosted runners. GitHub-hosted runners are sufficient for our scale, and self-hosted runners introduce supply-chain risk (a compromised runner could exfiltrate signing secrets).

If runner cost becomes a concern, the cheapest mitigation is to reduce build matrix breadth on PRs (run aarch64 only on `main` pushes), not to switch to self-hosted.

## Cost estimate

Approximate GitHub Actions minute usage per month (free tier is 2000 minutes for private repos; unlimited for public):

| Workflow | Per run | Frequency | Monthly minutes |
|---|---|---|---|
| CI | ~30 min (4 jobs × ~7 min avg) | ~50 PRs/month | 1500 |
| Nightly | ~45 min | 30/month | 1350 |
| Release | ~90 min | 2-4/month | 360 |
| **Total** | | | ~3200 |

Public repo on Actions is unlimited, so this is informational only. If we ever go private, we'd need a paid plan.

## See also

- [installer-windows.md](./installer-windows.md)
- [installer-macos.md](./installer-macos.md)
- [code-signing.md](./code-signing.md)
- [auto-update-flow.md](./auto-update-flow.md)
- [../06-roadmap/release-strategy.md](../06-roadmap/release-strategy.md)
- [../08-development/release-checklist.md](../08-development/release-checklist.md)
