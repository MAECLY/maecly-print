# Testing Strategy

> **TL;DR.** Test pyramid: many fast Rust unit tests, a thin layer of Rust integration binaries that exercise real on-disk paths, and a small WebdriverIO end-to-end suite driving the built GUI (Windows only in CI). Coverage is **gated in CI**: `cargo llvm-cov --fail-under-lines 70` on the Rust side and explicit `--coverage.thresholds.*` flags on the Vitest side. Requirement-tied tests carry an `FR-###` / `NFR-###` tag in their name.

This document covers what we test, how, with which tools, and what the acceptance bar looks like.

**Scope note.** Sections below distinguish what exists today from what is planned. The current suite prints nothing: there is no virtual-printer harness, no PDF-output comparison, no benchmark suite, and no subprocess-converter tests, because the converters those would exercise (LibreOffice, Chromium, Pandoc) are not wired into the engine yet. Everything marked *Planned* is a design commitment, not a description of the tree.

## The pyramid

```
                ┌──────────────┐
                │  Manual QA   │  Pre-release smoke; humans only
                ├──────────────┤
                │     E2E      │  WebdriverIO + tauri-driver (Win/Linux only)
                ├──────────────┤
                │ Integration  │  ~90 test fns across 8 binaries in src-tauri/tests/
                ├──────────────┤
                │     Unit     │  ~340 Rust fns co-located + ~405 Vitest cases
                └──────────────┘
```

(Counts are `#[test]`/`#[tokio::test]` functions under `src-tauri/src` and `src-tauri/tests`, plus `it(...)` cases across the 24 `*.test.ts(x)` files under `src/`. The standalone `shell-ext-win/` crate adds ~27 more Rust tests in its own lane.)

Most bugs are caught by unit tests (cheap, fast). A smaller layer of integration tests catches the ones that span process and filesystem boundaries. E2E tests catch GUI integration issues. Manual QA handles things that resist automation (does it look right? does it feel right?).

## Rust unit tests

### Tooling

- `cargo test` (the standard). This is the whole toolchain today — the crate has no test-only dependency beyond `tempfile` (a normal dependency, also used at runtime) and `tokio`'s `test-util` feature, which unlocks the virtual clock (`start_paused` / `time::advance`) the dispatcher's retry-backoff tests need to stay deterministic and instant.
- Optional: `cargo nextest` for faster local runs (3–5x speedup). Not used in CI.
- **Not used:** `insta` (snapshots), `proptest` (property tests), `mockall`, `wiremock`. None is in `src-tauri/Cargo.toml`. Adding one is a normal dependency decision, not a documented default.

### Conventions

- Co-locate with source: `#[cfg(test)] mod tests { ... }` at the bottom of the module file.
- One test per behavior. Don't pack 5 assertions into one test if you can split.
- Test name: `test_<scenario>_<expected_result>`. Examples:
  - `test_parse_profile_minimal_succeeds`
  - `test_parse_profile_missing_required_field_errors`
  - `test_enqueue_when_full_returns_full_error`
- Avoid `should_panic` tests except for verifying explicit `panic!()` messages. Prefer `assert!(matches!(...))` on `Result::Err`.

### Mocks vs fakes

We prefer **hand-rolled fakes** over mocking libraries.

- A **fake** is a real implementation of a trait that's simpler than production (e.g. an in-memory file store).
- A **mock** is a test-time stub of arbitrary call sequences (e.g. `mockall`).

Fakes are easier to debug, easier to maintain, and force good interface design. We use mocking libraries only when the fake would be larger than 200 LOC.

Example:

```rust
// engine/router.rs
trait PrinterAdapter: Send + Sync {
    async fn print(&self, file: &Path, opts: &PrintOpts) -> Result<()>;
}

// In tests:
struct FakePrinter {
    captured: Mutex<Vec<(PathBuf, PrintOpts)>>,
}

impl PrinterAdapter for FakePrinter {
    async fn print(&self, file: &Path, opts: &PrintOpts) -> Result<()> {
        self.captured.lock().unwrap().push((file.to_owned(), opts.clone()));
        Ok(())
    }
}

#[tokio::test]
async fn test_router_dispatches_pdf_to_pdf_adapter() {
    let printer = Arc::new(FakePrinter::default());
    let router = Router::new(printer.clone());
    router.print("test.pdf").await.unwrap();
    assert_eq!(printer.captured.lock().unwrap().len(), 1);
}
```

