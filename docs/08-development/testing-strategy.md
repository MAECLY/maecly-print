# Testing Strategy

> **TL;DR.** Test pyramid: many fast Rust unit tests, fewer integration tests that spawn real adapters in CI, fewer still end-to-end WebdriverIO tests through the GUI (Windows/Linux only). A "Microsoft Print to PDF" virtual printer is the workhorse of integration testing — every test prints to it and we verify the output. Coverage target: 70% on Rust, 60% on TypeScript — **aspirational, not gated** — with quality over quantity. Requirement-tied tests carry an `FR-###` / `NFR-###` tag in their name.

This document covers what we test, how, with which tools, and what the acceptance bar looks like.

## The pyramid

```
                ┌──────────────┐
                │  Manual QA   │  Pre-release smoke; humans only
                ├──────────────┤
                │     E2E      │  WebdriverIO + tauri-driver (Win/Linux only)
                ├──────────────┤
                │ Integration  │  ~50 tests; real subprocesses, virtual printer
                ├──────────────┤
                │     Unit     │  ~500 tests; in-process, milliseconds each
                └──────────────┘
```

Most bugs are caught by unit tests (cheap, fast). A smaller layer of integration tests catches the ones that span subprocess boundaries (LibreOffice CLI quirks, SumatraPDF flag changes). E2E tests catch GUI integration issues. Manual QA handles things that resist automation (does it look right? does it feel right?).

## Rust unit tests

### Tooling

- `cargo test` (the standard).
- Optional: `cargo nextest` for faster runs (3–5x speedup).
- Snapshot tests via `insta` for stable string output.
- Property tests via `proptest` for input domains (rare; mostly for the format detector).

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

We aim for **70% line coverage** on Rust code. We use `cargo llvm-cov` to measure:

```bash
cargo install cargo-llvm-cov --locked
cargo llvm-cov --html
```

Coverage is a sanity check, not a goal. We don't write tests purely to chase coverage; we write tests for behaviors that matter. Reaching 70% naturally means we've covered the important behaviors.

Excluded from coverage:

- Win32 / Apple FFI calls (hard to unit-test; covered by integration tests).
- `unreachable!()` arms.
- Trivial getters / setters.
- Generated code (e.g. from `build.rs`).

## Rust integration tests

### Location

`src-tauri/tests/`. Each `.rs` file is a separate test binary, which means each gets its own clean environment. The suite exists and grows per milestone — today it holds the persistence binaries; the print/queue/shell binaries land with their milestones.

```
src-tauri/tests/
├── profile_roundtrip.rs   # Present: FR-020/021/022/026/027 — CRUD, immutable
│                          #   built-ins, atomic save, 500 profiles, default rules
├── settings_persist.rs    # Present: FR-100/101/102/103 — human-readable TOML,
│                          #   corrupt-file fallback, atomic write, schema_version
├── pdf_print.rs           # Planned (M3): print a PDF, verify output
├── queue_concurrency.rs   # Planned: enqueue N jobs concurrently, all complete
└── shell_handoff.rs       # Planned (M2): spawn shell ext, send IPC, verify
```

Test functions that pin a requirement are **named after it** — `fr_020_crud_roundtrip`, `fr_102_atomic_write` — so a failure names the requirement it breaks. Domain logic is unit-tested in-module (`#[cfg(test)]`); these integration binaries exercise the real on-disk read/write paths through a tempdir.

### Tooling

- Standard `cargo test`.
- `tempfile` for ephemeral directories.
- `wiremock` for mocking the update server when testing the updater client.

### Virtual printer

Integration tests print to a virtual printer that captures output as a `.pdf` file we can inspect.

**Windows:** "Microsoft Print to PDF" (preinstalled). The output path is set via the print job's "OutputFile" parameter:

```rust
// src-tauri/src/printer/windows.rs (test helper)
#[cfg(test)]
pub fn print_to_pdf_helper(input: &Path, output: &Path) -> Result<()> {
    let opts = PrintOpts {
        printer_name: "Microsoft Print to PDF".to_string(),
        output_file: Some(output.to_path_buf()),
        ..Default::default()
    };
    print_pdf(input, &opts)
}
```

