# Update Check and Install

> **TL;DR** — Two steps. The **check** asks the GitHub Releases API (`https://api.github.com/repos/<owner>/<repo>/releases/latest`) whether a newer tag exists and compares it to the running version with semver precedence. The **install** runs only when the user asks for it (About → **Install now**, or `pluryprint update install`): `tauri-plugin-updater` downloads the artifact named by `latest.json`, verifies its minisign signature against the public key in `tauri.conf.json`, and installs it per-user (§10, [ADR-0008](../adr/0008-installer-nsis-per-user.md)). `updates.maecly.com` does not exist and is not contacted.

## 0. Status and scope

| Capability                                | Shipped today | Where |
| ----------------------------------------- | ------------- | ----- |
| Check whether a newer release exists      | ✅            | `src-tauri/src/update/mod.rs` |
| Launch-time check (delayed, throttled)    | ✅            | `src-tauri/src/lib.rs` `setup` |
| Manual check from the About screen        | ✅            | `check_for_updates` IPC command |
| Manual check from a script                | ✅            | `pluryprint update check [--json]` |
| "Skip this version"                       | ✅            | `set_skipped_update_version`, FR-084 |
| Download an update                        | ✅            | `update::install::find_update` / `install`, on explicit request only |
| Install / replace the running binary      | ✅            | `install_update` IPC command, `pluryprint update install [--yes]` |
| Signature verification (minisign/Ed25519) | ✅            | `tauri-plugin-updater`, against `plugins.updater.pubkey` |
| Major version needs explicit opt-in (FR-087) | ✅         | `update::install::is_major_bump` + `allowMajor` / `--yes` |
| Separate `beta` / `nightly` release feeds | ❌            | the preference is stored; every check hits `releases/latest` |
| A release pipeline that publishes signed artifacts | ❌   | no `release.yml` yet |

§1–§7 describe the check. §10 describes the install.

## 1. Why a read-only check

The check is deliberately the smallest thing that answers the only question a user actually asks — "am I running the current version?" — without owning a release CDN, a signing key, or an in-process binary-replacement path. Distribution is GitHub Releases; the user follows the link and installs the artifact the same way they installed the app.

Everything except the one network call is a pure function with co-located tests (`repo_slug` / `parse_release` / `is_newer` / `evaluate` / `interpret_response` / `interpret_repo_probe` / `due_for_check` / `should_check_on_startup` / `should_notify`). `fetch_latest` is the only part that touches the network and is kept tiny so the rest is reasoned about — and tested — entirely offline.

## 2. The request

```
GET https://api.github.com/repos/{owner}/{repo}/releases/latest
User-Agent: Pluryprint/<version>
Accept: application/vnd.github+json
X-GitHub-Api-Version: 2022-11-28
```

- `{owner}/{repo}` is parsed at compile time from the crate's `repository` URL (`CARGO_PKG_REPOSITORY`) by `repo_slug()`. A URL that is not a recognizable `https://github.com/<owner>/<repo>` yields "repository not configured" rather than a bogus API path.
- The client is **`ureq`** (blocking, 10-second timeout). There is no `reqwest` and no async runtime in the update path.
- A system proxy is honoured from the standard environment variables — the first of `HTTPS_PROXY` / `https_proxy` / `HTTP_PROXY` / `http_proxy` that parses. `ureq` does not read these itself, so `build_agent()` wires them explicitly. Only HTTP `CONNECT` proxies are supported (the `socks-proxy` feature is off), so a `socks://` value fails to parse and is skipped — the connection is then direct.
- **No request body, no identifiers, no telemetry.** GitHub sees a plain unauthenticated GET.

### 2.1 Status → outcome

`interpret_response(status, body)` owns the whole mapping, so the decision table is unit-tested without a network:

| Status | Outcome |
| ------ | ------- |
| `200` + parseable release | `Ok(Some(ReleaseInfo))` |
| `200` + unparseable body  | **Error.** Reporting "you're up to date" off an unreadable payload would be a lie. |
| `404`                     | `Ok(None)` — no releases published yet |
| `403` / `429`             | Error: "GitHub rate limit reached — try again later" |
| anything else             | Error: "github.com returned an unexpected status (N)" |
| transport failure         | Error: "could not reach github.com — check your connection" |

**The 404 probe.** GitHub answers `404` both for "this repo has no releases" and for "this repo does not exist", so a wrong `repository` slug would otherwise render as a reassuring "You're up to date" forever. On that one branch only, a second request probes `GET /repos/{owner}/{repo}`; `interpret_repo_probe` turns a `404` there into a hard "the configured repository was not found — this build's repository URL is wrong", and treats every other status (including a rate-limited `403`) as inconclusive, i.e. "the repo exists, it just has no releases yet". The happy path stays a single request.

### 2.2 Parsed payload

Only three fields are read from the release JSON:

