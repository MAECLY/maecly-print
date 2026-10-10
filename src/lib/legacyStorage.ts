/**
 * One-time move of the `localStorage` keys written under the product's former
 * name, MaeclyPrint (ADR-0009).
 *
 * Every key the app keeps (`theme`, `density`, `onboarding-complete`,
 * `locale-detected`) was prefixed `maeclyprint:` and is now `pluryprint:`.
 * A key moves only when the new one is not set yet — never overwrite — and the
 * old key is removed either way, so a second run finds nothing to do.
 *
 * The packaged app also changed its bundle identifier, which gives the webview
 * a new storage folder; there the old keys are simply not visible and this is
 * a no-op. It matters wherever the origin stayed the same (`pnpm dev`, the
 * browser preview, the e2e stub). For the packaged upgrade, what these keys
 * cached lives in `config.toml` too: theme and density in `[app]`, and the
 * first-run flags `locale_detected` / `onboarding_complete`, which the data
 * migration sets for an upgrader (`src-tauri/src/util/legacy.rs`).
 */

const LEGACY_PREFIX = 'maeclyprint:';
const PREFIX = 'pluryprint:';

/** Move every `maeclyprint:*` key to `pluryprint:*`. Returns the moved keys. */
export function migrateLegacyStorage(given?: Storage | null): string[] {
  const moved: string[] = [];
  try {
    // Reading `window.localStorage` itself throws where storage is blocked.
    const storage =
      given === undefined
        ? typeof window === 'undefined'
          ? null
          : window.localStorage
        : given;
    if (!storage) return moved;
    const legacy: string[] = [];
    for (let i = 0; i < storage.length; i += 1) {
      const key = storage.key(i);
      if (key?.startsWith(LEGACY_PREFIX)) legacy.push(key);
    }
    for (const key of legacy) {
      const next = PREFIX + key.slice(LEGACY_PREFIX.length);
      const value = storage.getItem(key);
      if (value !== null && storage.getItem(next) === null) {
        storage.setItem(next, value);
        moved.push(next);
      }
      storage.removeItem(key);
    }
  } catch {
    /* storage blocked: there is nothing to move */
  }
  return moved;
}
