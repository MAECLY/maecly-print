import '@testing-library/jest-dom/vitest';
import { toast } from 'sonner';
import { afterEach } from 'vitest';

// sonner keeps its toasts in a module-level store, and since 2.0.8 a `Toaster`
// replays every toast still active in it when it mounts (so a toast raised
// before the Toaster subscribed is not lost). A toast a test left on screen
// would then reappear in the next test's Toaster: retire them all after every
// test. This hook is registered before Testing Library's, so with Vitest's
// stacked hooks it runs after the tree is unmounted.
afterEach(() => {
  toast.dismiss();
});

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
