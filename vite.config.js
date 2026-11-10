import { defineConfig, searchForWorkspaceRoot } from 'vite';
import { realpathSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { formaMcpBridgePlugin } from './dev-server/index.js';

// The language package is linked from the sibling tokenizer checkout (scripts/tokenizer.mjs); its
// JS reaches Vite through imports, but the WASM it fetches by URL is outside the root and got 403.
const tokenizerPackage = (() => {
  try {
    return realpathSync(
      fileURLToPath(new URL('./node_modules/@themoretheless/tokenizer-forma', import.meta.url)),
    );
  } catch {
    return null;
  }
})();

// Pre-bundling moves the package's JS into .vite/deps, where its relative WASM URL no longer resolves.
export default defineConfig({
  optimizeDeps: { exclude: ['@themoretheless/tokenizer-forma'] },
  build: {
    target: 'esnext',
    chunkSizeWarningLimit: 600,
    rollupOptions: {
      input: { studio: 'index.html', controls: 'crates/forma/examples/controls.html' },
      output: {
        manualChunks(id) {
          if (id.includes('@codemirror/') || id.includes('@lezer/')) return 'editor';
        },
      },
    },
  },
  server: {
    host: '127.0.0.1',
    port: 5173,
    strictPort: true,
    fs: {
      allow: [
        searchForWorkspaceRoot(fileURLToPath(new URL('.', import.meta.url))),
        ...(tokenizerPackage ? [tokenizerPackage] : []),
      ],
    },
  },
  plugins: [formaMcpBridgePlugin()],
});