### Coverage target

**70% line coverage** on Rust code, and it is **enforced**. The `rust-coverage` job in `ci.yml` runs on `macos-latest` (so the CUPS backend is compiled and exercised) and fails the run below the threshold:

```bash
cargo llvm-cov --manifest-path src-tauri/Cargo.toml \
  --lcov --output-path lcov.info \
  --fail-under-lines 70
```

Locally, `cargo install cargo-llvm-cov --locked && cargo llvm-cov --html` gives the same numbers with a browsable report.

Measured on that lane at the time of writing: **80.4% lines** (82.4% regions, 67.8% functions). The threshold sits below the measurement on purpose — it is a regression gate, not a target — and is raised as the thinnest modules (`queue/mod.rs` ≈66%, `update/mod.rs` ≈78%) fill in.

Coverage is a sanity check, not a goal. We don't write tests purely to chase coverage; we write tests for behaviors that matter.

Excluded from coverage:

- Win32 / Apple FFI calls (hard to unit-test; covered by integration tests).
- `unreachable!()` arms.
- Trivial getters / setters.
- Generated code (e.g. from `build.rs`).

## Rust integration tests

### Location

`src-tauri/tests/`. Each `.rs` file is a separate test binary, which means each gets its own clean environment. Eight binaries exist today:

```
src-tauri/tests/
├── cli_contract.rs       # The headless surface: subcommand/flag names and the
│                         #   exit-code contract (FR-008/009/112)
├── engine_deps.rs        # Dependency resolution order and the remediation
│                         #   messages a missing program produces (FR-058)
├── folder_expansion.rs   # Walking a dropped directory: limits, skip reasons
│                         #   (FR-004/005/007/059)
├── ipc_surface.rs        # The tauri-specta binding drift gate — regenerates
│                         #   src/ipc/bindings.ts and fails if the commit is stale
├── print_timeout.rs      # A hung child must not wedge the queue (NFR-012)
├── profile_roundtrip.rs  # FR-020/021/022/026/027 — CRUD, immutable built-ins,
│                         #   atomic save, 500 profiles, default rules
├── settings_persist.rs   # FR-100/101/102/103 — human-readable TOML, corrupt-file
│                         #   fallback, atomic write, schema_version
└── update_startup.rs     # The startup update check, offline and error paths
                          #   (the live-network variant stays #[ignore]d in-module)
```

There is **no** `pdf_print.rs`, `queue_concurrency.rs` or `shell_handoff.rs`. Print-output verification is unbuilt (see *Print harness — planned* below); queue concurrency and the dispatcher are covered in-module under `src-tauri/src/queue/`; the shell handoff schema is covered by `shell-ext-win/tests/handoff_roundtrip.rs` on the producer side and `src-tauri/src/cli/handoff.rs` on the consumer side.

Test functions that pin a requirement are **named after it** — `fr_020_crud_roundtrip`, `fr_102_atomic_write` — so a failure names the requirement it breaks. Domain logic is unit-tested in-module (`#[cfg(test)]`); these integration binaries exercise the real on-disk read/write paths through a tempdir.

### Tooling

- Standard `cargo test`.
- `tempfile` for ephemeral directories.
- Nothing else. In particular there is **no HTTP mock server**: the update client's parsing, comparison and error paths are unit-tested against fixture payloads, and the one test that really talks to `api.github.com` is `#[ignore]`d (see *Ignored tests* below).

### Virtual printer — planned, not built

**Nothing in the suite prints today.** No test spawns SumatraPDF or `lp`; the print backends are covered as *pure argument construction* — `sumatra::print_settings_string`, `sumatra::sumatra_args`, `cups::lp_opts`, `cups::lp_args` are pure functions with co-located unit tests, and only `spool`/`cancel` touch a process, `cfg`-gated per platform.

The design when we do build it:

- **Windows:** "Microsoft Print to PDF" (preinstalled), output path set via the job's `OutputFile` parameter.
- **macOS:** `cups-pdf` (Homebrew), configured to write `.pdf` to a known location.

Verification would be page count + text layer (deterministic, fast), with an opt-in visual diff at 150 DPI behind an env var. That needs a PDF-reading crate in `dev-dependencies` (`lopdf` is already a normal dependency and can read page counts) and a fixtures tree — neither of which exists yet. **Do not** cite this section as if the harness were available.

### Subprocess fixtures — not applicable yet

