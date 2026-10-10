import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import { configDefaults } from 'vitest/config';
import path from 'node:path';

import { pluryE2eStub } from './e2e/browser/vite-plugin.ts';

const host = process.env.TAURI_DEV_HOST;

export default defineConfig(async () => ({
  // pluryE2eStub is dev-server only and inert unless PLURY_E2E_STUB=1
  // (`pnpm dev:e2e`): it injects the browser E2E fake Tauri host.
  plugins: [react(), pluryE2eStub(process.env)],
  resolve: {
    alias: {
      '@': path.resolve(import.meta.dirname, 'src'),
    },
  },
  clearScreen: false,
  server: {
    port: 1421,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: 'ws',
          host,
          port: 1422,
        }
      : undefined,
    watch: {
      ignored: ['**/src-tauri/**'],
    },
  },
  envPrefix: ['VITE_', 'TAURI_ENV_*'],
  build: {
    target:
      process.env.TAURI_ENV_PLATFORM === 'windows' ? 'chrome105' : 'safari13',
    minify: !process.env.TAURI_ENV_DEBUG ? 'oxc' : false,
    sourcemap: !!process.env.TAURI_ENV_DEBUG,
  },
  test: {
    environment: 'jsdom',
    globals: true,
    setupFiles: ['./src/test/setup.ts'],
    // Scope to unit/component tests under src, and explicitly exclude the
    // WebdriverIO e2e suite. The e2e specs are named *.e2e.ts (so vitest's
    // default {test,spec} glob never matches them) and only run via
    // `pnpm test:e2e`; the belt-and-suspenders include/exclude keeps
    // `pnpm test` unit-only even if a spec is renamed.
    // The one exception under e2e/ is the browser harness's own unit tests
    // (e2e/browser/*.test.ts), which are plain Vitest.
    include: ['src/**/*.{test,spec}.{ts,tsx}', 'e2e/browser/**/*.test.ts'],
    exclude: [...configDefaults.exclude, 'e2e/specs/**'],
    coverage: {
      provider: 'v8',
      reporter: ['text', 'html', 'lcov'],
      reportsDirectory: './coverage',
      // Keep non-product code out of the denominator. Target is ~60% lines
      // (docs/08-development/testing-strategy.md); informational, not gated.
      exclude: [
        'e2e/**',
        'dist/**',
        'node_modules/**',
        'src/ipc/bindings.ts',
        'src/test/**',
        'src/**/*.test.{ts,tsx}',
        'src/main.tsx',
        'src/vite-env.d.ts',
        '**/*.config.{ts,js}',
      ],
    },
  },
}));
