import { defineConfig } from 'eslint/config';
import js from '@eslint/js';
import globals from 'globals';
import jsxA11y from 'eslint-plugin-jsx-a11y';
import reactHooks from 'eslint-plugin-react-hooks';
import { reactRefresh } from 'eslint-plugin-react-refresh';
import tseslint from 'typescript-eslint';
import prettier from 'eslint-config-prettier/flat';

export default defineConfig(
  {
    ignores: [
      'dist',
      'coverage',
      'node_modules',
      'src-tauri/target',
      'src-tauri/gen',
      'src/ipc/bindings.ts',
      'docs',
      'claude design',
      // Agent worktrees are whole checkouts of this repo (with their own
      // node_modules and build output); linting them from here would lint
      // every branch twice and pick up their generated files.
      '.claude/**',
    ],
  },
  {
    files: ['**/*.{ts,tsx}'],
    extends: [
      js.configs.recommended,
      tseslint.configs.recommended,
      // The Rules of Hooks plus the React Compiler checks (refs read or
      // written during render, setState called synchronously in an effect,
      // impure render code, ...), as errors.
      reactHooks.configs.flat.recommended,
      prettier,
    ],
    languageOptions: {
      ecmaVersion: 2022,
      globals: globals.browser,
    },
    plugins: {
      'react-refresh': reactRefresh.plugin,
    },
    rules: {
      'react-refresh/only-export-components': [
        'warn',
        { allowConstantExport: true },
      ],
      '@typescript-eslint/no-unused-vars': [
        'error',
        { argsIgnorePattern: '^_', varsIgnorePattern: '^_' },
      ],
    },
  },
  {
    // Accessibility gate required by docs/08-development/coding-standards.md.
    // Only JSX carries markup, so the plugin is scoped to .tsx files.
    // TODO: the recommended rules are temporarily downgraded to 'warn' while the
    // existing violations are split across the team; raise the severity map to
    // 'error' at the end of the accessibility campaign so CI blocks regressions.
    files: ['**/*.tsx'],
    plugins: { 'jsx-a11y': jsxA11y },
    rules: Object.fromEntries(
      Object.entries(jsxA11y.configs.recommended.rules).map(([rule, entry]) => {
        const [severity, ...options] = Array.isArray(entry) ? entry : [entry];
        // Rules the preset deliberately disables stay disabled.
        const downgraded = severity === 'off' ? 'off' : 'warn';
        return [
          rule,
          options.length > 0 ? [downgraded, ...options] : downgraded,
        ];
      }),
    ),
  },
  {
    // E2E specs run under WebdriverIO + Mocha in Node, not the browser. The
    // wdio globals (browser/$/expect) are imported from '@wdio/globals'; Mocha
    // (describe/it/beforeEach) and Node builtins are ambient.
    files: ['e2e/**/*.ts'],
    languageOptions: {
      globals: { ...globals.node, ...globals.mocha },
    },
  },
);