CI installs **no** converters. There is no LibreOffice, Pandoc, Chromium or ImageMagick step in `ci.yml`, because the engine does not shell out to any of them: `engine::convert` routes PDFs through untouched and rasterises images with the pure-Rust `image` codecs. `engine::deps` can *locate* LibreOffice/Chromium/Pandoc and reports honestly that finding them changes nothing today.

When a converter is actually wired in, its adapter tests install the subprocess in CI (`choco install libreoffice-fresh` on Windows, `brew install --cask libreoffice` on macOS) and gate the test behind `#[ignore]` so a developer without it still gets a green `cargo test`.

### Ignored tests

Three tests are `#[ignore]`d because they reach outside the process (the comment above the `ignored-tests` job in `ci.yml` still says "two" — it predates `list_printers_smoke`):

| Test | Why | How to run |
|---|---|---|
| `update::tests::live_github_release_check` | Hits `api.github.com`; rate limits and third-party downtime must not fail a merge | `cargo test -- --ignored live_github_release_check` |
| `platform::cups::tests::add_and_remove_real_queue` | Creates a real CUPS queue via `lpadmin` | `cargo test -- --ignored add_and_remove` |
| `platform::cups::tests::list_printers_smoke` | Shells out to the host's CUPS install; the result depends on the developer's real queues | `cargo test -- --ignored list_printers_smoke` |

CI runs them in the **`ignored-tests`** job — `cargo test -- --ignored`, which runs *only* those, on `macos-latest`. That job is `continue-on-error: true` and fires on a weekly schedule (`cron: '0 5 * * 1'`) or `workflow_dispatch`; it never runs on a pull request and never gates a merge. Note the flag: CI uses `-- --ignored` (only the ignored ones — the rest already ran in the blocking lanes), **not** `--include-ignored`.

## Frontend unit tests

### Tooling

- **Vitest** (Vite-native test runner; faster than Jest for our setup), configured under `test:` in `vite.config.ts` — jsdom environment, globals on, `src/test/setup.ts` as the setup file, `include: ['src/**/*.{test,spec}.{ts,tsx}']` and `exclude: [..., 'e2e/**']`.
- **React Testing Library** (`@testing-library/react`) plus `@testing-library/jest-dom` matchers.
- **`@testing-library/user-event`** for simulating user interactions.
- **`mockIPC` from `@tauri-apps/api/mocks`** for the backend boundary — see *Mocking Tauri IPC* below.
- **Not used: MSW.** The frontend makes no HTTP requests of its own; everything crosses the Tauri IPC bridge, which `mockIPC` intercepts directly. There is no `msw` dependency in `package.json` and none is needed.

### Conventions

- Test files co-located: `Button.tsx` → `Button.test.tsx`.
- One `describe` block per component or function.
- Test names in plain English: `it("disables itself when isLoading is true", ...)`.
- Avoid testing implementation details. Test what the user (or component consumer) sees.

```tsx
// src/components/Button.test.tsx
import { describe, it, expect } from 'vitest';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { Button } from './Button';

describe('Button', () => {
  it('calls onClick when clicked', async () => {
    const onClick = vi.fn();
    render(<Button onClick={onClick}>Print</Button>);
    await userEvent.click(screen.getByRole('button', { name: 'Print' }));
    expect(onClick).toHaveBeenCalledOnce();
  });

  it('is disabled when loading', () => {
    render(<Button isLoading>Print</Button>);
    expect(screen.getByRole('button')).toBeDisabled();
  });
});
```

### Mocking Tauri IPC

`src/test/setup.ts` is deliberately thin — it wires `@testing-library/jest-dom` and shims `localStorage` for jsdom's opaque origin. It does **not** globally stub IPC. Instead, each test that needs the bridge installs a **per-test `mockIPC`** from `@tauri-apps/api/mocks` and tears it down with `clearMocks()` in `afterEach`. The handler is usually stateful, so a store's reload-after-write sees a realistic post-mutation snapshot:

```ts
import { mockIPC, clearMocks } from '@tauri-apps/api/mocks';

afterEach(() => clearMocks());

it('FR-022: the default flag moves after set_default_profile', async () => {
  let list = fixtureProfiles;
  mockIPC((cmd, args) => {
    switch (cmd) {
      case 'list_profiles': return list;
      case 'set_default_profile':
        list = list.map((p) => ({ ...p, default: p.id === (args as any).id }));
        return null;
    }
  });
  // …drive the store, assert the reloaded list…
});
```