**macOS:** `cups-pdf` (Homebrew). Configures CUPS to write `.pdf` to a known location.

### Output verification

After printing, integration tests verify the output `.pdf`:

| Verification | Tool | Purpose |
|---|---|---|
| File exists and has nonzero size | `std::fs::metadata` | Sanity check |
| Page count matches expected | `pdf-extract` crate | Confirms multi-page batches don't lose pages |
| Text content matches expected | `pdf-extract` text layer | Confirms content didn't get corrupted |
| Visual diff against fixture | `image_compare` after rendering with `pdfium-render` | Catches layout regressions |

For visual diffs, fixtures are stored as PNG renderings at 150 DPI. Diff threshold: < 0.5% of pixels different. Above that, the test fails and the diff image is uploaded as a CI artifact for review.

### Subprocess fixtures

Integration tests that need LibreOffice, Pandoc, etc. install them in CI:

```yaml
# In ci.yml integration-tests job:
- name: Install LibreOffice
  if: runner.os == 'Windows'
  run: choco install libreoffice-fresh -y --no-progress

- name: Install Pandoc
  run: choco install pandoc -y --no-progress
```

On macOS:

```yaml
- name: Install LibreOffice + Pandoc
  if: runner.os == 'macOS'
  run: brew install --cask libreoffice && brew install pandoc
```

This adds ~5 minutes to CI but is the only way to test these adapters honestly.

### When to skip subprocess tests

Tests that require subprocesses are gated behind `#[ignore]` for `cargo test --release` runs that don't have those subprocesses available:

```rust
#[test]
#[ignore = "requires LibreOffice; run with --include-ignored"]
fn test_docx_print_via_libreoffice() {
    // ...
}
```

CI runs them via `cargo test --include-ignored`. Local dev runs `cargo test` and skips them by default. Local dev runs them on demand via `cargo test --include-ignored` or by name.

## Frontend unit tests

### Tooling

- **Vitest** (Vite-native test runner; faster than Jest for our setup).
- **React Testing Library** for component tests.
- **MSW (Mock Service Worker)** for mocking HTTP requests in components that fetch.
- **`@testing-library/user-event`** for simulating user interactions.

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

**60% line coverage** on TypeScript. Lower than Rust because UI code has more code that's hard to test (rendering details, Tailwind utility usage). We don't chase the number; we cover behaviors.

Measured by `pnpm test:coverage` (`vitest run --coverage`) with the `@vitest/coverage-v8` provider — reporters `text`/`html`/`lcov`, output under `coverage/`, non-product files (generated `bindings.ts`, `src/test/**`, config) excluded from the denominator (`vite.config.ts`). **The 60/70% targets are aspirational and not enforced** — CI runs the coverage build and uploads the report as an artifact, but there is no failing threshold gate.

## End-to-end tests

### Tooling

- **WebdriverIO** as the test framework (`@wdio/cli` + the Mocha framework adapter), configured in `e2e/wdio.conf.ts`.
- **`tauri-driver`** (a cargo binary, `cargo install tauri-driver --locked`) as the WebDriver bridge to the Tauri webview. On Windows it proxies to the Edge WebDriver (`msedgedriver.exe`) matching the installed WebView2, passed via the `NATIVE_DRIVER` env var.
- Tests run against an actual built `maeclyprint(.exe)` binary (`pnpm tauri build --debug`), not the dev server. Run locally with `pnpm test:e2e`.

### Platform support — Windows/Linux only

`tauri-driver` supports **Windows and Linux only**; **macOS WKWebView exposes no WebDriver endpoint**, so there is no automated E2E on macOS. macOS is covered instead by the Rust unit/integration suite, the Vitest suite, a **launch smoke** ("does the built app start and stay up"), and manual QA. A parallel **launch smoke** also runs on Windows.

### Coverage scope

E2E specs live in `e2e/specs/` and are intentionally few — they cover **flows**, not features:

- `launch.spec.ts` — the binary starts and the shell paints.
- `navigation.spec.ts` — sidebar navigation swaps screens.
- `about.spec.ts` — About shows version, license, and the update check.

