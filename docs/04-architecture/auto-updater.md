# Update Check

> **TL;DR** — MaeclyPrint ships an **update *check*, not an updater**. It asks the GitHub Releases API (`https://api.github.com/repos/<owner>/<repo>/releases/latest`) whether a newer tag exists, compares it to the running version with semver precedence, and — if so — hands the user a link to the release page. There is **no download, no install, no signature verification and no `tauri-plugin-updater`** in this build. `updates.maecly.com` does not exist and is not contacted.

## 0. Status and scope

| Capability                                | Shipped today | Where |
| ----------------------------------------- | ------------- | ----- |
| Check whether a newer release exists      | ✅            | `src-tauri/src/update/mod.rs` |
| Launch-time check (delayed, throttled)    | ✅            | `src-tauri/src/lib.rs` `setup` |
| Manual check from the About screen        | ✅            | `check_for_updates` IPC command |
| Manual check from a script                | ✅            | `maeclyprint update check [--json]` |
| "Skip this version"                       | ✅            | `set_skipped_update_version`, FR-084 |
| Download an update                        | ❌            | — |
| Install / replace the running binary      | ❌            | `maeclyprint update install` exits 11 with an explanation |
| Signature verification (minisign/Ed25519) | ❌            | nothing is downloaded, so nothing is verified |
| Separate `beta` / `nightly` release feeds | ❌            | the preference is stored; every check hits `releases/latest` |

`tauri-plugin-updater` is **not** a dependency (`src-tauri/Cargo.toml`), and `tauri.conf.json` has no `plugins.updater` block, no `endpoints` and no `pubkey`. The sections that described those are gone from this document; see §7 for the open decision this leaves.

## 1. Why a read-only check

The check is deliberately the smallest thing that answers the only question a user actually asks — "am I running the current version?" — without owning a release CDN, a signing key, or an in-process binary-replacement path. Distribution is GitHub Releases; the user follows the link and installs the artifact the same way they installed the app.

Everything except the one network call is a pure function with co-located tests (`repo_slug` / `parse_release` / `is_newer` / `evaluate` / `interpret_response` / `interpret_repo_probe` / `due_for_check` / `should_check_on_startup` / `should_notify`). `fetch_latest` is the only part that touches the network and is kept tiny so the rest is reasoned about — and tested — entirely offline.

## 2. The request

```
GET https://api.github.com/repos/{owner}/{repo}/releases/latest
User-Agent: MaeclyPrint/<version>
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
| CLI | `maeclyprint update check [--json]` — the same `evaluate()` over the same feed |
| CLI | `maeclyprint update channel [stable\|beta\|nightly]` — reads/writes `updater.channel` in `config.toml` |
| CLI | `maeclyprint update install [--yes]` — reports exit code 11 with "installing updates is not available in this version" |

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

## 8. Open decision (needs a human)

ADR [`0004-auto-updater-tauri-builtin`](../adr/0004-auto-updater-tauri-builtin.md) records the decision to use `tauri-plugin-updater` with a signed manifest on `updates.maecly.com`. **The implementation does not do that**, and this document now describes the implementation. ADRs are not rewritten, they are superseded — so either:

- a new ADR supersedes 0004 and records "read-only check against GitHub Releases" as the standing decision, or
- the plugin path is actually implemented and this document is rewritten back toward 0004.

Until one of those happens, ADR-0004 and this file disagree by design, and that disagreement is the open item — not a doc bug to be patched away. The same applies to the release-pipeline documents that still describe minisign manifest signing.

## 9. Related documents

- [`security-model.md`](./security-model.md) — network egress audit and signing context.
- [`ipc-and-cli.md`](./ipc-and-cli.md) — `UpdateCheck` / `UpdateState` wire types, the `update-available` event, and the `update` subcommand.
- [`../09-legal/privacy.md`](../09-legal/privacy.md) — what this one network call means for privacy.
- ADR [`0004-auto-updater-tauri-builtin`](../adr/0004-auto-updater-tauri-builtin.md) — the superseded-in-practice decision (see §8).
