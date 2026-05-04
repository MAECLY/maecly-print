# Non-Functional Requirements

> **TL;DR** — Quality attributes: how fast, how reliable, how secure, how accessible, how maintainable. Targets are concrete and testable; vague aspirations are not allowed here.

Non-functional requirements (NFRs) are the "ilities" that the system must satisfy across all features. Functional requirements (FRs) live in [functional-requirements.md](./functional-requirements.md).

Each NFR has a stable ID (`NFR-XXX`), a measurable target, and a verification method.

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
| NFR-012 | A converter timeout MUST NOT leak processes; orphan child processes MUST be killed within 5 s of timeout | Zero orphan PIDs after `cargo test --features stress` | Process audit |
| NFR-013 | The system MUST tolerate the disappearance of an external dependency mid-job (e.g., LibreOffice uninstalled) by failing the job cleanly with a typed error | Test passes with `pkill soffice` mid-conversion | Chaos test |
| NFR-014 | The system MUST support running a single instance only. A second invocation MUST send its arguments to the running instance via IPC and exit. | Pass: spawn 10 invocations in parallel, only 1 process exists | Concurrency test |

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
| NFR-027 | Secrets (signing private keys, future API tokens) MUST NEVER be committed; CI uses GitHub Secrets / OIDC. | gitleaks scan in CI | CI gate |

## 4. Usability

| ID | Requirement | Target | Verification |
|---|---|---|---|
| NFR-030 | A first-time user MUST be able to print a single PDF from the context menu in <30 seconds end-to-end after install | Usability test n=5, all complete | Recorded user test |
| NFR-031 | All defaults MUST be sane such that the application is useful without opening Settings | Verified by zero-config quick start in user test | Same |
| NFR-032 | Destructive actions (delete profile, clear history) MUST require confirmation with a clear "Undo" affordance for 5 seconds. | UI assertion in component tests | Vitest |
| NFR-033 | Error messages MUST be actionable: state what went wrong, what the user can do, and provide a link to logs. Vague "Something went wrong" is forbidden. | PR review checklist + lint rule for empty `.message` | Code review |
| NFR-034 | The system MUST provide keyboard shortcuts for all primary actions (print, cancel, switch profile, open queue). | Documented in Help → Shortcuts | Manual QA |
| NFR-035 | All UI text MUST avoid jargon when a plain-language equivalent exists; jargon allowed in Advanced Settings. | Editorial pass before each release | Release checklist |

## 5. Accessibility

| ID | Requirement | Target | Verification |
|---|---|---|---|
| NFR-040 | The GUI MUST conform to **WCAG 2.1 Level AA** | axe-core scan in Playwright e2e returns zero violations on every screen | CI |
| NFR-041 | The GUI MUST be fully operable with a keyboard alone (no mouse required) | Manual keyboard-only walkthrough in release checklist | Manual QA |
| NFR-042 | All interactive controls MUST have visible focus indicators with ≥3:1 contrast against background | axe-core + manual review | CI + QA |
| NFR-043 | All non-decorative images and icons MUST have text alternatives (`aria-label`, `alt`, etc.) | axe-core | CI |
| NFR-044 | The application MUST be compatible with screen readers (NVDA on Windows, VoiceOver on macOS) | Manual smoke test with screen reader before each release | Release checklist |
| NFR-045 | Color MUST NOT be the only carrier of meaning; status uses icon + label + color. | Component review | Code review |

## 6. Internationalization

| ID | Requirement | Target | Verification |
|---|---|---|---|
| NFR-050 | All user-facing strings MUST be externalized in `i18n/{lang}.json`; no string literals in JSX/Rust user-facing paths | ESLint rule + Rust `#[deny(string_literal_in_user_text)]` macro check | CI |
| NFR-051 | The translation framework MUST support pluralization, gendered forms, and date/number formatting via ICU MessageFormat | Format string reference works on test fixtures | Unit tests |
| NFR-052 | The system MUST support adding a new language by adding a single `i18n/<lang>.json` file with no code changes | Documented in [contributing.md](../08-development/contributing.md) | Repro test |
| NFR-053 | Layouts MUST handle text expansion of +50% without truncation or overlap (test with German pseudolocale) | Snapshot tests at 100% and 150% text size | Vitest |
| NFR-054 | The system SHOULD be RTL-ready by v2.0 (logical CSS properties, no hard-coded `left`/`right`) | RTL pseudolocale snapshot test | Future |

## 7. Maintainability

| ID | Requirement | Target | Verification |
|---|---|---|---|
| NFR-060 | Test coverage MUST be ≥70% for Rust code, ≥60% for TS code | `cargo tarpaulin` and `vitest --coverage` | CI gate |
| NFR-061 | All public Rust functions MUST have rustdoc with at least a one-line summary | `#![deny(missing_docs)]` for `pub` items | CI |
| NFR-062 | Documentation in this `docs/` folder MUST stay in sync with code; PRs that modify behavior MUST update affected docs | Reviewer checklist | Code review |
| NFR-063 | The CI MUST be green for a PR to be merged (no override without maintainer approval and reason in commit) | Branch protection rule | GitHub settings |
| NFR-064 | Build MUST be reproducible: same source + same toolchain version → byte-identical binary | `cargo build --locked --release` is reproducible | Quarterly verification |
| NFR-065 | Deps MUST be audited weekly via `cargo audit` and `pnpm audit`; high/critical vulnerabilities MUST be fixed within 7 days. | Scheduled CI workflow | CI |

## 8. Portability

| ID | Requirement | Target | Verification |
|---|---|---|---|
| NFR-070 | A single codebase MUST support Windows (x64, ARM64) and macOS (Apple Silicon, Intel) | All four targets build and pass tests in CI | CI matrix |
| NFR-071 | Platform-specific code MUST be isolated to clearly named modules (`platform/windows/`, `platform/macos/`) | File layout audit | Code review |
| NFR-072 | The system MUST NOT require Linux support in v1; Linux MAY be added in v2 if community demand exists | Documented in [milestones.md](../06-roadmap/milestones.md) | Roadmap |

## 9. Observability

| ID | Requirement | Target | Verification |
|---|---|---|---|
| NFR-080 | All log entries MUST be structured (JSONL) with: timestamp (ISO 8601), level, target/module, message, optional fields | `tracing-subscriber` JSON formatter | Unit test on log format |
| NFR-081 | Logs MUST rotate at 10 MB or daily, whichever first; retain last 7 files | `tracing-appender::rolling` | Manual review of rotation |
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
