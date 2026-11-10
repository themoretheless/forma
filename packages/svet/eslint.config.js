import js from '@eslint/js';
import globals from 'globals';
import tseslint from 'typescript-eslint';

// Engine sources get the typed rule set; tests and scripts the plain recommended rules.
// Findings that predate the linter are recorded in eslint-suppressions.json and may only shrink.
export default tseslint.config(
  {
    ignores: [
      'dist/**',
      'node_modules/**',
      'benchmarks/**',
      'crates/**',
      'src/engine/wasm-binary.ts',
      'tests/**/fixtures/**',
    ],
  },
  js.configs.recommended,
  ...tseslint.configs.recommended,
  {
    files: ['src/**/*.{ts,tsx}', 'tests/**/*.{ts,tsx}'],
    languageOptions: { globals: { ...globals.browser, ...globals.node } },
  },
  // Scripts and edge tests drive browsers and pass callbacks that run in the page.
  {
    files: ['scripts/**/*.mjs', 'tests/**/*.mjs', '*.js', '*.mjs'],
    languageOptions: { globals: { ...globals.node, ...globals.browser } },
  },
  {
    files: ['src/engine/**/*.ts'],
    languageOptions: {
      parserOptions: { projectService: true, tsconfigRootDir: import.meta.dirname },
    },
    rules: {
      '@typescript-eslint/no-floating-promises': 'error',
      '@typescript-eslint/switch-exhaustiveness-check': 'error',
    },
  },
);
