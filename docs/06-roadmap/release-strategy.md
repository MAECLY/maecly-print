# Release Strategy

> **TL;DR.** Pluryprint follows strict semantic versioning, ships on three channels (stable, beta, nightly), and uses a trunk-based branching model with squash-merged feature branches. Stable releases land every 4–8 weeks, betas every 2 weeks, and nightlies every night.

> **Status (2026-10-10).** Parts of this strategy were replaced by [ADR-0011](../adr/0011-release-channel.md) and are described as built in [releasing.md](../07-build-and-release/releasing.md): releases and the per-channel `latest.json` manifests live on **GitHub Releases**, not `updates.maecly.com`; a `vX.Y.Z` tag makes a stable draft and a `vX.Y.Z-beta.N` tag a beta one; there are **no nightly builds** (the nightly channel follows beta); and nothing is signed with an EV, self-signed or Developer ID certificate — updates are signed with Pluryprint's own minisign key and every release lists `SHA256SUMS`. The first, 0.0.1 preview, was published on 2026-10-10. Versioning and branching below still apply.

This document defines how versions are numbered, when they ship, what each channel commits to, and how we handle pre-releases, yanks, and long-term support. It is the contract between the project and its users.

## Versioning: Semantic Versioning 2.0.0

We follow [SemVer 2.0.0](https://semver.org/spec/v2.0.0.html) verbatim. Versions take the form `MAJOR.MINOR.PATCH[-prerelease][+build]`.

### What each segment means in practice

| Segment | Bumped when | Examples |
|---|---|---|
| `MAJOR` | Backward-incompatible change to **CLI flags**, **profile TOML schema**, **IPC API**, **shell extension contract**, or **on-disk data layout**. | `1.0.0` → `2.0.0` |
| `MINOR` | New backward-compatible feature: new adapter, new GUI page, new CLI subcommand, new profile field with a default. | `1.0.0` → `1.1.0` |
| `PATCH` | Backward-compatible bug fix, security fix, dependency bump that doesn't change behavior, doc-only changes. | `1.0.0` → `1.0.1` |

### What is **NOT** part of the public contract

These can change without a major bump:

- Internal Rust module organization.
- Names of private functions / structs.
- Log message wording (unless format like JSON line schema changes).
- The exact wording of UI copy (translations are minor at most).
- Internal IPC channel names if the public Tauri command surface is unchanged.
- Bundled SumatraPDF / LibreOffice patch versions.

### Pre-1.0 caveat

Per SemVer spec section 4: "Anything MAY change at any time" while `MAJOR` is `0`. We honor this. Versions `0.x.y` are explicitly marked as such and users are warned that `0.2.0` may break things `0.1.0` did.

The `1.0.0` line is when the contract starts. After 1.0.0, the table above is binding.

## Channels

Three release channels, each with its own audience and stability promise.

| Channel | Cadence | Audience | Stability promise | Auto-update default |
|---|---|---|---|---|
| **stable** | Every 4–8 weeks | All end users | Tested across full QA matrix; no known crash-on-startup or data-loss bugs. | On (24h check) |
| **beta** | Every ~2 weeks | Early adopters, testers, contributors | Passes CI + smoke tests; may have minor bugs; report-and-fix loop is fast. | On (24h check) |
| **nightly** | Every night `main` is green | Contributors, paranoid testers | Passes CI; any feature in `main` may be present; expect rough edges. | Off by default (manual check only) |

### How users pick a channel

- **Default:** stable.
- Switching: Settings → Updates → Channel selector. Switching from stable to beta or nightly is allowed at any time. Switching from nightly back to stable requires the next stable release to be `>=` current nightly version (otherwise it would be a downgrade — see below).

### Channel-version rules

Versions are continuous across channels. There is no `1.0.0-stable` vs `1.0.0-beta`. Instead:

- **stable:** version is exactly `MAJOR.MINOR.PATCH` (no prerelease tag).
- **beta:** version is `MAJOR.MINOR.PATCH-beta.N` where `N` increments per beta release within the same MAJOR.MINOR.PATCH window.
- **nightly:** version is `MAJOR.MINOR.PATCH-nightly.YYYYMMDD.SHORT_SHA` (e.g. `1.1.0-nightly.20260601.a3f2d1c`).
- Release candidates: `MAJOR.MINOR.PATCH-rc.N` immediately preceding a stable.

Examples of an ordered version sequence leading up to `1.1.0`:

```
1.0.0
1.0.1
1.0.2
1.1.0-nightly.20260520.aaaaaaa
1.1.0-nightly.20260521.bbbbbbb
1.1.0-beta.1
1.1.0-beta.2
1.1.0-rc.1
1.1.0-rc.2
1.1.0
```

SemVer's precedence rules (`-alpha < -beta < -rc < <no tag>`) apply naturally.

## Cadence in detail

### Stable: every 4–8 weeks

We aim for a stable release every 4–8 calendar weeks. The window is wide on purpose:

- 4 weeks when there are accumulated fixes and small features ready.
- Up to 8 weeks when a larger feature is in flight and we want to ship it whole.
- Never longer than 8 weeks (forces us to cut scope rather than sit on a stale stable).

Stable releases get:

- A GitHub Release with full changelog excerpt.
- An update manifest update on `updates.maecly.com/pluryprint/stable.json`.
- A blog post / social announcement for MINOR and MAJOR; PATCH is silent.

### Beta: every ~2 weeks

Betas snapshot whatever is on `main` at the cut moment. Cuts happen on the **second and fourth Monday** of every month, deterministically. If `main` is red on cut day, the cut is delayed until the next green commit (usually within hours, max 48h).

Betas get:

- A GitHub Pre-release.
- An update manifest update on `updates.maecly.com/pluryprint/beta.json`.
- A short note in `#announcements` (project Discord/forum, when it exists).

### Nightly: every night `main` is green

A scheduled CI workflow runs at 03:00 UTC. If `main` has new commits since the last successful nightly build, the workflow:

1. Builds for all target platforms.
2. Signs with the nightly self-signed cert (NOT the EV cert).
3. Uploads to `updates.maecly.com/pluryprint/nightly/<version>/`.
4. Updates `nightly.json` manifest.

If the nightly build fails, the previous nightly remains the latest. We do not auto-fix; the next morning's `main` either heals it or somebody investigates.

## Branching strategy

We use a lightweight trunk-based model with feature branches that live no longer than a week.

```
main ───●───●───●───●───●───●───●───●───  (always green)
         \         \       \
          o─o       o─o     o─o          (feature/* branches, squashed in)
```

### Branch types

| Branch | Purpose | Lifetime | Merge style |
|---|---|---|---|
| `main` | Stable, always green, source of nightlies and tags | Forever | N/A |
| `feature/<short-description>` | One feature or coherent change | <1 week | Squash merge into `main` |
| `fix/<short-description>` | One bug fix | <1 week | Squash merge into `main` |
| `release/<MAJOR.MINOR>` | Stabilization branch for a stable release | Optional, only for high-risk releases | Cherry-pick from `main`, tag from this branch |
| `hotfix/<version>` | Patch-level fix on a previously released MAJOR.MINOR | Until tag | Squash merge into `release/<MAJOR.MINOR>`, then tag, then cherry-pick to `main` |

We **do not** maintain a `develop` branch. The Git Flow model is overkill for our size.

### Linear history

`main` has a strictly linear history. All merges are squash merges, producing one commit per PR on `main`. This makes `git bisect` trivial and `git log` readable.

### Tags

- Every released version gets an annotated tag: `v1.2.3`, `v1.2.3-beta.1`, etc.
- Tags are pushed only after the binary is built and signed, not before.
- Tag messages link to the GitHub Release.

## Release cut process (stable)

This is the abbreviated flow; full pre-release checklist lives in [../08-development/release-checklist.md](../08-development/release-checklist.md).

1. **T-7 days:** announce intent to cut on the project channel. Soft freeze on new features (fixes still welcome).
2. **T-3 days:** create `release/<MAJOR.MINOR>` branch from `main` if release is high-risk; otherwise stay on `main`.
3. **T-2 days:** cut `<version>-rc.1`. Distribute to testers. Run full QA matrix.
4. **T-1 day:** if rc.1 has issues, cut `rc.2`. Otherwise prep release notes.
5. **T-0:** tag `v<version>`. CI builds, signs, notarizes, publishes to GitHub Releases, updates manifest.
6. **T+1:** monitor crash reports (opt-in only) and update channel for the next 48h. If a critical issue surfaces, see "yanking" below.

## Yanking policy

We do not delete published binaries (broken links break user trust). Instead, we **yank** them by:

1. Editing the GitHub Release, prepending a clear `[YANKED]` warning to the release notes with the reason and the recommended replacement version.
2. Removing the version from the update manifest so auto-update no longer offers it.
3. If the issue is severe (data loss, security), uploading a `YANKED` marker file alongside the binary on the CDN that the app checks at startup and refuses to run with a clear error.

A yanked version still exists on disk for users who already installed it; we cannot un-install it. We can warn them at startup and direct them to upgrade.

### When we yank

- Critical security vulnerability (CVSS >= 7.0) with no patch available.
- Data-loss bug (anything that corrupts user files or profiles).
- Crash on launch on a supported OS version.

### When we do **not** yank

- Cosmetic bugs.
- Feature regressions that have a workaround.
- Performance regressions.

For these, we ship a patch release as quickly as possible but keep the broken release available.

## Long-term support (LTS) — future policy

Pre-2.0 we do not commit to LTS. After 2.0:

- The most recent two MINOR releases of the current MAJOR receive security fixes for **6 months** after their release.
- A new MAJOR (e.g. `2.0.0` → `3.0.0`) starts a 6-month overlap period during which the previous MAJOR's last MINOR receives security fixes only.
- LTS status is published in the GitHub Release notes and on the website.

This policy is intentionally conservative. We will revisit when there is real demand and a real maintenance budget.

## Backporting policy

A fix lands on `main` first, always. If it needs to ship to a still-supported MINOR:

1. Fix is squash-merged to `main`.
2. Maintainer cherry-picks the squashed commit onto `release/<MAJOR.MINOR>`.
3. Conflicts are resolved in the cherry-pick; the cherry-picked commit retains the original author and adds `[backport from main]` in the body.
4. New patch tag cut from `release/<MAJOR.MINOR>`.

If a fix cannot cleanly cherry-pick, we evaluate case by case: rewrite for the older branch, or decline the backport with a clear note in the issue.

## Downgrade handling

The auto-updater **never downgrades**. A user who manually installs `1.1.0` and then switches to a channel currently on `1.0.5` will not be downgraded automatically. They must uninstall and reinstall to go backward.

We do not refuse downgrades initiated by the user manually (they can install any binary they want), but we do not offer them as a guided flow.

## Deprecation policy

Public-API deprecations (CLI flags, profile fields, IPC commands) follow:

1. **Announce:** the feature is marked deprecated in the current MINOR's release notes and in `--help` / GUI.
2. **Coexist:** the deprecated feature continues to work for at least one full MINOR cycle.
3. **Remove:** the feature is removed in the next MAJOR. Bump MAJOR per SemVer.

Example timeline:

```
1.4.0  - new --output-dir flag added; --out flag deprecated (still works, prints warning).
1.5.0  - --out still works, still warns.
1.6.0  - --out still works, still warns. (Min one cycle done.)
2.0.0  - --out removed. MAJOR bump.
```

## Communication channels

| Channel | Use for |
|---|---|
| GitHub Releases | All releases (primary source of truth) |
| CHANGELOG.md | Human-readable changelog, updated atomically with releases |
| Project blog (when established) | MAJOR + significant MINOR announcements |
| Twitter/X / Mastodon | Headline announcements |
| Email list (opt-in) | Major announcements only, never marketing |

We do not push releases to package managers (winget, Homebrew, Chocolatey) until 1.0. After 1.0, we accept community-maintained packages and may officially ship one if a maintainer steps up.
