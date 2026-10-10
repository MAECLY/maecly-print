# Replaying the MaeclyPrint → Pluryprint rename

`feat/rename-pluryprint` was cut from `71fbb3f`; `feat/rename-pluryprint-v2`
replays it onto `ee83c1b` (see "Replayed onto `ee83c1b`" below). When the base moves (for
example onto `feat/page-completeness-campaign`), do not rebase the mechanical
commits: they touch hundreds of lines that the new base also changed. Replay
the rename instead. It has two parts:

1. a **mechanical** text rewrite, which `rename.pl` does again on whatever the
   new base contains;
2. a short list of **manual** edits. Each lives in its own commit on this
   branch and can be cherry-picked on top of the mechanical step.

The decisions themselves are in `docs/adr/0009-rename-to-pluryprint.md`.

## 0. Start

```sh
git switch -c feat/rename-pluryprint-v2 <new-base>
git checkout feat/rename-pluryprint -- scripts/rename   # this folder
pnpm install --frozen-lockfile
```

Run the mechanical step **before** any manual commit lands. The manual commits
add code that has to keep the old name (`util/legacy.rs`, the policy fallback,
`legacyStorage.ts`, the NSIS hook, ADR-0009, `TRADEMARKS.md`). `rename.pl` would
rewrite those literals.

## 1. Mechanical rewrite

```sh
scripts/rename/files.sh | xargs -0 perl scripts/rename/rename.pl
git mv src-tauri/src/bin/maeclyprint-cli.rs src-tauri/src/bin/pluryprint-cli.rs
```

`files.sh` lists every tracked text file that mentions `maecly` in any case,
minus these exclusions (the "keeps"):

| Excluded                          | Why                                                                                                                                     |
| --------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------- |
| `CHANGELOG.md`                    | History. It gets one new Unreleased entry by hand (step 2).                                                                             |
| `docs/adr/0*.md`                  | ADRs are append-only. The rename is ADR-0009. (`docs/adr/README.md` is not excluded; its TL;DR names the product.)                      |
| `claude design/**`                | The archived design hand-off. Code comments cite its path `claude design/maeclyprint/…`, which `rename.pl` protects with a look-behind. |
| `site/**`                         | The landing page belongs to another branch.                                                                                             |
| `**/Cargo.lock`, `pnpm-lock.yaml` | Regenerated in step 3.                                                                                                                  |
| `scripts/rename/**`               | This tool.                                                                                                                              |

`rename.pl` applies these substitutions, in this order, to every line of every
listed file. The order matters: the specific forms run before the generic ones.

| #   | Pattern (Perl regex)             | Replacement                      | What it catches                                                                                                                                                                                   |
| --- | -------------------------------- | -------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 1   | `github\.com/maecly/maeclyprint` | `github.com/MAECLY/maecly-print` | wrong links in `.github/ISSUE_TEMPLATE/config.yml` and `docs/05-features/about.md`                                                                                                                |
| 2   | `com\.maecly\.maeclyprint`       | `com.maecly.pluryprint`          | the docs' bundle id, `src/test/fixtures.ts`                                                                                                                                                       |
| 3   | `com\.maecly\.print\b`           | `com.maecly.pluryprint`          | `tauri.conf.json` identifier, `group.com.maecly.print` in `shell-ext-mac/README.md`                                                                                                               |
| 4   | `Maecly Print`                   | `Pluryprint App`                 | fixture install paths in `src/lib/format.test.ts`, `src/screens/HomeScreen.test.tsx`, `shell-ext-win/src/launch.rs`. Those tests are about a path **with a space**, so the replacement keeps one. |
| 5   | `print_with_maecly_print`        | `print_with_pluryprint`          | the Finder service selector in `platform/macos/services.rs`                                                                                                                                       |
| 6   | `MaeclyPrint`                    | `Pluryprint`                     | display name and every CamelCase identifier (`MaeclyPrintServicesProvider`, `printWithMaeclyPrint`, the ProjectDirs `APP`, `com.Maecly.MaeclyPrint`, `Policies\Maecly\MaeclyPrint`)               |
| 7   | `(?<!design/)maeclyprint`        | `pluryprint`                     | crate, binaries, CLI name, log/temp prefixes, `maeclyprint:` storage keys, npm name                                                                                                               |
| 8   | `maeclyprofile`                  | `pluryprintprofile`              | the e2e fake backend's save path and filter                                                                                                                                                       |
| 9   | `MAECLYPRINT`                    | `PLURYPRINT`                     | env vars (`*_UPDATE_ENDPOINT`, `*_REQUIRE_RESOURCES`, `*_PRINTER_NAME/URL`), `CLSID_MAECLYPRINT_COMMAND`                                                                                          |
| 10  | `\.maecly-profile\.toml`         | `.pluryprint-profile.toml`       | exported profile file name                                                                                                                                                                        |
| 11  | `__MAECLY_E2E__`                 | `__PLURY_E2E__`                  | e2e browser stub global                                                                                                                                                                           |
| 12  | `MAECLY_E2E_STUB`                | `PLURY_E2E_STUB`                 | e2e Vite define                                                                                                                                                                                   |
| 13  | `maeclyE2eStub`                  | `pluryE2eStub`                   | e2e Vite plugin                                                                                                                                                                                   |
| 14  | `maecly-e2e`                     | `plury-e2e`                      | e2e plugin name and temp prefixes                                                                                                                                                                 |
| 15  | `maecly-track-test`              | `pluryprint-track-test`          | ipptool job name in `engine/print/cups.rs`                                                                                                                                                        |
| 16  | `\.maecly-tmp\b`                 | `.pluryprint-tmp`                | atomic-write temporary for a Chromium `Local State` in `util/chromium.rs`                                                                                                                         |

