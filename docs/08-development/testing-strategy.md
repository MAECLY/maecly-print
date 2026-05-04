# Testing Strategy

> **TL;DR.** Test pyramid: many fast Rust unit tests, fewer integration tests that spawn real adapters in CI, fewer still end-to-end Playwright tests through the GUI. A "Microsoft Print to PDF" virtual printer is the workhorse of integration testing — every test prints to it and we verify the output. Coverage target: 70% on Rust, 60% on TypeScript, with quality over quantity.

This document covers what we test, how, with which tools, and what the acceptance bar looks like.

## The pyramid

```
                ┌──────────────┐
                │  Manual QA   │  Pre-release smoke; humans only
                ├──────────────┤
                │     E2E      │  ~20 tests; Playwright + tauri-driver
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

`src-tauri/tests/`. Each `.rs` file is a separate test binary, which means each gets its own clean environment.

```
src-tauri/tests/
├── pdf_print.rs           # Print a PDF, verify output
├── docx_print.rs          # Print a DOCX via LibreOffice, verify output
├── profile_roundtrip.rs   # Save profile to disk, load it back, assert equal
├── queue_concurrency.rs   # Enqueue 100 jobs concurrently, all complete
└── shell_handoff.rs       # Spawn shell-ext-win.dll, send IPC, verify receive
```

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

Tests that depend on `invoke()` mock the Tauri API:

```ts
// src/test/setup.ts
import { vi } from 'vitest';

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(),
}));
```

Tests then control return values:

```ts
import { invoke } from '@tauri-apps/api/core';

it('shows loaded profiles', async () => {
  vi.mocked(invoke).mockResolvedValue([{ id: 'a', name: 'Default' }]);
  render(<ProfilesPage />);
  expect(await screen.findByText('Default')).toBeInTheDocument();
});
```

### Coverage target

**60% line coverage** on TypeScript. Lower than Rust because UI code has more code that's hard to test (rendering details, Tailwind utility usage). We don't chase the number; we cover behaviors.

Measured via Vitest's built-in coverage reporter (`pnpm test --coverage`), backed by V8.

## End-to-end tests

### Tooling

- **Playwright** as the test framework.
- **`tauri-driver`** as the bridge between Playwright and the Tauri app's webview.
- Tests run against an actual built `maeclyprint(.exe)` binary, not the dev server.

### Coverage scope

E2E tests are intentionally small in number (~20 tests). They cover **flows**, not features:

- App launches successfully.
- User opens Settings, changes the channel, restarts; the change persists.
- User imports a profile, prints with it; the queue shows the job.
- User triggers "Check for updates"; the appropriate notification appears.
- User opens About page; license and version are visible.

What E2E tests don't cover (we have lower-level tests for these):

- Component-level UI behavior (unit tests).
- Print correctness (integration tests).
- IPC contract (Rust tests).

### Test naming and structure

```ts
// e2e/profiles.spec.ts
import { test, expect } from '@playwright/test';

test.describe('Profiles', () => {
  test('user creates a profile and uses it', async ({ page }) => {
    await page.goto('/profiles');
    await page.getByRole('button', { name: 'New profile' }).click();
    await page.getByLabel('Name').fill('Draft');
    await page.getByLabel('Print quality').selectOption('Draft');
    await page.getByRole('button', { name: 'Save' }).click();

    await expect(page.getByText('Draft')).toBeVisible();
    // ... use the profile in a print job, verify queue state
  });
});
```

### CI integration

E2E tests are slow (multiple seconds per test) and flaky-prone (they depend on a real GUI). We run them:

- On every PR (full suite, sequentially).
- On nightly builds.

Flake handling: any test that fails twice in a row on `main` gets `test.fail()` applied and an issue opened. We do **not** merge flaky tests forward; we either fix them within a week or delete them.

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
| `pnpm test` | Frontend unit tests |
| `pnpm test:e2e` | E2E tests (requires built binary) |
| `pnpm check-all` | Everything: lint, typecheck, format-check, all tests |

## Test naming

| Layer | Convention |
|---|---|
| Rust unit | `test_<scenario>_<expected>` |
| Rust integration | `test_<feature>_<scenario>_<expected>` |
| Vitest | Plain English in `it(...)` |
| Playwright | Plain English in `test(...)`, grouped by `test.describe(...)` |

Names should read as a sentence describing what's true if the test passes. Bad: `test_queue_1`. Good: `test_enqueue_when_full_returns_full_error`.

## See also

- [coding-standards.md](./coding-standards.md)
- [contributing.md](./contributing.md)
- [release-checklist.md](./release-checklist.md)
- [../07-build-and-release/ci-cd.md](../07-build-and-release/ci-cd.md)