Event-driven stores need one extra step. The tauri-specta `events` object drives the event plugin over IPC, which `mockIPC` would also intercept, so those tests `vi.mock('@/ipc/bindings')` to swap **just `events`** for a controllable fake (keeping the real `commands` so `mockIPC` still serves command invokes). `vi.hoisted` makes the fake registry available inside the hoisted `vi.mock` factory. This is the pattern in `queueStore.test.ts`.

### Coverage target

**60% line coverage** on TypeScript is the target. Lower than Rust because UI code has more that is hard to test (rendering details, Tailwind utility usage). We don't chase the number; we cover behaviors.

Measured by `pnpm test:coverage` (`vitest run --coverage`) with the `@vitest/coverage-v8` provider — reporters `text`/`html`/`lcov`, output under `coverage/`, non-product files (generated `bindings.ts`, `src/test/**`, `src/main.tsx`, configs) excluded from the denominator (`vite.config.ts`).

**CI enforces a floor.** The `frontend` job passes explicit thresholds on the command line rather than in `vite.config.ts`, so the gate stays CI-owned and a local `pnpm test:coverage` never fails on a threshold:

```
pnpm test:coverage \
  --coverage.thresholds.lines=55 \
  --coverage.thresholds.statements=55 \
  --coverage.thresholds.functions=65 \
  --coverage.thresholds.branches=75
```

Those numbers sit just under the measured tree (lines/statements 61.4%, functions 71.4%, branches 79.8%) — a regression gate, not the target. The 60% target is what they are raised toward; the report is also uploaded as the `frontend-coverage` artifact (7-day retention) either way.

> The comment in `vite.config.ts` still describes coverage as "informational, not gated". That comment is stale with respect to `ci.yml`; the CI flags above are authoritative.

## End-to-end tests

### Tooling

- **WebdriverIO** as the test framework (`@wdio/cli` + the Mocha framework adapter), configured in `e2e/wdio.conf.ts`.
- **`tauri-driver`** (a cargo binary, `cargo install tauri-driver --locked`) as the WebDriver bridge to the Tauri webview. On Windows it proxies to the Edge WebDriver (`msedgedriver.exe`) matching the installed WebView2, passed via the `NATIVE_DRIVER` env var.
- Tests run against an actual built `maeclyprint(.exe)` binary (`pnpm tauri build --debug`), not the dev server. Run locally with `pnpm test:e2e`.

### Platform support — Windows/Linux only

`tauri-driver` supports **Windows and Linux only**; **macOS WKWebView exposes no WebDriver endpoint**, so there is no automated E2E on macOS. macOS is covered instead by the Rust unit/integration suite, the Vitest suite, a **launch smoke** ("does the built app start and stay up"), and manual QA. A parallel **launch smoke** also runs on Windows.

### Coverage scope

E2E specs live in `e2e/specs/` and are named **`*.e2e.ts`**, not `*.spec.ts`. The extension is load-bearing: Vitest's default `{test,spec}` glob would otherwise pick them up and try to run a WebdriverIO suite inside jsdom. `wdio.conf.ts` matches them with `specs: [path.resolve(here, 'specs', '**', '*.e2e.ts')]`.

There is one spec per screen — seven files plus a shared `helpers.ts` (dismiss onboarding, navigate the sidebar):

| Spec | Covers |
|---|---|
| `home.e2e.ts` | Home advertises only formats the engine really accepts; the global shortcut layer is bound to a real window (NFR-034); quick actions are focusable buttons (NFR-041); recent activity is fed by a live `queue_status` |
| `print-job.e2e.ts` | Add a file, submit the batch, see it in the Queue; an unsupported file is refused with a visible reason (FR-005); a dropped folder asks before recursing (FR-004) |
| `queue.e2e.ts` | The live queue renders instead of an "unavailable" notice; switching List / Board / Timeline views; the queue controls, including an honest "Clear done" |
| `printers.e2e.ts` | Enumeration comes from the host spooler, not mock data; the add-printer methods are tabs and honour the Windows transport limit |
| `profiles.e2e.ts` | Create a custom profile and make it the default; unsaved edits survive leaving and returning to the screen |
| `settings.e2e.ts` | Theme switch applies the dark class; a changed setting survives a relaunch with the local cache cleared (FR-100); language switching re-renders and persists (FR-094) |
| `about.e2e.ts` | Semver version string; license/release/docs/issue links (FR-110); the live log directory (FR-113); React/Tauri versions and host platform; the update check is enabled inside the real host and the IPC-unavailable fallback never shows |

