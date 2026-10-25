import { defineConfig } from 'vite';
import path from 'node:path';
import { svetVitePlugin } from './src/engine/compiler.ts';

export default defineConfig({
  plugins: [svetVitePlugin()],
  server: {
    fs: {
      allow: [
        path.resolve(import.meta.dirname),
        path.resolve(import.meta.dirname, '../forma'),
      ],
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
