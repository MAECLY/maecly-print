# Changelog

All notable changes to MaeclyPrint will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Initial repository scaffold: `package.json`, `pnpm-workspace.yaml`, `tsconfig.json`, `vite.config.ts`, `tailwind.config.ts`, ESLint 9 flat config, Prettier, Husky + lint-staged + commitlint.
- Tauri 2 backend skeleton in `src-tauri/` with utility modules (`paths`, `error`, `logging`, `settings`) and a stub IPC command (`app_info`) wired through tauri-specta.
- React + TypeScript frontend scaffold in `src/`: design tokens (CSS variables), `Brand`, `Icon`, and 15 UI primitives ported 1:1 from `claude design/maeclyprint/project/`.
- Six main screens (Home, PrintJob, Queue, Profiles, Settings, About) and onboarding flow rendered with mock data.
- GitHub Actions CI for `windows-latest`: lint, typecheck, vitest, cargo test, cargo clippy, tauri build (unsigned).
- Community files: `CONTRIBUTING.md`, `CODE_OF_CONDUCT.md`, `SECURITY.md`, issue/PR templates, CODEOWNERS, dependabot.

### Notes

- This is the M0 scaffold milestone. The application opens an empty window and shows mock screens; printing is **not** wired yet — that lands in the M1 plan.
- Local prerequisites for running `pnpm tauri dev`: Rust stable 1.78+, Node 20 LTS, pnpm 9, Visual Studio Build Tools 2022 with C++ workload + Windows 11 SDK + ATL. See `docs/08-development/local-setup.md`.

## [0.0.1] - 2026-04-26

### Added

- Initial commit with `LICENSE` (GPLv3) and the documentation tree under `docs/`.
- Claude Design handoff bundle under `claude design/maeclyprint/`.