There is no `launch.e2e.ts` or `navigation.e2e.ts`: "the binary starts and paints" is what the **smoke launch** CI steps assert (cheaper, and they also run on macOS where WebDriver is unavailable), and navigation is exercised by `helpers.navigateTo` in every other spec.

What E2E tests don't cover (we have lower-level tests for these): component-level UI behavior (Vitest), print correctness (Rust unit tests over the flag builders), and the IPC contract (`src-tauri/tests/ipc_surface.rs`).

### Test structure

```ts
// e2e/specs/about.e2e.ts — WebdriverIO + Mocha globals
import { $, expect } from '@wdio/globals';
import { dismissOnboarding, navigateTo } from './helpers.js';

describe('About', () => {
  beforeEach(async () => {
    await dismissOnboarding();
    await navigateTo('About'); // takes the sidebar label
  });

  it('shows a semver version string', async () => {
    await expect($('body')).toHaveText(/\d+\.\d+\.\d+/);
  });
});
```

### CI integration

GUI verification runs in three places, none of which gates a merge, because driving a real WebView2 GUI (or launching a GUI on a headless runner) is inherently flake-prone:

- **Smoke launch (Windows)** — a step inside the `build` job. Launches the packaged debug binary against a sandboxed `APPDATA`/`LOCALAPPDATA`, then polls the JSONL log for `app_started` **and** `main_window_resolved`, 60 s deadline. The log is uploaded as `windows-smoke-log` on failure. This step *does* fail the `build` job — it is the one GUI assertion that is blocking.
- **Smoke launch (macOS)** — the same idea in the `macos` job, but both the `pnpm tauri build --debug` and the launch step are `continue-on-error: true`: WKWebView has no WebDriver and a headless runner has no guaranteed window server.
- **E2E (WebdriverIO + tauri-driver)** — the whole `e2e/specs/**` suite on `windows-latest`, `timeout-minutes: 35`, `continue-on-error: true`. It installs `tauri-driver`, reads the installed WebView2 version out of the EdgeUpdate registry key, downloads the matching `msedgedriver` and exports it as `NATIVE_DRIVER`, then builds the frontend and `cargo build`s the binary directly (cheaper than a full `tauri build`, which would also produce installers the run does not need). Failure screenshots are uploaded as `e2e-artifacts`.

The blocking gates are `frontend` (lint, format, typecheck, coverage-gated Vitest), `backend` (`cargo fmt`/`clippy -D warnings`/`cargo test` + the bindings-drift check on Windows), `macos` (clippy + `cargo test` + `pnpm test`), `rust-coverage`, `shell-ext-win` and `shell-ext-portable`, and `build`.

Flake handling: a spec that fails repeatedly on `main` is fixed within a week or removed — we don't carry flaky GUI tests forward.

### The full job list in `ci.yml`

| Job | Runner | Blocking? | What it does |
|---|---|---|---|
| `frontend` | ubuntu | yes | `pnpm lint`, `format:check`, `typecheck`, `test:coverage` with threshold flags; uploads `frontend-coverage` |
| `backend` | windows | yes | `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`, then a `git diff` gate on `src/ipc/bindings.ts` |
| `macos` | macOS | yes | clippy + `cargo test` (exercises the CUPS backend), bindings gate, `pnpm test`, then the advisory debug build + smoke launch |
| `rust-coverage` | macOS | yes | `cargo llvm-cov --fail-under-lines 70`; uploads `lcov.info` |
| `ignored-tests` | macOS | no (advisory) | `cargo test -- --ignored`; **schedule/workflow_dispatch only**, weekly Mondays 05:00 UTC |
| `build` | windows | yes | `pnpm tauri build --debug` + the Windows smoke launch; `needs: [frontend, backend]` |
| `e2e` | windows | no | the WebdriverIO suite; `needs: [frontend, backend]` |
| `shell-ext-win` | windows | yes | matrix over `x86_64-` and `aarch64-pc-windows-msvc`: fmt, clippy, release build with `MAECLYPRINT_REQUIRE_RESOURCES=1`, `cargo test` (x64 only — an x64 runner cannot execute an ARM64 test binary), and a `dumpbin /EXPORTS` check for `DllGetClassObject` + `DllCanUnloadNow` |
| `shell-ext-portable` | ubuntu | yes | proves the non-COM half of the shell extension is developable off Windows: `cargo test` on the host plus `cargo check --target x86_64-pc-windows-msvc` (type-checks the COM layer with no linker) |

