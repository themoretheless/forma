import js from '@eslint/js';
import globals from 'globals';

// Hand-written JavaScript only; svet has its own TypeScript toolchain and the crates have clippy.
// Existing findings are recorded in eslint-suppressions.json and may only shrink.
export default [
  {
    ignores: [
      'dist/**',
      'public/**',
      'target/**',
      '.forma/**',
      '.release/**',
      'output/**',
      'node_modules/**',
      'packages/**',
      'crates/**',
      'native-app/**',
      'bench/results/**',
    ],
  },
  js.configs.recommended,
  {
    files: ['src/**/*.js', 'bench/**/*.js'],
    languageOptions: { ecmaVersion: 'latest', sourceType: 'module', globals: globals.browser },
  },
  {
    files: ['test/**/*.js', 'scripts/**/*.mjs', 'mcp/**/*.js', '*.js'],
    languageOptions: { ecmaVersion: 'latest', sourceType: 'module', globals: globals.node },
  },
  // These suites install fake window and document objects on globalThis before using them.
  {
    files: ['test/element-tools.test.js', 'test/vector-preview.test.js'],
    languageOptions: { globals: { window: 'readonly', document: 'readonly' } },
  },
  {
    rules: {
      'no-unused-vars': ['error', { argsIgnorePattern: '^_', caughtErrors: 'none' }],
      'no-empty': ['error', { allowEmptyCatch: true }],
      'prefer-const': 'error',
      'no-var': 'error',
      eqeqeq: ['error', 'always', { null: 'ignore' }],
    },
  },
];
