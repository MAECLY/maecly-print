# Coding Standards

> **TL;DR.** Rust code uses `rustfmt` defaults, `clippy --all-targets --all-features -- -D warnings`, edition 2024, MSRV 1.80, `anyhow` for app errors and `thiserror` for library boundaries, `tracing` for logging, `tokio` for async. TypeScript uses `strict: true`, ESLint + Prettier, function components only, Zustand for state, Tailwind for styling. Comments document **why**, not **what**.

This document is the style guide for all code in the repository. Linters enforce most of it; the human-readable rules below cover what tooling cannot.

## Rust

### Toolchain

| Setting | Value | Why |
|---|---|---|
| Toolchain | `stable` | Stable only. No nightly features. |
| Edition | `2024` | Latest edition; future-proof. |
| MSRV (Minimum Supported Rust Version) | `1.90` (app crate) | Far enough behind stable that most users have it; recent enough for `let-else`, `let-chains`, async-fn-in-trait. |
| Format | `rustfmt` (default config) | Stop arguing about formatting. |
| Lint | `cargo clippy --all-targets --all-features -- -D warnings` | All warnings are errors. |
| Audit | `cargo audit` (in CI) | Catches known vulnerable deps. |

`rust-toolchain.toml` pins the toolchain:

```toml
[toolchain]
channel = "stable"
components = ["rustfmt", "clippy"]
profile = "default"
```

### Module organization