| Field       | Required | Used for |
| ----------- | -------- | -------- |
| `tag_name`  | yes      | The version compared against the running one |
| `html_url`  | no       | The link shown when an update is available |
| `body`      | no       | Release notes (markdown) shown in the prompt |

## 3. Version comparison

`is_newer(current, tag)` strips a single leading `v`/`V` from both sides and compares with **`semver::Version::cmp_precedence`**, not `Ord`. This matters: `Ord` totally-orders versions and therefore ranks `0.0.1+build` *above* `0.0.1`, which would flag a build-metadata-only "release" as an update. SemVer §10 excludes build metadata from precedence, and `cmp_precedence` implements exactly that.

An unparseable version on either side yields `false` and logs a warning, so a weird tag never spuriously prompts.

## 4. The launch-time check (FR-080)

The check lives in `run()`'s Tauri `setup`, not in a frontend effect, for three reasons: the single-instance plugin guarantees `setup` runs exactly once per process, so a second launch cannot double-check; the webview can be reloaded or its window closed and reopened, neither of which should re-run a network call; and the preference, the throttle and the skip rule already live in Rust next to the state file.

```
T = 0        App launches; window mounts. Nothing network happens.
T = 30 s     STARTUP_CHECK_DELAY elapses on a detached thread.
             run_startup_check():
               - updater.check_on_startup off?          -> None (no request)
               - last check < MIN_CHECK_INTERVAL_HOURS? -> None (no request)
               - fetch fails / rate-limited / offline?  -> None
               - not strictly newer?                    -> None (state stamped)
               - tag == skipped_version?                -> None (state stamped)
               - otherwise                              -> Some(UpdateCheck)
T = ~30 s+   Some(_) is emitted as the `update-available` event.
```

The whole thread — the sleep included — is detached, so a cold start never waits on the network and a user who quits before the delay elapses simply takes the thread down with the process, having persisted nothing.

`STARTUP_CHECK_DELAY` is 30 s and `MIN_CHECK_INTERVAL_HOURS` is 24 (`update/mod.rs`). Because every uninteresting outcome collapses to `None`, the frontend never has to filter the event — receiving `update-available` *is* the decision to tell the user.

### 4.1 Persisted state

`update-state.json` lives in the per-user **data** directory next to `queue.json` — it is live bookkeeping, not configuration, so it is deliberately *not* part of `config.toml`: a corrupt or deleted file must degrade to "check again", never to a settings-parse refusal.

```json
{
  "last_check_at": "2026-07-20T09:12:44Z",
  "skipped_version": "v1.3.0"
}
```

| Field             | Meaning |
| ----------------- | ------- |
| `last_check_at`   | RFC 3339 stamp of the last **completed** check, manual or automatic |
| `skipped_version` | The tag the user chose to skip (FR-084) |

Writes are atomic (temp file in the same directory, then rename). An unreadable file is logged and ignored; the worst case is one extra check.

**Throttle edge cases** (`due_for_check`): never having checked is due; an unparseable timestamp is due (a hand-edited or older-build file must not wedge updates forever); a timestamp in the *future* is due (the clock moved backwards, or the file was copied between machines). In all three, refusing to check would be the worse failure.

The state is stamped whenever a check **completed**, including one that found nothing — the throttle counts checks, not findings, or an up-to-date app would re-check on every launch. A manual check from the About screen stamps it too, so pressing the button postpones the next automatic check rather than adding to it.

### 4.2 Skipping a version (FR-084)

`should_notify` silences the launch-time prompt while `skipped_version` names the same release as `latest`. Skipping is **version-scoped**: skipping `1.2.0` silences that release only — `1.3.0` prompts again. Comparison ignores a leading `v` and build metadata, falling back to a case-insensitive string compare for non-semver tags. A *manual* check does not consult this: the user asked, so they get the answer either way.

## 5. Surfaces

