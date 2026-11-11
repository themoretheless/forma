import { defineConfig } from 'vitest/config';
import path from 'node:path';
import { svetVitePlugin } from './src/engine/compiler.ts';

export default defineConfig({
  plugins: [svetVitePlugin({ runtimeImportSource: 'svet' })],
  build: { outDir: 'dist/demo' },
  test: {
    projects: [
      {
        extends: true,
        test: {
          name: 'unit',
          include: ['tests/**/*.test.ts'],
          exclude: [
            'tests/event-adoption.test.ts',
            'tests/precompiled-wasm.test.ts',
            'tests/slim-entry.test.ts',
            'tests/ssr-carrier.test.ts',
            'tests/ssr-deep-tree.test.ts',
          ],
          isolate: false,
        },
      },
      {
        extends: true,
        test: {
          name: 'isolated',
          include: [
            'tests/event-adoption.test.ts',
            'tests/precompiled-wasm.test.ts',
            'tests/slim-entry.test.ts',
            'tests/ssr-carrier.test.ts',
            'tests/ssr-deep-tree.test.ts',
          ],
          isolate: true,
        },
      },
    ],
  },
  server: {
    fs: {
      allow: [path.resolve(import.meta.dirname), path.resolve(import.meta.dirname, '../forma')],
    },
  },
  resolve: {
    alias: {
      '@forma': path.resolve(import.meta.dirname, '../forma'),
      'svet/jsx-runtime': path.resolve(import.meta.dirname, 'src/engine/jsx-runtime.ts'),
      'svet/jsx-dev-runtime': path.resolve(import.meta.dirname, 'src/engine/jsx-runtime.ts'),
      svet: path.resolve(import.meta.dirname, 'src/engine/index.ts'),
    },
  },
  oxc: {
    jsx: {
      runtime: 'automatic',
      importSource: 'svet',
    },
  },
});
