/**
 * Canonical project identity. Centralized so the About screen, the sidebar,
 * the issue reporter, and update-check links all agree without re-typing the
 * repository anywhere.
 *
 * `REPO_URL` must stay in sync with the crate's `repository` field in
 * `src-tauri/Cargo.toml` — that is what Rust reads (`CARGO_PKG_REPOSITORY`)
 * for the update check and the prefilled issue URL, so those two are the only
 * places the slug exists. `AboutScreen.test.tsx` reads Cargo.toml and fails if
 * they ever drift apart.
 *
 * Every URL here is under `github.com`, which the Rust `open_external`
 * allow-list permits. The `blob`/`tree` links target `main` (the default
 * branch) and paths that really exist in the tree: `LICENSE` and `docs/`.
 */

export const REPO_URL = 'https://github.com/MAECLY/maecly-print';
export const RELEASES_URL = REPO_URL + '/releases';
export const LICENSE_URL = REPO_URL + '/blob/main/LICENSE';
export const DOCS_URL = REPO_URL + '/tree/main/docs';

/**
 * The app's own SPDX license id, as declared in `src-tauri/Cargo.toml` and
 * `package.json`. Rendered in the About screen twice (the Components row and
 * the footer), so it lives here rather than being typed out per surface;
 * `AboutScreen.test.tsx` asserts it still matches both manifests.
 *
 * It is an identifier, not prose: it stays untranslated in every locale.
 */
export const APP_LICENSE = 'AGPL-3.0';

/**
 * The single placeholder shown wherever the app version is rendered but not
 * available: while `app_info` is still in flight, or outside the Tauri host
 * (the standalone Vite preview) where there is no backend to ask. Deliberately
 * *not* a version number — the running version is authoritative from Rust
 * (`app_info().version`, sourced from `CARGO_PKG_VERSION`), and a hardcoded
 * frontend number would silently lie after the first release bump.
 */
export const UNKNOWN_VERSION = '—';