| Surface | How |
| ------- | --- |
| React (About screen) | `commands.checkForUpdates()`, `commands.getUpdateState()`, `commands.setSkippedUpdateVersion(v)`; passive notification via the `update-available` event |
| CLI | `pluryprint update check [--json]` — the same `evaluate()` over the same feed |
| CLI | `pluryprint update channel [stable\|beta\|nightly]` — reads/writes `updater.channel` in `config.toml` |
| React (About screen) | `commands.installUpdate(allowMajor)`, `commands.restartApp()`; progress via the `update-download-progress` event |
| CLI | `pluryprint update install [--yes]` — the same install core; exit 11 on failure, 12 when the prompt is declined; on Windows refused (exit 1) while the app is open, and the result printed at installer hand-off (see [ipc-and-cli.md](./ipc-and-cli.md#update-install)) |

The wire shape is `UpdateCheck` (`current`, `latest`, `available`, `url`, `notes`, `checked_at`); `url` and `notes` are populated **only** when `available` is true. See [ipc-and-cli.md](./ipc-and-cli.md) §2.2.

## 6. Channels

```toml
[updater]
channel = "stable"        # "stable" | "beta" | "nightly"
check_on_startup = true
```

Those are the only two `[updater]` keys `settings::UpdaterCfg` defines. There is no `startup_delay_seconds` and no `include_prerelease`; the delay is the `STARTUP_CHECK_DELAY` constant.

**The channel is recorded but not yet honoured.** Every check hits `releases/latest` regardless of the value, because there are no separate beta/nightly feeds. This is stated on the type itself (`UpdaterCfg`'s doc comment) so a reader of the generated bindings sees it too.

## 7. Failure modes

| Failure | Handling |
| ------- | -------- |
| DNS / connection failure | "could not reach github.com — check your connection". Automatic check: silent (`None`). Manual check: shown as `UpdateCheckFailed`; CLI exits 10. |
| `403` / `429` rate limit | "GitHub rate limit reached — try again later". Same silent/loud split. |
| `200` with an unreadable body | Error, never "up to date". |
| `404` on `releases/latest`, repo exists | Normal state: no releases yet, `available = false`. |
| `404` on `releases/latest`, repo missing | Hard error naming the misconfiguration. |
| Unparseable running version or tag | Logged; treated as "not newer". |
| Corrupt `update-state.json` | Logged and ignored; degrades to "check again". |

There is no backoff ladder: the 24 h throttle *is* the rate limiter for the automatic path, and the manual path is user-initiated.

## 8. Relation to ADR-0004

*Resolved (2026-10-09):* the plugin path from ADR [`0004-auto-updater-tauri-builtin`](../adr/0004-auto-updater-tauri-builtin.md) is implemented, with two differences recorded in [ADR-0008](../adr/0008-installer-nsis-per-user.md). The manifest is a GitHub release asset rather than `updates.maecly.com`, and nothing installs without an explicit user action.

## 10. Installing an update

`src-tauri/src/update/install.rs` holds the policy, as pure functions with tests. The plugin does the download, signature check and platform install.

1. **Find.** `find_update` builds an updater for the endpoint from `resolve_endpoint`: `https://github.com/MAECLY/maecly-print/releases/latest/download/latest.json`, or `PLURYPRINT_UPDATE_ENDPOINT` when it is set (local end-to-end testing; release builds still refuse plain `http`). When the variable is set, the *check* (`update check`, About, the launch-time check) reads the same manifest instead of the GitHub releases API, so a test server drives every step (`update::fetch_configured_latest`). Without it, a `404` from a private or deleted repository is reported as `repo_missing` ("the releases could not be reached"), not as a broken build. No newer version gives `installed: null`.
2. **Gate.** `install` refuses a major jump (`is_major_bump`) unless `allow_major` is set, which only the About confirmation dialog or the CLI's `--yes` / typed "yes" provide (FR-087). The confirmation is bound to its version (`check_confirmation`): the About screen asks about the release the GitHub API named and passes that version along, and `install` reads the manifest again, so a major jump to any *other* version (a newer major published in between, or a manifest that differs from the API) is refused with `major_update_changed` until the user confirms it. A typed "yes" in the CLI pins the version the prompt named; `--yes` is a blanket opt-in. Minor and patch updates are not compared. It refuses a binary that is not inside an installed `.app` bundle on macOS (`check_installable_location`), so a dev build never replaces its build directory. One install runs at a time per process.
3. **Download and verify.** `Update::download` streams the artifact (progress goes out as `update-download-progress`) and verifies the minisign signature against `plugins.updater.pubkey` before returning any bytes. Every signature failure maps to `IpcError::UpdateSignatureInvalid`, whose message says the update was **not** installed and nothing changed (`classify_error`).
4. **Install.** Windows runs the per-user NSIS setup passively, and the app exits so the installer can restart it. macOS swaps the `.app` in place and returns `restart_required: true`, and About offers **Restart now** (`restart_app`).

The CLI runs the same steps through `headless_updater_app()`, a windowless Tauri app with only the updater plugin, because the plugin needs an `AppHandle`. Each step is logged (`update_install_started`, `update_download_rejected`, `update_installed`).

Signing, the `latest.json` format and the CI setup are in [ci-cd.md § Updater signing](../07-build-and-release/ci-cd.md#updater-signing).

## 9. Related documents

- [`security-model.md`](./security-model.md) — network egress audit and signing context.
- [`ipc-and-cli.md`](./ipc-and-cli.md) — `UpdateCheck` / `UpdateState` wire types, the `update-available` event, and the `update` subcommand.
- [`../09-legal/privacy.md`](../09-legal/privacy.md) — what this one network call means for privacy.
- ADR [`0004-auto-updater-tauri-builtin`](../adr/0004-auto-updater-tauri-builtin.md) — the superseded-in-practice decision (see §8).