Left alone on purpose (no pattern touches them): the repo URL and slug
`MAECLY/maecly-print` / `maecly-print`, the org name `Maecly` (ProjectDirs org,
`publisher`, `CN=Maecly`, `Policies\Maecly\`), `maecly.com` and its subdomains,
`@maecly-maintainers`, the CUPS test queues `Maecly_Hold*`, the test env vars
`MAECLY_REAL_PRINTER` / `MAECLY_HOLD_QUEUE`, the `maecly-real-printer-*` job
titles, the `'maecly'` icon key in `src/components/Icon.tsx`, and the CLSID GUID.

## 2. Manual edits

Cherry-pick these commits from `feat/rename-pluryprint`, in this order, after
step 1. Each holds manual edits only: its diff was taken on top of the
mechanical rewrite.

| Commit    | Subject                                                         | Edits                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
| --------- | --------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `743601e` | feat(core): migrate MaeclyPrint data dirs…                      | new `src-tauri/src/util/legacy.rs`; `pub mod legacy;` in `util/mod.rs`; `migrate_current()` before `logging::init()` + `log_report` after it in `lib.rs::run_with`, and the same around `init_cli()` in `cli/mod.rs::execute`; `PLURYPRINT_SKIP_MIGRATION=1` in `tests/cli_console_binary.rs`                                                                                                                                                                                               |
| `2e15cb9` | feat(core): read deprecated MAECLYPRINT\_\* env vars…           | `update/install.rs::endpoint()` reads through `util::legacy::env_var`; `shell-ext-win/build.rs` falls back to `MAECLYPRINT_REQUIRE_RESOURCES`                                                                                                                                                                                                                                                                                                                                               |
| `d600aab` | feat(core): honour admin policies deployed for MaeclyPrint      | `settings/policy.rs`: `APP_DIR`/`LEGACY_APP_DIR`, `system_policy_path(app)`, `is_legacy`, `win::PolicyKey`, warning, test                                                                                                                                                                                                                                                                                                                                                                   |
| `c00ce75` | test(core): keep importing … .maecly-profile.toml               | one test in `profiles/mod.rs`                                                                                                                                                                                                                                                                                                                                                                                                                                                               |
| `047d883` | feat(ui): draw the Pluryprint wordmark…                         | `Brand.tsx` `MPWordmark`: `Plury<span accent>print</span>`; About header byline; `about.json` `byline` EN "by Maecly" / ES "por Maecly"; About test                                                                                                                                                                                                                                                                                                                                         |
| `b68f52b` | feat(ui): move maeclyprint:\* localStorage keys…                | new `src/lib/legacyStorage.ts` + test; call in `src/main.tsx` before `bootAppearance()`                                                                                                                                                                                                                                                                                                                                                                                                     |
| `9b44e04` | test(ui): give the app-info fixture the real macOS folder names | `src/test/fixtures.ts`: `com.Maecly.Pluryprint`, `~/Library/Logs/Pluryprint`                                                                                                                                                                                                                                                                                                                                                                                                                |
| `c09628f` | feat(build): take over a MaeclyPrint install…                   | new `src-tauri/windows/installer-hooks.nsh`; `installerHooks` in `tauri.conf.json`; new `src-tauri/tests/bundle_identity.rs`                                                                                                                                                                                                                                                                                                                                                                |
| `0fbd90c` | docs: record the rename…                                        | new `docs/adr/0009-rename-to-pluryprint.md`, ADR index row, `TRADEMARKS.md`, CHANGELOG entry (header line + first bullet under Unreleased → Changed), migration paragraphs in `data-model.md` §1, `privacy.md` and `installer-windows.md`, ownership lines and the fork-name example in `docs/09-legal/trademark.md`                                                                                                                                                                        |
| `ad7ce98` | test(core): expect notes.txt first…                             | `logging/mod.rs` test `fr_113_sweep_drops_files_older_than_retain_days`: `pluryprint-*` sorts after `notes.txt`                                                                                                                                                                                                                                                                                                                                                                             |
| `620d9eb` | fix(build): wait for the MaeclyPrint uninstaller…               | `src-tauri/windows/installer-hooks.nsh` rewritten (unquoted folder from `HKCU\Software\Maecly\MaeclyPrint`, `_?=` on every run, cleanup gated on success, per-machine MSI offer); four new tests in `tests/bundle_identity.rs` (one compiles the hook with `makensis -WX` when installed); `installerHooks` bullet in `installer-windows.md`                                                                                                                                                |
| `070c876` | fix(core): never leave a truncated file behind…                 | `util/legacy.rs`: `Ops`, `merge_with`, `move_entry_with` (copy to a hidden temporary, rename into place), `copy_recursive` takes `ops`; three tests                                                                                                                                                                                                                                                                                                                                         |
| `e67f48e` | feat(core): wait a bounded time for a lock…                     | `cli/instance.rs`: new `claim_within`                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
| `c65bc8d` | fix(core): migrate MaeclyPrint data under locks…                | `util/legacy.rs`: `Layout` gains `new_lock`, `migration_lock`, `marker`; `Report::backed_up`; `Migration` + `migrate_with` (migration lock, Pluryprint lock probe, old lock held for the move, marker); `is_first_run_default`, `replace_first_run_default`; `migrate_current` returns `Migration`; `settings/mod.rs`: `Config::is_first_run_default`; `lib.rs::run_with` keeps the guard until after the instance-lock claim in `setup`; `cli/mod.rs::execute` drops it after `log_report` |
| `c810f06` | feat(core): keep the first-run flags in config.toml…            | `settings/mod.rs`: `AppCfg::locale_detected` / `onboarding_complete` (+ `Default`, `is_first_run_default`, tests); `ipc/settings.rs` test literal; `util/legacy.rs`: `Layout::new_config`, `record_upgrade`, tests; `src/ipc/bindings.ts` is regenerated in step 3                                                                                                                                                                                                                          |
| `393973b` | feat(ui): read the first-run flags from config.toml…            | `I18nProvider.tsx` (config flag first, write `locale_detected` with the detected locale), `App.tsx` (`showOnboarding` waits for the config, `completeOnboarding` patches it), `SettingsScreen.tsx` (language change sets `locale_detected`), `legacyStorage.ts` doc; tests in `App.test.tsx`, `I18nProvider.test.tsx`, `SettingsScreen.test.tsx`                                                                                                                                            |
| `78f3501` | fix(core): let every new policy location beat every legacy one  | `settings/policy.rs`: `Source`, `Found`, `sources()`, `resolve`, `read_source`, `registry_policy(legacy)`; `policy_paths` removed; tests                                                                                                                                                                                                                                                                                                                                                    |
| `0bbc55e` | docs: describe the locked, one-time migration…                  | `data-model.md` §1 "Upgrading from MaeclyPrint", ADR-0009 Decision and Consequences bullets                                                                                                                                                                                                                                                                                                                                                                                                 |
| `ad84cf8` | docs(shell): require removing a MaeclyPrint sparse package…     | `shell-ext-win/README.md`, comment in `shell-ext-win/res/shellext.manifest`, `shell-extensions.md` §"Registering the sparse package" and uninstall snippet, ADR-0009 bullet                                                                                                                                                                                                                                                                                                                 |
| `b4c3ff4` | chore(license): relicense under AGPL-3.0-only                   | `LICENSE` becomes the AGPL-3.0 text (the GPL-3.0 text comes back as `LICENSES/GPL-3.0.txt` in `702ee1c`); `AGPL-3.0-only` in `package.json`, both `Cargo.toml`, `shellext.rc` `LegalCopyright`; `tauri.conf.json` `copyright`; `repo.ts` `APP_LICENSE`; `about.json` EN/ES `links.license`; About test; `testpage.rs` footer + test                                                                                                                                                         |
| `5295acf` | docs(legal): take contributions under a CLA…                    | new `CLA.md`; `CONTRIBUTING.md`; `PULL_REQUEST_TEMPLATE.md` CLA checkbox; `contributing.md`, `coding-standards.md`                                                                                                                                                                                                                                                                                                                                                                          |
| `31b2a6e` | ci: drop the DCO sign-off check                                 | deletes `.github/workflows/dco.yml`; `ci-cd.md`, `tech-stack.md`, `repo-structure.md`                                                                                                                                                                                                                                                                                                                                                                                                       |
| `22142b3` | docs(legal): state the 7(e) trademark term under the AGPL       | `TRADEMARKS.md` licence bullet; `docs/09-legal/trademark.md`. Needs `0fbd90c` (`TRADEMARKS.md`)                                                                                                                                                                                                                                                                                                                                                                                             |
| `702ee1c` | feat(legal): ship third-party notices and licence texts…        | new `THIRD-PARTY-NOTICES.md`, `LICENSES/` (`GPL-3.0.txt`, `Apache-2.0.txt`, `OFL-1.1.txt`), `src-tauri/tests/licence_bundle.rs`; `bundle.resources` becomes a map; `third-party-licenses.md`, installer docs, `repo-structure.md`, fonts and SumatraPDF READMEs, `TRADEMARKS.md` notices line. If the new base changed dependencies, rerun the commands in the notices' "How this list was made" and update the counts and lists                                                            |
| `2d58042` | feat(ui): link the third-party notices and the CLA from About   | `repo.ts` `CLA_URL`, `NOTICES_URL`; two buttons in `AboutScreen.tsx`; `about.json` EN/ES `links.notices`, `links.cla`; About tests; `docs/05-features/about.md`                                                                                                                                                                                                                                                                                                                             |
| `5098d26` | docs(adr): record the relicense to AGPL-3.0 as ADR-0010         | new `docs/adr/0010-relicense-to-agpl-3.0.md`; ADR-0002 status line; ADR index row; `docs/README.md` decisions table and ADR list; `milestones.md` M0; CHANGELOG Unreleased → Changed first bullet (above the rename bullet)                                                                                                                                                                                                                                                                 |
| `ca962e2` | docs: describe Pluryprint as AGPL-3.0 everywhere                | `README.md` badge, License and Contributing; `docs/09-legal/license.md` rewritten; GPLv3 → AGPL-3.0 in vision, research, NFR-100/101, `security-model.md`, `tech-stack.md`, `future-features.md`, `milestones.md`; `release-checklist.md` notices step                                                                                                                                                                                                                                      |

The last eight rows are the relicense to AGPL-3.0 (ADR-0010). Pick them after
the rename's own commits: they edit `TRADEMARKS.md` and the CHANGELOG entry that
`0fbd90c` adds. ADR-0010 and `docs/adr/0002` are ADRs, so `rename.pl` never
touches them; nothing in these commits uses the old name.

One manual edit is inside the mechanical commit `5b524e4` and has to be redone
by hand: in `src-tauri/src/platform/testpage.rs::render`, the wordmark's two
`TextItem::Text` strings become `"Plury"` (ink) and `"print"` (accent), replacing
`"Maecly"` and `"Print"`.

A conflict in a cherry-pick means the new base changed the same lines; keep the
base's change and re-apply the edit described in the table.

## 3. Regenerate

```sh
cargo check --manifest-path src-tauri/Cargo.toml        # Cargo.lock: package pluryprint
cargo check --manifest-path shell-ext-win/Cargo.toml    # Cargo.lock: pluryprint-shellext
cargo fmt --manifest-path src-tauri/Cargo.toml
cargo fmt --manifest-path shell-ext-win/Cargo.toml
pnpm exec prettier --write src e2e
PLURYPRINT_SKIP_MIGRATION=1 cargo test --manifest-path src-tauri/Cargo.toml   # also rewrites src/ipc/bindings.ts
```

Commit any change to `src/ipc/bindings.ts` and the lockfiles.

`PLURYPRINT_SKIP_MIGRATION=1` keeps a test run from moving a development
machine's real MaeclyPrint folders, which a pre-rename checkout still uses.

## 4. Verify

```sh
pnpm lint && pnpm typecheck && pnpm test && pnpm format:check
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo clippy --manifest-path shell-ext-win/Cargo.toml --all-targets -- -D warnings
pnpm tauri build --debug --bundles app --config src-tauri/tauri.ci.conf.json
PLURYPRINT_SKIP_MIGRATION=1 cargo test --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path shell-ext-win/Cargo.toml
git grep -niE 'maecly ?print|maecly-print|MAECLYPRINT'
```

Run the built CLI against a throwaway `HOME`, never your own:
`HOME=$(mktemp -d) src-tauri/target/debug/bundle/macos/Pluryprint.app/Contents/MacOS/pluryprint printers list --json`.

The leftover grep must show only the intentional keeps:

- the repo URL / slug `MAECLY/maecly-print`, `<org>/maecly-print`, `cd maecly-print`, and the `("MAECLY", "maecly-print")` slug tests in `update/mod.rs`;
- `CHANGELOG.md` history, `docs/adr/0001`–`0008`, and the `claude design/maeclyprint/` paths cited in `Icon.tsx`, `format.ts`, `tokens.css`, `tailwind.config.ts`;
- the migration and fallback code and its docs: `src-tauri/src/util/legacy.rs`, `settings/policy.rs`, `update/install.rs` (doc comment), `lib.rs` and `cli/mod.rs` (comments), `profiles/mod.rs` (legacy import test), `shell-ext-win/build.rs`, `src/lib/legacyStorage.ts` (+ test), `src/main.tsx` and `src/i18n/I18nProvider.tsx` (comments), `src-tauri/windows/installer-hooks.nsh`, `src-tauri/tests/bundle_identity.rs`, `src-tauri/tests/installer_hooks.rs` (comment), `tests/cli_console_binary.rs` (comment), ADR-0009 and its index row, `TRADEMARKS.md`, the migration paragraphs in `data-model.md`, `privacy.md` and `installer-windows.md`;
- the old sparse-package identity that registration must remove first: `shell-ext-win/README.md`, the comment in `shell-ext-win/res/shellext.manifest`, `docs/04-architecture/shell-extensions.md`;
- this folder.

A mention anywhere else is something the new base added after `ee83c1b`: run
step 1 on that file, or edit it by hand if it is one of the keeps above.

## Replayed onto `ee83c1b`

The base `feat/page-completeness-campaign` had gained the Windows context-menu
installer (`src-tauri/windows/installer-hooks.nsh`, wired as `installerHooks`),
`scripts/build-shell-ext.ps1`, `scripts/check-failed-logons.ps1`,
`util/chromium.rs` and `#![deny(missing_docs)]`. Replaying on it:

- Step 1 renamed those files too. `rename.pl` gained rule 16 for the
  `Local State.maecly-tmp` temporary in `util/chromium.rs`. The WebView2
  profile seeded at start (`%LOCALAPPDATA%\<identifier>\EBWebView`) follows
  the identifier from `tauri.conf.json`, so it needed no edit.
- There is **one** hooks file. The takeover's `NSIS_HOOK_PREINSTALL` (with its
  defines and `PLURY_UNQUOTE`) sits in `src-tauri/windows/installer-hooks.nsh`
  between the `; ---- Taking over a MaeclyPrint install` markers, before the
  context-menu `NSIS_HOOK_POSTINSTALL`; `installers/windows/nsis/hooks.nsh` is
  gone and `installerHooks` stays `windows/installer-hooks.nsh`.
  `bundle_identity.rs` reads that file, and `nsis_hook_compiles` copies it to a
  temporary folder beside a placeholder `shell_ext_win.dll` (the file stops
  with `!error` without one) and compiles all four hooks with `makensis -WX`.
- `installer_hooks.rs` checks HKCU-only on `WriteReg*` lines: the takeover
  reads HKLM (the per-machine MSI) and deletes a legacy `ContextMenuHandlers`
  key, neither of which registers anything.
- Conflicts kept both sides: `Config::is_first_run_default` next to the
  documented `Config::config_path`; the per-architecture SumatraPDF pins and
  licence files from the base with the GPL-3.0/MuPDF wording, `LICENSES/` and
  the `bundle.resources` map from the relicense (`third-party-licenses.md`,
  `THIRD-PARTY-NOTICES.md`, the SumatraPDF README, `security-model.md`).
- Three commits follow the cherry-picks: docs on `DirPair` and `Migration`
  fields (`missing_docs`), the `installer_hooks.rs` change above, and the
  regenerated `src/ipc/bindings.ts`. Commits between `c810f06`'s replay and the
  bindings commit carry the base's `bindings.ts`.
