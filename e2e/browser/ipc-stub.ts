/**
 * Browser E2E harness: a fake Tauri host for the Vite dev server.
 *
 * Loaded as the first module script of `index.html` when Vite runs with
 * `PLURY_E2E_STUB=1` (`pnpm dev:e2e`; see the `pluryE2eStub` plugin in
 * `vite.config.ts`, which never applies to `vite build`). It installs
 * `window.__TAURI_INTERNALS__` before the app boots, so `isTauri()` is true and
 * every `@tauri-apps/api` call (invoke, events, dialogs, webview metadata)
 * lands on the in-memory {@link FakeBackend}.
 *
 * Test drivers (Chrome DevTools MCP, Playwright, a human in the console) reach
 * the backend through `window.__PLURY_E2E__`:
 *
 * ```js
 * __PLURY_E2E__.backend.queue            // live queue state
 * __PLURY_E2E__.backend.tick()           // advance the queue by hand
 * __PLURY_E2E__.backend.openPaths = [..] // what the next file picker returns
 * __PLURY_E2E__.forwardSelection({ paths: ['/x.pdf'], action: 'configure', profile_id: null })
 * __PLURY_E2E__.dropFiles(['/x.pdf'])    // a native drag-and-drop
 * ```
 *
 * URL query flags: `?e2e-seed=1` starts from the fixture queue (every job
 * state), `?e2e-tick=0` stops the timer, `?e2e-onboarded=1` skips onboarding.
 */

import type { Selection } from '@/ipc/bindings';

import { FakeBackend, type FakeBackendOptions } from './fake-backend';

type Callback = (payload: unknown) => void;

interface Listener {
  event: string;
  handlerId: number;
  eventId: number;
}

export interface E2eHarness {
  backend: FakeBackend;
  /** Emit an event to the app's listeners, as the Rust side would. */
  emit: (event: string, payload: unknown) => void;
  forwardSelection: (sel: Selection) => void;
  /** Simulate a native file drop onto the window. */
  dropFiles: (paths: string[]) => void;
  /** Every command the app invoked, oldest first. */
  calls: () => { cmd: string; args: unknown }[];
}

declare global {
  interface Window {
    __PLURY_E2E__?: E2eHarness;
  }
}

const WINDOW_LABEL = 'main';

/**
 * Install the fake host on `win` and return the harness handle. Exported so
 * the harness itself can be unit-tested under jsdom.
 */
export function installIpcStub(
  win: Window & typeof globalThis,
  opts: FakeBackendOptions = {},
): E2eHarness {
  const backend = new FakeBackend(opts);
  const callbacks = new Map<number, Callback>();
  const listeners: Listener[] = [];
  let nextId = 1;

  const runCallback = (id: number, data: unknown) => callbacks.get(id)?.(data);

  const emit = (event: string, payload: unknown) => {
    for (const l of listeners.filter((x) => x.event === event)) {
      runCallback(l.handlerId, { event, id: l.eventId, payload });
    }
  };
  backend.connect(emit);

  const dialog = (cmd: string, args: Record<string, unknown>): unknown => {
    if (cmd === 'plugin:dialog|open') {
      const options = (args.options ?? {}) as {
        multiple?: boolean;
        directory?: boolean;
      };
      const paths = backend.openPaths;
      if (paths.length === 0) return null;
      return options.multiple ? [...paths] : paths[0];
    }
    if (cmd === 'plugin:dialog|save') return backend.savePath;
    if (
      cmd === 'plugin:dialog|message' ||
      cmd === 'plugin:dialog|ask' ||
      cmd === 'plugin:dialog|confirm'
    ) {
      return true;
    }
    throw { code: 'Internal', message: `not stubbed: ${cmd}` };
  };

  const invoke = async (
    cmd: string,
    args: Record<string, unknown> = {},
  ): Promise<unknown> => {
    if (cmd === 'plugin:event|listen') {
      const eventId = nextId++;
      listeners.push({
        event: args.event as string,
        handlerId: args.handler as number,
        eventId,
      });
      return eventId;
    }
    if (cmd === 'plugin:event|unlisten') {
      const at = listeners.findIndex(
        (l) => l.event === args.event && l.eventId === args.eventId,
      );
      if (at >= 0) listeners.splice(at, 1);
      return null;
    }
    if (cmd === 'plugin:event|emit' || cmd === 'plugin:event|emit_to') {
      emit(args.event as string, args.payload);
      return null;
    }
    if (cmd.startsWith('plugin:dialog|')) return dialog(cmd, args);
    if (cmd.startsWith('plugin:')) {
      // Window/webview/app plugin calls the app may make incidentally
      // (focus, title, version). Resolving null keeps them harmless.
      return null;
    }
    return backend.invoke(cmd, args);
  };

  const internals = {
    invoke,
    transformCallback(cb?: Callback, once = false): number {
      const id = nextId++;
      callbacks.set(id, (data) => {
        if (once) callbacks.delete(id);
        cb?.(data);
      });
      return id;
    },
    unregisterCallback(id: number) {
      callbacks.delete(id);
    },
    runCallback,
    callbacks,
    convertFileSrc(path: string, protocol = 'asset') {
      return `${protocol}://localhost/${encodeURIComponent(path)}`;
    },
    metadata: {
      currentWindow: { label: WINDOW_LABEL },
      currentWebview: { windowLabel: WINDOW_LABEL, label: WINDOW_LABEL },
    },
    plugins: {},
  };

  const w = win as unknown as Record<string, unknown>;
  w.__TAURI_INTERNALS__ = internals;
  w.__TAURI_EVENT_PLUGIN_INTERNALS__ = {
    unregisterListener(_event: string, eventId: number) {
      const l = listeners.find((x) => x.eventId === eventId);
      if (l) callbacks.delete(l.handlerId);
    },
  };

  const harness: E2eHarness = {
    backend,
    emit,
    forwardSelection: (sel) => backend.forwardSelection(sel),
    dropFiles: (paths) =>
      emit('tauri://drag-drop', { paths, position: { x: 200, y: 200 } }),
    calls: () => [...backend.calls],
  };
  win.__PLURY_E2E__ = harness;
  return harness;
}

/** Read the `?e2e-*` flags from the page URL. */
export function optionsFromUrl(search: string): FakeBackendOptions {
  const q = new URLSearchParams(search);
  const opts: FakeBackendOptions = {};
  if (q.get('e2e-seed') === '1') opts.seedQueue = true;
  const tick = q.get('e2e-tick');
  if (tick !== null && Number.isFinite(Number(tick)))
    opts.tickMs = Number(tick);
  return opts;
}

// Self-install when loaded by the dev server (not when imported by a test).
if (typeof window !== 'undefined' && !import.meta.env?.VITEST) {
  installIpcStub(window, optionsFromUrl(window.location.search));
  if (
    new URLSearchParams(window.location.search).get('e2e-onboarded') === '1'
  ) {
    try {
      // Keep in sync with ONBOARD_KEY in src/App.tsx.
      window.localStorage.setItem('pluryprint:onboarding-complete', '1');
    } catch {
      // Storage blocked: onboarding simply shows.
    }
  }
  console.info('[plury-e2e] fake Tauri host installed');
}
