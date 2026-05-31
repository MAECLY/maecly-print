import '@testing-library/jest-dom/vitest';

// jsdom runs on an opaque origin (about:blank), where `window.localStorage`
// is not exposed. The app guards its own access in try/catch, but tests need
// it to persist UI flags (e.g. the onboarding-complete marker). Provide a
// minimal in-memory shim when the environment doesn't supply one.
if (typeof window !== 'undefined' && !window.localStorage) {
  const store = new Map<string, string>();
  Object.defineProperty(window, 'localStorage', {
    configurable: true,
    value: {
      getItem: (key: string) => (store.has(key) ? store.get(key)! : null),
      setItem: (key: string, value: string) => {
        store.set(key, String(value));
      },
      removeItem: (key: string) => {
        store.delete(key);
      },
      clear: () => store.clear(),
      key: (index: number) => Array.from(store.keys())[index] ?? null,
      get length() {
        return store.size;
      },
    },
  });
}
