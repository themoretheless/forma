import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import { preview } from 'vite';
import {
  createActionFixture,
  createResumptionFixture,
  createStreamingFixture,
} from './browser-fixture-endpoints.mjs';
const qualification = JSON.parse(
  await readFile(new URL('../dist/package-verification.json', import.meta.url), 'utf8'),
);
const consumer = process.argv[2] ?? qualification.consumerDirectory;
assert(consumer, 'Run npm run test:package -- --keep first');
const root = resolve(consumer),
  metadata = JSON.parse(await readFile(resolve(root, 'dist/metadata.json'), 'utf8'));
assert.equal(metadata.protocol, 'installed-package-browser-v1');
const engine = await import(
  pathToFileURL(resolve(root, 'node_modules/svet-engine/dist/package/index.js')).href
);
const provenance = {
  packageSha256: metadata.packageSha256,
  installedRuntimeSha256: metadata.installedRuntimeSha256,
  endpointSourceSha256: createHash('sha256')
    .update(await readFile(new URL('./browser-fixture-endpoints.mjs', import.meta.url)))
    .digest('hex'),
};
const serverEngine = await import(
  pathToFileURL(resolve(root, 'node_modules/svet-engine/dist/package/server.js')).href
);
const actions = createActionFixture(),
  resumption = createResumptionFixture(
    { ...engine, renderToStringAsync: serverEngine.renderToStringAsync },
    provenance,
  ),
  streaming = createStreamingFixture(
    { ...engine, renderToChunksAsync: serverEngine.renderToChunksAsync },
    provenance,
  );
const server = await preview({
  root,
  configFile: false,
  build: { outDir: resolve(root, 'dist') },
  preview: { host: '127.0.0.1', port: 4206, strictPort: true },
  plugins: [
    {
      name: 'svet-installed-fixture-endpoints',
      configurePreviewServer(vite) {
        vite.middlewares.use(actions.handle);
        vite.middlewares.use(resumption.handle);
        vite.middlewares.use(streaming.handle);
      },
    },
  ],
});
console.log(
  JSON.stringify(
    {
      consumer: root,
      ...provenance,
      url: 'http://127.0.0.1:4206/browser-matrix.html',
      ssr: 'http://127.0.0.1:4206/resumption-roundtrip.html',
      http: 'http://127.0.0.1:4206/server-action-http.html',
    },
    null,
    2,
  ),
);
server.printUrls();
let closing = false;
const close = () => {
  if (closing) return;
  closing = true;
  actions.close();
  resumption.close();
  streaming.close();
  server.httpServer.closeAllConnections();
  server.httpServer.close();
};
process.once('SIGINT', close);
process.once('SIGTERM', close);