More flows (create a profile and use it; change a setting and confirm it persists across restart) land as the suite grows. What E2E tests don't cover (we have lower-level tests for these): component-level UI behavior (Vitest), print correctness (Rust integration), and the IPC contract (Rust).

### Test structure

```ts
// e2e/specs/about.spec.ts — WebdriverIO + Mocha globals
import { navigateTo } from './helpers';

describe('About', () => {
  it('shows the version and offers an update check', async () => {
    await navigateTo('about');
    await expect($('h1')).toHaveText('About');
    await expect($('button=Check for updates')).toBeDisplayed();
  });
});
```

### CI integration

Two GUI jobs run on GitHub Actions, both **non-blocking** (`continue-on-error`) because driving a real WebView2 GUI (or launching a GUI on a headless runner) is inherently flake-prone:

- **Smoke launch** — builds the app and asserts it starts and stays up ~10 s. Runs on both `windows-latest` and `macos-latest`.
- **E2E (WebdriverIO + tauri-driver)** — the `e2e/specs/**` suite on `windows-latest`; installs `tauri-driver` and points it at the Edge WebDriver.

The blocking gates are the Rust (`cargo test` on Windows and macOS) and frontend (`pnpm test:coverage`) jobs. Flake handling: a spec that fails repeatedly on `main` is fixed within a week or removed — we don't carry flaky GUI tests forward.

## Print mock harness

This is the central piece of integration testing.

### What it does

- Provides a deterministic virtual printer (Microsoft Print to PDF on Windows, CUPS-PDF on macOS).
- Captures every print job as a `.pdf` file in a known location.
- Provides helper functions to compare output against fixtures.

### Fixtures

`src-tauri/tests/fixtures/` contains:

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

Fixtures are committed (Git LFS for files > 1 MB).

### Comparison strategy

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

### Benchmarks

`src-tauri/benches/` houses Criterion benchmarks for hot paths:

```rust
// src-tauri/benches/queue_throughput.rs
use criterion::{criterion_group, criterion_main, Criterion};

fn bench_enqueue(c: &mut Criterion) {
    c.bench_function("enqueue_1000_jobs", |b| {
        b.iter(|| {
            // ...
        });
    });
}

criterion_group!(benches, bench_enqueue);
criterion_main!(benches);
```

We do not run benchmarks in CI by default (too noisy on shared runners). We run them locally before any change to the queue or engine and document results in the PR.

### Regression detection

For correctness regressions, the integration suite is the safety net. For performance regressions, we depend on developers running benches before suspect changes.

If users report performance regressions in production, we add a benchmark for that specific scenario and add it to a manual-run regression suite.

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

## Snapshot tests

For output that's hard to assert structurally (log line format, generated HTML, manifest JSON), we use `insta`:

```rust
#[test]
fn test_log_format_is_stable() {
    let entry = make_log_entry();
    insta::assert_json_snapshot!(entry);
}
```

When the snapshot legitimately changes:

```bash
INSTA_UPDATE=always cargo test
git diff src-tauri/tests/snapshots/  # Review carefully
```

Snapshot files are committed. They are documentation of expected output as much as test artifacts.

## What we deliberately don't test

- **Tauri itself.** We trust upstream's tests.
- **Win32 / CUPS APIs.** We trust the OS.
- **Bundled subprocess output.** SumatraPDF is GPL'd third-party; if its output changes between versions, we update fixtures, not bug them.
- **Network layer for the updater.** We mock the manifest endpoint with `wiremock`. Trusting `reqwest` and Tauri's updater plugin.
- **Rendering output of WebView2 / WKWebView.** Browser rendering is tested by upstream.

## Running tests locally

| Command | Runs |
|---|---|
| `cargo test` (in `src-tauri/`) | Unit + integration tests not marked `#[ignore]` |
| `cargo test --include-ignored` | All tests including subprocess-dependent ones |
| `cargo test <name>` | Tests matching `<name>` substring |
| `pnpm test` | Frontend unit tests (`vitest run`) |
| `pnpm test:coverage` | Frontend tests with V8 coverage (report under `coverage/`) |
| `pnpm test:e2e` | E2E tests (Windows/Linux; requires a built binary + `tauri-driver`) |
| `pnpm check-all` | Lint, format-check, typecheck, and the Vitest suite |

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