Every job except `ignored-tests` carries `if: github.event_name != 'schedule'`; `ignored-tests` carries the inverse. The workflow triggers are `push` to `main`, `pull_request` to `main`, `workflow_dispatch` and the weekly `schedule` — the last two exist solely for `ignored-tests`.

Note that `backend` and `shell-ext-win` are separate lanes on purpose: `shell-ext-win/` is deliberately **not** a Cargo workspace member (its release profile must keep `panic = "unwind"` so `catch_unwind` at the COM boundary works, which `src-tauri`'s `panic = "abort"` would destroy), so `--manifest-path src-tauri/Cargo.toml` never touches it.

## Print mock harness — planned

> **Status: not implemented.** Nothing described in this section exists in the tree. There is no `src-tauri/tests/fixtures/`, no `src-tauri/tests/common/`, and no PDF-comparison helper. It is kept as the design to build against when print-output verification lands; treat every path and crate name below as a proposal.

### What it would do

- Provides a deterministic virtual printer (Microsoft Print to PDF on Windows, CUPS-PDF on macOS).
- Captures every print job as a `.pdf` file in a known location.
- Provides helper functions to compare output against fixtures.

### Fixtures (proposed layout)

`src-tauri/tests/fixtures/` **does not exist**. It would contain:

```
fixtures/
├── inputs/
│   ├── single-page.pdf
│   ├── multi-page.pdf
│   ├── encrypted.pdf
│   ├── small.docx
│   ├── chart.xlsx
│   ├── slide.pptx
│   ├── readme.md
│   ├── photo.jpg
│   └── webpage.html
└── expected/
    ├── single-page.pdf       # What "Microsoft Print to PDF" produces from inputs/single-page.pdf
    ├── single-page.png       # Rendered at 150 DPI for visual diff
    └── ...
```

Fixtures would be committed (Git LFS for files > 1 MB). Note that the input list above spans Office, HTML and Markdown, which the engine cannot print at all today (see [supported-formats.md](../03-requirements/supported-formats.md)) — the fixture set arrives with the converters, not before them.

### Comparison strategy (proposed)

```rust
// src-tauri/tests/common/mod.rs
pub fn assert_pdf_matches(actual: &Path, expected: &Path) -> Result<()> {
    // 1. Page count
    let actual_pages = pdf_extract::extract_pages(actual)?.len();
    let expected_pages = pdf_extract::extract_pages(expected)?.len();
    assert_eq!(actual_pages, expected_pages, "page count mismatch");

    // 2. Text layer
    let actual_text = pdf_extract::extract_text(actual)?;
    let expected_text = pdf_extract::extract_text(expected)?;
    assert_eq!(actual_text, expected_text, "text mismatch");

    // 3. Visual diff (optional, expensive)
    if std::env::var("RUN_VISUAL_DIFF").is_ok() {
        let actual_png = render_pdf_to_png(actual, 150)?;
        let expected_png_path = expected.with_extension("png");
        let diff_pct = image_compare(&actual_png, &expected_png_path)?;
        assert!(diff_pct < 0.5, "visual diff {}% exceeds 0.5%", diff_pct);
    }
    Ok(())
}
```

We default to **text + page count** comparison because it's deterministic and fast. Visual diff is opt-in via env var because it's slower and can flag minor rendering differences (anti-aliasing, font hinting) that don't matter functionally.

## Performance / regression tests

### Benchmarks — none exist

There is **no `src-tauri/benches/` directory and no `criterion` dependency.** The performance NFRs (NFR-001…NFR-005) are therefore unverified by automation today.

If and when benchmarks are added, the plan is Criterion under `src-tauri/benches/`, run locally before any change to the queue or engine and reported in the PR — not in CI, where shared runners are too noisy to compare against a baseline. Until then, do not claim a bench "confirms" anything.

### Regression detection

For correctness regressions, the Rust suite plus the bindings-drift gate is the safety net. There is no performance regression net at all — this is a known gap, not a policy.

## Manual QA

Manual QA is for things that resist automation:

- "Does the print job feel responsive?"
- "Is the Spanish translation natural?"
- "Does the UI look right at 200% Windows display scaling?"
- "Does the right-click menu appear in the right position in File Explorer?"
- "Does an EV-signed installer trigger no SmartScreen warning?"

We have a manual QA checklist (in [release-checklist.md](./release-checklist.md)) that runs pre-release on:

- Windows 10 22H2.
- Windows 11 23H2.
- macOS 13, 14, 15 (M5+).
- At least 3 different physical printers (laser, inkjet, network).

The checklist covers ~30 minutes of focused testing per platform.

## Generated-artifact drift gates

We do not use snapshot testing (`insta` is not a dependency, and `src-tauri/tests/snapshots/` does not exist). What we do have is one **generated-file drift gate**, which serves a similar purpose for the artifact that matters most:

`src-tauri/tests/ipc_surface.rs` regenerates `src/ipc/bindings.ts` from the tauri-specta command surface during `cargo test` and fails when the committed copy is stale. Regenerate deliberately with:

```bash
UPDATE_BINDINGS=1 cargo test --manifest-path src-tauri/Cargo.toml
git add src/ipc/bindings.ts
```

CI backs this up in both the `backend` and `macos` jobs with a plain `git diff --exit-code -- src/ipc/bindings.ts`, which catches the one case the test cannot see: a filtered `cargo test` run that never regenerated anything.

If a future need for real string snapshots appears (log line format, manifest JSON), adding `insta` is a normal dependency decision — but it must be made before this section can describe it in the present tense.

## What we deliberately don't test

- **Tauri itself.** We trust upstream's tests.
- **Win32 / CUPS APIs.** We trust the OS.
- **Bundled subprocess output.** SumatraPDF is GPL'd third-party; if its output changes between versions, we update expectations, not bug them.
- **Network layer for the updater.** We trust `ureq`. The update client's own logic (semver comparison, release-payload parsing, the 404/no-release and error paths) *is* unit-tested against fixture payloads, and `src-tauri/tests/update_startup.rs` covers the startup check — but no test stands up an HTTP server, and the one that really calls `api.github.com` is `#[ignore]`d.
- **Rendering output of WebView2 / WKWebView.** Browser rendering is tested by upstream.

## Running tests locally

| Command | Runs |
|---|---|
| `cargo test --manifest-path src-tauri/Cargo.toml` | Unit + integration tests not marked `#[ignore]` (or `make test-rust`) |
| `cargo test --manifest-path src-tauri/Cargo.toml -- --ignored` | **Only** the ignored ones (network + real CUPS) — what the weekly CI lane runs |
| `cargo test --manifest-path src-tauri/Cargo.toml -- --include-ignored` | Everything, ignored included |
| `cargo test <name>` | Tests matching `<name>` substring |
| `cargo test --manifest-path shell-ext-win/Cargo.toml` | The shell extension's portable half; works on any host OS |
| `pnpm test` | Frontend unit tests (`vitest run`) |
| `pnpm test:coverage` | Frontend tests with V8 coverage (report under `coverage/`); no threshold locally |
| `pnpm test:e2e` | E2E tests (Windows/Linux; requires a built binary + `tauri-driver` + `NATIVE_DRIVER`) |
| `pnpm check-all` | `lint`, `format:check`, `typecheck`, `test` — **frontend only, no Rust** |
| `make check` | Everything: the four above plus `cargo fmt --check`, `clippy -D warnings` and `cargo test` |

## Test naming

| Layer | Convention |
|---|---|
| Rust unit | `test_<scenario>_<expected>`, or `<scenario>_<expected>` in a co-located `mod tests` |
| Rust integration | `fr_<NNN>_<scenario>` when pinning a requirement (e.g. `fr_102_atomic_write`), else `<feature>_<scenario>` |
| Vitest | Plain English in `it(...)`, prefixed `FR-###:` / `NFR-###:` when it pins a requirement |
| WebdriverIO | Plain English in `it(...)`, grouped by `describe(...)` |

Names should read as a sentence describing what's true if the test passes. Bad: `test_queue_1`. Good: `test_enqueue_when_full_returns_full_error`. The `FR`/`NFR` tag is a real, searchable convention — `it('FR-021: built-in profiles are read-only …')`, `fr_020_crud_roundtrip` — so you can grep a requirement id to its tests.

## See also

- [coding-standards.md](./coding-standards.md)
- [contributing.md](./contributing.md)
- [release-checklist.md](./release-checklist.md)
- [../07-build-and-release/ci-cd.md](../07-build-and-release/ci-cd.md)
