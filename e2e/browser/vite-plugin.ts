/**
 * Vite plugin that injects the browser E2E fake host (`ipc-stub.ts`) into
 * `index.html`, ahead of the app's own entry module.
 *
 * Two independent guards keep it out of anything shipped:
 * - `apply: 'serve'` — Vite never runs it for `vite build`, so no production
 *   bundle can contain the stub whatever the environment says;
 * - it is a no-op unless `PLURY_E2E_STUB=1` is set or the server runs in the
 *   `e2e` mode (`pnpm dev:e2e` = `vite --mode e2e`, which behaves the same from
 *   a POSIX shell and from cmd/PowerShell), so the ordinary `pnpm dev` /
 *   `pnpm tauri dev` server is untouched.
 */

import type { HtmlTagDescriptor, Plugin } from 'vite';

export const E2E_STUB_ENV = 'PLURY_E2E_STUB';
export const E2E_MODE = 'e2e';
export const E2E_STUB_SRC = '/e2e/browser/ipc-stub.ts';

export function pluryE2eStub(env: Record<string, string | undefined>): Plugin {
  let enabled = env[E2E_STUB_ENV] === '1';
  return {
    name: 'plury-e2e-stub',
    apply: 'serve',
    configResolved(config) {
      if (config.mode === E2E_MODE) enabled = true;
    },
    transformIndexHtml(): HtmlTagDescriptor[] {
      if (!enabled) return [];
      // `head-prepend` + `type=module`: module scripts run in document order,
      // so the fake host is installed before `/src/main.tsx` evaluates.
      return [
        {
          tag: 'script',
          attrs: { type: 'module', src: E2E_STUB_SRC },
          injectTo: 'head-prepend',
        },
      ];
    },
  };
}