Organize by **feature**, not by **layer**. See [repo-structure.md](./repo-structure.md#module-organization-by-feature-not-by-layer) for examples.

- One module per coherent concept (queue, profiles, engine, printer).
- Inside a module: types first, then `impl` blocks, then helpers, then `#[cfg(test)] mod tests`.
- Re-export public items at the module root; don't force callers to know internal layout.

### Naming

- Types: `UpperCamelCase` (`PrintJob`, `ProfileSchema`).
- Functions, methods, modules, files: `snake_case` (`enqueue_job`, `validate_profile`, `print_queue.rs`).
- Constants: `SCREAMING_SNAKE_CASE`.
- Generic type parameters: single capital letter (`T`, `E`) or descriptive UpperCamelCase (`Adapter`, `Output`).
- No Hungarian notation. No type prefixes (`StructName`, `EnumName`).
- No abbreviations except universally understood ones (`url`, `id`, `db`). Spell out `cfg` to `config` when it's a public item.

### Error handling

Two crates, two domains:

| Crate | Used in | Why |
|---|---|---|
| `anyhow` | Application code (`src-tauri/src/`) | Easy `?` propagation, contextual `.context()`, doesn't force callers to handle specific variants. |
| `thiserror` | Library boundaries (`engine` adapters return typed errors) | Callers can match on variants; serializable across IPC. |

Conventions:

- Always return `Result<T, anyhow::Error>` or `Result<T, MyTypedError>` from fallible functions. Never `panic!()` outside of programmer-error situations (e.g. `unreachable!()` after a verified-exhaustive match).
- Use `.context("descriptive failure point")` liberally. Errors should read like a stack trace of human intentions:
  ```
  failed to print job 4f2a
   caused by: failed to convert document to PDF
   caused by: LibreOffice exited with code 1
   caused by: stderr: cannot open input file
  ```
- Never `unwrap()` or `expect()` in non-test code unless the invariant is proven by surrounding code (and write a one-line comment explaining why it cannot fail).
- Custom error types use `#[derive(thiserror::Error, Debug)]` and implement `Display`. Keep them simple: 5-10 variants per type, max.

```rust
#[derive(thiserror::Error, Debug)]
pub enum PrintError {
    #[error("printer not found: {0}")]
    PrinterNotFound(String),

    #[error("unsupported file format: {0}")]
    UnsupportedFormat(String),

    #[error("adapter timed out after {0:?}")]
    Timeout(std::time::Duration),

    #[error("io error")]
    Io(#[from] std::io::Error),
}
```

### Logging

`tracing` only. Never `println!`, never `log` crate, never `eprintln!` in production code.

```rust
use tracing::{debug, info, warn, error, instrument};

#[instrument(skip(file_data), fields(file_size = file_data.len()))]
pub async fn print_pdf(printer: &str, file_data: Vec<u8>) -> Result<()> {
    info!("starting print job");
    // ...
    debug!(printer, "submitted to spooler");
    Ok(())
}
```

Levels:

- `error!`: something went wrong; user impact; needs investigation.
- `warn!`: something unexpected but recoverable; user may see degraded behavior.
- `info!`: notable lifecycle events (app start, job started/finished, settings change).
- `debug!`: detailed flow useful when debugging a specific issue.
- `trace!`: very verbose; never enabled in production.

Default subscriber outputs JSON-lines to a rotated file. The file format is part of the support contract (we ask users to share log files); changes go in CHANGELOG.

### Async

`tokio` only. No `async-std`, no `smol`. Use the multi-thread runtime (`#[tokio::main(flavor = "multi_thread")]`) at the application root.

- Spawn long-running tasks with `tokio::spawn`. Use `tokio::task::JoinSet` for groups.
- Channels: `tokio::sync::mpsc` (single-producer in our use cases). Avoid `crossbeam` channels (good crate but mixing with tokio bridges is friction).
- Mutexes: `tokio::sync::Mutex` if held across `await`, `std::sync::Mutex` (or `parking_lot`) for synchronous critical sections.
- Cancellation: prefer `tokio_util::sync::CancellationToken` over racing futures.

### Tests

- Unit tests: `#[cfg(test)] mod tests { ... }` at the bottom of each module file.
- Integration tests: separate `.rs` files under `src-tauri/tests/`, each becoming its own test binary.
- Test naming: `fn test_<scenario>_<expected>()`. Example: `fn test_print_pdf_invalid_path_returns_error()`.
- Mocks: prefer hand-rolled fakes over `mockall`. Mocks are a code smell; if you need many, the design is probably wrong.
- Snapshot tests: use `insta` for output that is hard to assert structurally (e.g. log line format).
- No `#[ignore]` in main branch. If a test is broken, fix it or delete it; ignored tests rot.

### Comments

Default: **no comments**. Code should explain itself through naming.

When to comment:

- **Why**, not what. The why is opaque; the what is in the code.
- Non-obvious invariants. "Caller must hold the queue lock."
- Workarounds for upstream bugs. Always link the upstream issue.
- Unsafe blocks. Mandatory: justify every `unsafe`.
- Public API. Every `pub` item gets at least one rustdoc line.

When **not** to comment:

- Redundant restatement. `// increment counter` next to `counter += 1` is noise.
- Outdated speculation. Comments rot; code is the truth.

Rustdoc style:

```rust
/// Enqueues a print job and returns a handle for cancellation.
///
/// Jobs are processed in FIFO order. The queue is bounded; if full, this
/// returns [`QueueError::Full`] without blocking.
pub async fn enqueue(&self, job: Job) -> Result<JobHandle, QueueError> {
    // ...
}
```

### Crate-level guidelines

- Pin direct dependency major versions in `Cargo.toml` (`serde = "1"`, not `serde = "*"`).
- Avoid feature flags unless we genuinely conditionally compile something. Most "feature" abuse is just preemptive optionality.
- Prefer the standard library over external crates when possible. Use `std::path::Path`, not a separate path crate.
- Audit transitive dependencies on every cargo-update PR. `cargo tree -d` shows duplicates; investigate and unify when possible.

### Unsafe

Allowed only in the FFI module that calls Win32 APIs (`printer/windows.rs` for `PrintDlg`/`StartDoc` paths). Every unsafe block:

1. Has a `// SAFETY: ...` comment explaining the invariants we uphold.
2. Is the smallest possible scope (don't wrap a 30-line block; wrap the single dangerous call).
3. Has a sound, safe wrapper exposed to the rest of the crate.

```rust
// SAFETY: hwnd is a valid window handle obtained from BeginPaint earlier in this function.
// We do not retain the handle beyond this call.
unsafe { windows_sys::Win32::UI::WindowsAndMessaging::SetForegroundWindow(hwnd); }
```

## TypeScript

### Toolchain

| Setting | Value | Why |
|---|---|---|
| TypeScript | 5.x | Latest stable. |
| `strict` | `true` | All strict checks on. |
| `noUncheckedIndexedAccess` | `true` | `arr[i]` is `T \| undefined`, forcing explicit handling. |
| `exactOptionalPropertyTypes` | `true` | `{ x?: number }` differs from `{ x?: number \| undefined }`. |
| Format | Prettier (default config + 100 line width) | Same rationale as rustfmt. |
| Lint | ESLint with `@typescript-eslint`, `eslint-plugin-react-hooks`, `eslint-plugin-jsx-a11y` | All warnings as errors in CI. |

`tsconfig.json` core:

```json
{
  "compilerOptions": {
    "target": "ES2022",
    "lib": ["ES2022", "DOM", "DOM.Iterable"],
    "module": "ESNext",
    "moduleResolution": "Bundler",
    "jsx": "react-jsx",
    "strict": true,
    "noUncheckedIndexedAccess": true,
    "exactOptionalPropertyTypes": true,
    "noImplicitOverride": true,
    "noPropertyAccessFromIndexSignature": true,
    "isolatedModules": true,
    "skipLibCheck": true,
    "esModuleInterop": true,
    "resolveJsonModule": true
  }
}
```

### React

- **Function components only.** No class components. Hooks are the model.
- Use `function MyComponent() {}` declarations, not arrow function assignments to `const`. Easier to find with `Ctrl+F MyComponent` (the keyword anchor).
- Props interfaces named `<ComponentName>Props` and exported when the component is exported.
- Key prop on every list item. Use stable IDs, never array indices.
- Memoization (`useMemo`, `useCallback`, `React.memo`) only when there's a measured perf problem. Premature memoization adds complexity without benefit.

```tsx
interface ProfileCardProps {
  profile: Profile;
  onSelect: (id: string) => void;
}

export function ProfileCard({ profile, onSelect }: ProfileCardProps) {
  return (
    <button onClick={() => onSelect(profile.id)} className="p-4 border rounded">
      <h3 className="font-medium">{profile.name}</h3>
    </button>
  );
}
```

### State management

- **Zustand** is the chosen store library. (Jotai was a runner-up; Zustand won on simplicity for this app size.)
- One store per major domain: `useQueueStore`, `useProfilesStore`, `useSettingsStore`.
- No Redux. Too much ceremony for our scale.
- Keep server state (Tauri IPC results) in store fields, not in `useState` local state, when multiple components need it.
- Selectors with shallow comparison: `const items = useQueueStore((s) => s.items);` (Zustand's default selector is shallow).

### Styling

- **Tailwind CSS** for everything except a tiny `globals.css` that imports Tailwind base/components/utilities.
- No CSS-in-JS (no `styled-components`, no `emotion`). They add bundle size and runtime cost.
- No CSS modules. Tailwind classes alone for utility, `@apply` only when a class set genuinely repeats 3+ times in unrelated components.
- Theme tokens (colors, spacing) defined in `tailwind.config.ts`, never hardcoded.

### File naming

- Components: `PascalCase.tsx` (`ProfileCard.tsx`, `PrintJob.tsx`).
- Hooks: `useCamelCase.ts` (`usePrintQueue.ts`, `useUpdater.ts`).
- Utilities: `camelCase.ts` (`formatBytes.ts`, `parseProfile.ts`).
- Types-only modules: `kebab-case.types.ts` (`print-job.types.ts`).
- Test files: same name with `.test.ts` or `.test.tsx` suffix, co-located with the source.

### Imports

Order (auto-sorted by Prettier + `prettier-plugin-organize-imports`):

1. Built-in / external packages (`react`, `zustand`, `@tauri-apps/api`).
2. Internal modules with absolute alias (`@/components/Button`).
3. Internal modules with relative path (`./helpers`).
4. Type-only imports last, with `import type`.

No barrel files (`index.ts` re-exports). They break tree-shaking and IDE go-to-definition.

### Async / Promises

- Always `await` promises. Never `.then()` chains in new code.
- Never silently swallow errors. Either propagate (let `try/catch` higher up handle) or log explicitly.
- Use `AbortSignal` for cancellable operations. The Tauri IPC layer supports it.

### Comments

Same philosophy as Rust: comment **why**, not **what**. JSDoc is required only for exported functions in `lib/` utility modules where types alone don't convey intent.

```ts
/**
 * Formats a file size for display in the UI.
 *
 * Uses base-2 units (KiB, MiB) consistent with what File Explorer shows.
 * For the user-facing string only — never use this for serialization.
 */
export function formatBytes(bytes: number): string {
  // ...
}
```

## Commits

### Conventional Commits

We use [Conventional Commits](https://www.conventionalcommits.org/) 1.0.0 strictly.

```
<type>(<scope>): <subject>

<body>

<footer>
```

Allowed types:

| Type | Use for |
|---|---|
| `feat` | New feature visible to users |
| `fix` | Bug fix |
| `docs` | Documentation only |
| `style` | Formatting, missing semis, no logic change |
| `refactor` | Code change that neither fixes a bug nor adds a feature |
| `perf` | Performance improvement |
| `test` | Adding or fixing tests |
| `chore` | Tooling, deps, repo housekeeping |
| `build` | Build system or external deps (Cargo.toml, package.json) |
| `ci` | CI config changes |

Scope is optional but encouraged: `feat(queue):`, `fix(printer/windows):`, `docs(roadmap):`.

Subject line:

- ≤72 characters.
- Imperative mood ("add", not "added" or "adds").
- No period at the end.
- Lowercase first letter (after the colon).

Body (optional):

- Wrapped at 80 columns.
- Explain **why**, not what (the diff shows what).
- Reference issues with `Fixes #123` or `Refs #456`.

Footer (optional):

- `BREAKING CHANGE: <description>` for breaking changes (also bumps to MAJOR per SemVer).
- `Signed-off-by:` (added by `git commit -s`) is welcome but optional; contributions come in under the [CLA](../../CLA.md), not a sign-off.

Example:

```
fix(printer/windows): handle paper-out errors without panicking

Previously, an unrecoverable spooler error caused the queue thread to
panic and abort the entire job batch. Now we surface the error to the
UI and continue with the next job.

Fixes #142

Signed-off-by: Jane Doe <jane@example.com>
```

### Atomic commits

One logical change per commit. Squash-merge into `main` produces a single commit per PR; the PR title becomes the squashed commit subject (use Conventional Commits format for PR titles too).

## Branches

| Branch | Purpose |
|---|---|
| `main` | Stable, always green. The only long-lived branch. |
| `feature/<short-description>` | One feature, lifetime <1 week. |
| `fix/<short-description>` | One bug fix, lifetime <1 week. |
| `release/<MAJOR.MINOR>` | Stabilization branch for high-risk releases (optional). |
| `hotfix/<version>` | Patch on a previously released MINOR. |

Use kebab-case for the descriptive part: `feature/queue-priority`, `fix/win10-shell-crash`.

Delete branches after merge.

## PR rules

- One reviewer approval required (initially the maintainer; project will scale this).
- All CI checks green.
- The [CLA](../../CLA.md) confirmed in the pull request template.
- Linear history (squash merge).
- PR description follows the template (`Description`, `Motivation`, `Testing`, `Screenshots`).

See [contributing.md](./contributing.md) for the full PR workflow.

## Tooling configuration files

| File | Purpose |
|---|---|
| `rust-toolchain.toml` | Pins Rust version |
| `rustfmt.toml` | Empty (use defaults) |
| `clippy.toml` | Per-lint allowlist exceptions if any |
| `.eslintrc.json` | TypeScript + React lint config |
| `.prettierrc.json` | Prettier config (line width 100) |
| `.editorconfig` | Cross-editor whitespace rules |
| `.gitattributes` | Line endings + binary marks |

## See also

- [repo-structure.md](./repo-structure.md)
- [local-setup.md](./local-setup.md)
- [contributing.md](./contributing.md)
- [testing-strategy.md](./testing-strategy.md)
