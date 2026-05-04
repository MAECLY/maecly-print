import type { Config } from 'tailwindcss';

/**
 * Tailwind covers layout (flex, grid, gap, spacing) only.
 * Design tokens (colors, fonts, shadows) live as CSS custom properties
 * in src/styles/tokens.css and are referenced via var(--mp-*).
 * This preserves 1:1 fidelity with the Claude Design mockups in
 * `claude design/maeclyprint/project/brand.jsx`.
 */
const config: Config = {
  content: ['./index.html', './src/**/*.{ts,tsx}'],
  darkMode: 'class',
  theme: {
    extend: {},
  },
  plugins: [],
  corePlugins: {
    preflight: false,
  },
};

export default config;
