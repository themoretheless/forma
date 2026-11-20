import { createHash } from 'node:crypto';
import { mkdir, readFile, readdir, writeFile } from 'node:fs/promises';
import { dirname, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';
import { build, preview } from 'vite';

const { values } = parseArgs({
  options: {
    serve: { type: 'boolean', default: false },
    port: { type: 'string', default: '4208' },
  },
});

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const port = Number(values.port);
if (!Number.isInteger(port) || port < 1024 || port > 65535) throw Error('Invalid preview port');

const cache = resolve(root, 'node_modules/.cache/svet-product-app-bench');
const snapshot = `snapshot-${Date.now()}`;
const output = resolve(cache, snapshot, 'dist');
const hash = data => createHash('sha256').update(data).digest('hex');

const files = (await readdir(resolve(root, 'src/engine')))
  .filter(file => file.endsWith('.ts'))
  .sort()
  .map(file => `src/engine/${file}`);
files.push(
  'tests/browser/product-app-bench.html',
  'scripts/prepare-product-app-bench.mjs',
  'vite.config.ts',
  'package.json',
);

const hashes = {};
for (const file of files) hashes[file] = hash(await readFile(resolve(root, file)));

await build({
  root,
  configFile: resolve(root, 'vite.config.ts'),
  build: {
    outDir: output,
    emptyOutDir: true,
    rollupOptions: { input: resolve(root, 'tests/browser/product-app-bench.html') },
  },
});

for (const file of files) {
  if (hashes[file] !== hash(await readFile(resolve(root, file)))) {
    throw Error(`Source changed while building: ${file}`);
  }
}

const artifacts = {};
async function scan(directory) {
  for (const entry of await readdir(directory, { withFileTypes: true })) {
    const path = resolve(directory, entry.name);
    if (entry.isDirectory()) await scan(path);
    else artifacts[relative(output, path)] = hash(await readFile(path));
  }
}
await scan(output);

const vite = JSON.parse(await readFile(resolve(root, 'node_modules/vite/package.json'), 'utf8'));
const metadata = {
  snapshot,
  createdAt: new Date().toISOString(),
  node: process.version,
  vite: vite.version,
  sourceSha256: hashes,
  artifactSha256: artifacts,
  scope:
    'Frozen production bundle measuring real Svet + Forma product app startup, p95 interaction latency, rAF frame time, and WASM/DOM memory.',
};

await writeFile(resolve(output, 'metadata.json'), JSON.stringify(metadata, null, 2) + '\n');
await mkdir(cache, { recursive: true });
const latest = {
  snapshot,
  output,
  url: `http://127.0.0.1:${port}/tests/browser/product-app-bench.html`,
};
await writeFile(resolve(cache, 'latest.json'), JSON.stringify(latest, null, 2) + '\n');
console.log(JSON.stringify(latest, null, 2));

if (values.serve) {
  const server = await preview({
    root,
    configFile: false,
    build: { outDir: output },
    preview: { host: '127.0.0.1', port, strictPort: true },
  });
  server.printUrls();
}
