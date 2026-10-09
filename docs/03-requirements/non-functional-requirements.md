# Non-Functional Requirements

> **TL;DR** — Quality attributes: how fast, how reliable, how secure, how accessible, how maintainable. Targets are concrete and testable; vague aspirations are not allowed here.

Non-functional requirements (NFRs) are the "ilities" that the system must satisfy across all features. Functional requirements (FRs) live in [functional-requirements.md](./functional-requirements.md).

Each NFR has a stable ID (`NFR-XXX`), a measurable target, and a verification method.

> **The "Verification" column is the *intended* method, not a claim that it runs.** Where an NFR is met, or where its stated verification does not exist, this is called out inline. The [Implementation status](#implementation-status) section at the end is the summary; it is the part to trust, because every line in it was checked against the tree.

## 1. Performance

| ID | Requirement | Target | Verification |
|---|---|---|---|
| NFR-001 | Cold start of the context menu invocation MUST complete the launch of the configuration GUI in <500 ms after right-click | p95 ≤ 500 ms on a 4-core/8 GB Win 11 machine with SSD | E2E test with stopwatch instrumentation |
| NFR-002 | Cold start of the silent print path (default profile, no GUI) MUST submit the first job to the spooler in <300 ms | p95 ≤ 300 ms | Same E2E test |
| NFR-003 | Idle RAM usage of the resident GUI MUST be <100 MB | RSS ≤ 100 MB after 60s idle | `ps`/Task Manager assertion in CI |
| NFR-004 | Peak RAM usage with 50 jobs queued MUST be <250 MB | RSS ≤ 250 MB | Load test |
| NFR-005 | The system MUST NOT add more than 50 ms of overhead to the total per-file printing time vs invoking the same backend (SumatraPDF/CUPS) directly | Δ ≤ 50 ms p50 | Microbenchmark in CI |
| NFR-006 | The Windows installer (`setup.exe`) MUST be ≤ 15 MB | size ≤ 15 MB | Size assertion in release pipeline |
| NFR-007 | The macOS `.dmg` MUST be ≤ 25 MB (universal binary) | size ≤ 25 MB | Same |
| NFR-008 | The Sparse MSIX package MUST be ≤ 2 MB | size ≤ 2 MB | Same |

## 2. Reliability

| ID | Requirement | Target | Verification |
|---|---|---|---|
| NFR-010 | The system MUST recover unfinished queue items after an unexpected exit (crash, kill, OS reboot) | 100% of jobs not marked `printed` resume on next launch | Crash-restart integration test |
| NFR-011 | The system MUST NOT corrupt the user's source files. Print engine adapters MUST treat input files as **read-only**. | Zero hash mismatches on test corpus after a full print cycle | File hash verification in CI |
| NFR-012 | A print/convert timeout MUST NOT leak processes; orphan children MUST be killed | **Met**: the dispatcher uses `wait-timeout` for a bounded wait and kills the child, so a hung subprocess cannot wedge the queue. Covered by `src-tauri/tests/print_timeout.rs`. There is no `stress` feature. | Integration test |
| NFR-013 | The system MUST tolerate the disappearance of an external dependency mid-job (e.g., LibreOffice uninstalled) by failing the job cleanly with a typed error | Test passes with `pkill soffice` mid-conversion | Chaos test |
| NFR-014 | The system MUST support running a single instance only. A second invocation MUST send its arguments to the running instance via IPC and exit. | Pass: spawn 10 invocations in parallel, only 1 process exists | Concurrency test — **Met**: `tauri-plugin-single-instance` plus `cli/single_instance.rs`; the 10-way spawn test is not automated |

## 3. Security

| ID | Requirement | Target | Verification |
|---|---|---|---|
| NFR-020 | The application MUST install and run without administrator privileges for normal use | Install in `%LOCALAPPDATA%`, register in `HKCU` only | Manual QA on standard user account |
| NFR-021 | All released binaries MUST be code-signed with a valid certificate (EV cert preferred for Windows from day 1) | `signtool verify /pa` passes; Apple `codesign -dvv` passes; notarization passes | Release pipeline gate |
| NFR-022 | All update artifacts MUST be cryptographically signed; the client MUST refuse unsigned or wrongly-signed updates | Tampered update is rejected with `SignatureMismatch` error | Updater integration test |
| NFR-023 | The application MUST NOT make any unsolicited network requests except to the configured update server | Network audit shows only `updates.maecly.com` egress (or user-configured override) | Wireshark capture in QA |
| NFR-024 | The shell extension COM DLL MUST be limited to: receive selection, serialize to file/stdin, spawn the main executable. It MUST NOT touch user data. | Code review + LOC budget ≤ 1500 | Code review checklist |
| NFR-025 | The application MUST validate all file paths from the shell extension as canonical absolute paths before passing to converters (no path traversal). | Fuzz test with `..\..\..\Windows\System32` payloads | Fuzz harness |
| NFR-026 | All third-party converter invocations MUST use **arg vector** APIs (`Command::new(...).args(...)`), never shell strings, to prevent command injection. | Code review; clippy lint `clippy::cmd_arg_string` enabled | Static analysis |
| NFR-027 | Secrets (signing private keys, future API tokens) MUST NEVER be committed; CI uses GitHub Secrets / OIDC. | **No gitleaks scan exists.** Vacuously satisfied today: `ci.yml` reads no secret at all, because nothing signs or publishes. | — |

## 4. Usability

| ID | Requirement | Target | Verification |
|---|---|---|---|
| NFR-030 | A first-time user MUST be able to print a single PDF from the context menu in <30 seconds end-to-end after install | Usability test n=5, all complete | Recorded user test |
| NFR-031 | All defaults MUST be sane such that the application is useful without opening Settings | Verified by zero-config quick start in user test | Same |
| NFR-032 | Destructive actions (delete profile, clear history) MUST require confirmation with a clear "Undo" affordance for 5 seconds. | UI assertion in component tests | Vitest |
| NFR-033 | Error messages MUST be actionable: state what went wrong, what the user can do, and provide a link to logs. Vague "Something went wrong" is forbidden. | PR review checklist + lint rule for empty `.message` | Code review |
| NFR-034 | The system MUST provide keyboard shortcuts for all primary actions (print, cancel, switch profile, open queue). | Documented in the app | **Met** — see status section |
| NFR-035 | All UI text MUST avoid jargon when a plain-language equivalent exists; jargon allowed in Advanced Settings. | Editorial pass before each release | Release checklist |

## 5. Accessibility

| ID | Requirement | Target | Verification |
|---|---|---|---|
| NFR-040 | The GUI MUST conform to **WCAG 2.1 Level AA** | *Stated target.* **The stated verification does not exist**: there is no axe-core dependency and no Playwright — the E2E suite is WebdriverIO. | Partly covered by component tests |
| NFR-041 | The GUI MUST be fully operable with a keyboard alone (no mouse required) | Manual keyboard-only walkthrough in release checklist | Manual QA |
| NFR-042 | All interactive controls MUST have visible focus indicators with ≥3:1 contrast against background | Per-primitive Vitest assertions that a focus ring is painted (the `.mp-focus` class), plus manual contrast review. No axe-core. | Vitest + QA |
| NFR-043 | All non-decorative images and icons MUST have text alternatives (`aria-label`, `alt`, etc.) | Accessible-name assertions in the primitives' and screens' Vitest suites. No axe-core. | Vitest |
| NFR-044 | The application MUST be compatible with screen readers (NVDA on Windows, VoiceOver on macOS) | Manual smoke test with screen reader before each release | Release checklist |
| NFR-045 | Color MUST NOT be the only carrier of meaning; status uses icon + label + color. | Component review | Code review |

## 6. Internationalization

| ID | Requirement | Target | Verification |
|---|---|---|---|
| NFR-050 | All user-facing strings MUST be externalized; no string literals in JSX/Rust user-facing paths | Real layout is `src/i18n/locales/{lang}/{namespace}.json`, not a flat `i18n/{lang}.json`. **No lint enforces this**: the ESLint rule does not exist, and `#[deny(string_literal_in_user_text)]` is not a real attribute. Enforced by review only. | Code review |
| NFR-051 | The translation framework MUST support pluralization and interpolation | **Partly met, by a different mechanism.** `src/i18n/` is dependency-free — not ICU MessageFormat and not i18next. It supports `{placeholder}` interpolation and `_one`/`_other` plural keys selected by a numeric `count`, with English rules. Gendered forms and ICU date/number formatting are **not** supported. | `src/i18n/i18n.test.ts` |
| NFR-052 | The system MUST support adding a new language with no code changes | **Not met as written.** A language is a directory of eight namespace JSONs, and `src/i18n/index.ts` imports each one statically, so adding a language edits code. English is the source of truth and missing keys fall back to it. | — |
| NFR-053 | Layouts MUST handle text expansion of +50% without truncation or overlap | **Unverified.** No pseudolocale, no snapshot tests at 150%. | — |
| NFR-054 | The system SHOULD be RTL-ready by v2.0 (logical CSS properties, no hard-coded `left`/`right`) | RTL pseudolocale snapshot test | Future |

## 7. Maintainability

| ID | Requirement | Target | Verification |
|---|---|---|---|
| NFR-060 | Test coverage MUST be ≥70% for Rust code, ≥60% for TS code | **Gated in CI**, but with `cargo llvm-cov --fail-under-lines 70` (not tarpaulin) and Vitest thresholds currently set at 55/55/65/75 — below the 60% target, as a regression gate. Measured: Rust 80.4% lines, TS 61.4% lines. | CI gate |
| NFR-061 | All public Rust functions MUST have rustdoc with at least a one-line summary | **The lint is NOT enabled.** `#![deny(missing_docs)]` appears in neither `src-tauri/src/lib.rs` nor `shell-ext-win/src/lib.rs`. In practice the tree is documented well above the bar, but nothing enforces it — a new undocumented `pub` item compiles clean. | Code review only |
| NFR-062 | Documentation in this `docs/` folder MUST stay in sync with code; PRs that modify behavior MUST update affected docs | Reviewer checklist | Code review |
| NFR-063 | The CI MUST be green for a PR to be merged (no override without maintainer approval and reason in commit) | Branch protection rule | GitHub settings |
| NFR-064 | Build MUST be reproducible: same source + same toolchain version → byte-identical binary | `cargo build --locked --release` is reproducible | Quarterly verification |
| NFR-065 | Deps MUST be audited weekly via `cargo audit` and `pnpm audit`; high/critical vulnerabilities MUST be fixed within 7 days. | **No audit workflow exists.** The only scheduled CI job is `ignored-tests`. Dependabot is the sole dependency automation. | — |

## 8. Portability

| ID | Requirement | Target | Verification |
|---|---|---|---|
| NFR-070 | A single codebase MUST support Windows (x64, ARM64) and macOS (Apple Silicon, Intel) | **Not verified for all four.** `ci.yml` builds only each runner's native architecture; the sole cross-arch matrix is `shell-ext-win` (x64 + ARM64, ARM64 build-only). No `x86_64-apple-darwin` lane, no universal binary. | Partial CI |
| NFR-071 | Platform-specific code MUST be isolated to clearly named modules | **Met.** `src-tauri/src/platform/`: `windows.rs`, `cups.rs`, `macos/`. Windows is a single file rather than a directory; the isolation requirement is satisfied either way. | Code review |
| NFR-072 | The system MUST NOT require Linux support in v1; Linux MAY be added in v2 if community demand exists | Documented in [milestones.md](../06-roadmap/milestones.md) | Roadmap |

## 9. Observability

| ID | Requirement | Target | Verification |
|---|---|---|---|
| NFR-080 | All log entries MUST be structured (JSONL) with: timestamp (ISO 8601), level, target/module, message, optional fields | `tracing-subscriber` JSON formatter | Unit test on log format |
| NFR-081 | Logs MUST roll daily and by size; retention bounded | **Met, with a tightened policy**: `logging::Policy` rolls daily *and* at a size ceiling, then sweeps by `retain_days` (14) **and** `retain_files` (7) — whichever bound bites first wins. The sweep runs at startup and again on every roll, so a resident instance stays bounded without a restart. | `logging` unit tests |
| NFR-082 | Sensitive data (file contents, full paths if user opts) MUST never be logged at default level | Log audit at QA | Manual QA |
| NFR-083 | The system MAY include an opt-in crash reporter (Sentry-compatible) disabled by default | Documented in Settings | Privacy doc |

## 10. Privacy

See [privacy.md](../09-legal/privacy.md) for the full statement.

| ID | Requirement | Target | Verification |
|---|---|---|---|
| NFR-090 | The system MUST NOT collect telemetry by default. | No telemetry endpoint reachable in default config | Network audit |
| NFR-091 | File contents MUST never leave the user's machine | Integration test asserts no network egress during a print job (other than to the printer) | Wireshark in CI optional |
| NFR-092 | If a future opt-in telemetry feature is added, it MUST be **off by default** and require explicit affirmative consent. | Design review of telemetry doc | Governance |

## 11. Compliance

| ID | Requirement | Target | Verification |
|---|---|---|---|
| NFR-100 | The project MUST comply with GPLv3 obligations: source disclosure, license text in distribution, copyright notices preserved. | License file present in distribution; about dialog links to source | Release checklist |
| NFR-101 | All bundled third-party components MUST be license-compatible with GPLv3. See [third-party-licenses.md](../09-legal/third-party-licenses.md). | License audit per release | Release checklist |
| NFR-102 | The project MUST publish a SECURITY.md with a private vulnerability disclosure channel. | File exists, channel works | Release checklist |

## Implementation status

Verified against the tree. Only NFRs whose status is non-obvious or has recently changed are listed; anything absent from this table has not been assessed and must not be assumed met.

### Met

| ID | Evidence |
|---|---|
| **NFR-012** | `wait-timeout` bounded wait + kill in the dispatcher; `src-tauri/tests/print_timeout.rs`. |
| **NFR-014** | `tauri-plugin-single-instance`; `src-tauri/src/cli/single_instance.rs`; a second launch hands its argv to the running instance instead of racing it over `queue.json`. |
| **NFR-034** | **Newly met.** `APP_SHORTCUTS` in `src/components/AppShell/MPAppShell.tsx` is a single table with three consumers that cannot disagree: `App` (owns the actions), `MPSidebar` (prints key caps), and Home (documents the full set). Six shortcuts cover the four primary actions the requirement names plus the two job entry points: `newJob` (N), `printFolder` (O), `print` (P), `switchProfile` (K), `openQueue` (J), `cancel` (`.`). `matchShortcut` requires exactly one platform modifier (⌘ on macOS, Ctrl elsewhere) with the other up, ignores auto-repeat, and refuses to fire while focus is in an `input`/`textarea`/`select`/`contenteditable`. `useAppShortcuts(handlers, enabled)` takes an `enabled` flag so modal surfaces (onboarding, confirmation dialogs) own the keyboard. Verified at two levels: component tests dispatch synthetic events at jsdom, and `e2e/specs/home.e2e.ts` asserts the layer is bound to a **real** window in the packaged app — the case jsdom cannot catch — including a test that Home documents every shortcut it implements. |
| **NFR-041** | Keyboard operability is asserted rather than assumed: quick actions are real `<button>`s (checked in `home.e2e.ts`, not decorative `div`s), `ConfirmDialog` traps Tab and Shift+Tab and lands focus on Cancel — the least destructive option — and the shortcut layer above means no primary action requires a pointer. |
| **NFR-042** | Every focusable primitive paints a focus ring via `.mp-focus`, asserted per component (`Switch`, `Checkbox`, `Select` test suites) rather than left to a global stylesheet. The ≥3:1 contrast figure itself is still manual. |
| **NFR-043** | Accessible names asserted across the primitives (`Checkbox`, `Select`, `Switch`, `Kbd` — including a spoken alternative for glyph-only keys) and at screen level (the Printers tablist, the reassign dropdown). |
| **NFR-044** | Semantics rather than styling: `ConfirmDialog` announces title and body through `role="dialog"`, `Switch` exposes switch semantics with its checked state, `Checkbox` exposes checkbox semantics named by its label, `Kbd` renders a real `<kbd>` with no interactive role. Screen-reader smoke testing remains manual. |
| **NFR-045** | State is never carried by colour alone: `Segmented` exposes selection through `aria-checked`, `Checkbox` reports indeterminate as `mixed`. |
| **NFR-071** | `src-tauri/src/platform/{windows.rs, cups.rs, macos/}`. |
| **NFR-081** | `logging::Policy` — daily + size roll, swept at startup and on every roll by `retain_days` (14) and `retain_files` (7), whichever bites first. |

### Not met, or unverified

| ID | Reality |
|---|---|
| **NFR-001 … NFR-008** | No performance or artifact-size measurement of any kind. There is no `benches/` directory, no `criterion`, no RAM assertion, and no installer to measure. |
| **NFR-021, NFR-022** | Nothing is code-signed and no updater artifact is signed. The updater is a *check only* against the GitHub releases API; the app does not self-install. |
| **NFR-027** | No gitleaks scan. Vacuously true today — `ci.yml` reads no secrets. |
| **NFR-040** | No axe-core, no Playwright. WCAG AA is approached through the per-component assertions listed above, not through an automated conformance scan. |
| **NFR-050** | Neither the ESLint rule nor the (non-existent) Rust attribute is in place; externalisation is enforced by review. |
| **NFR-051, NFR-052, NFR-053** | See the inline notes: no ICU, no gendered forms, adding a language touches code, no expansion testing. |
| **NFR-060** | Gated, but with `cargo llvm-cov` rather than tarpaulin, and the TS thresholds sit below the 60% target as a regression gate. |
| **NFR-061** | **`#![deny(missing_docs)]` is not enabled in either crate.** This is the requirement most likely to be assumed satisfied because the tree reads as though it were. It is not enforced; a new undocumented `pub` item compiles clean. Enabling it is a small, self-contained change. |
| **NFR-064** | No reproducibility verification. |
| **NFR-065** | No `cargo audit` / `pnpm audit` workflow. |
| **NFR-070** | Only native architectures are built in CI; no universal macOS binary. |
