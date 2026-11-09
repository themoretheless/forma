import { createHash } from 'node:crypto';
import { mkdir, readFile, readdir, writeFile } from 'node:fs/promises';
import { dirname, resolve, relative } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';
import { build, preview } from 'vite';
const { values } = parseArgs({
  options: {
    serve: { type: 'boolean', default: false },
    port: { type: 'string', default: '4205' },
    fixture: { type: 'string', default: 'event-instances' },
    installed: { type: 'string' },
  },
});
const root = resolve(dirname(fileURLToPath(import.meta.url)), '..'),
  port = Number(values.port);
if (!Number.isInteger(port) || port < 1024 || port > 65535) throw Error('Invalid preview port');
if (
  ![
    'event-instances',
    'cross-copy-boundaries',
    'cross-copy-router',
    'history-native-replay',
    'retained-ui-gc',
    'cross-core-errors',
  ].includes(values.fixture)
)
  throw Error('Unknown fixture');
const fixture = `tests/browser/${values.fixture}.html`;
const cache = resolve(root, 'node_modules/.cache/svet-event-instances'),
  snapshot = `snapshot-${Date.now()}`,
  output = resolve(cache, snapshot, 'dist');
const hash = data => createHash('sha256').update(data).digest('hex');
const files = (await readdir(resolve(root, 'src/engine')))
  .filter(file => file.endsWith('.ts'))
  .sort()
  .map(file => `src/engine/${file}`);
files.push(fixture, 'scripts/prepare-event-instances.mjs', 'vite.config.ts', 'package.json');
const installed = values.installed ? resolve(values.installed) : null;
const entry = installed
  ? resolve(installed, 'dist/package/index.js')
  : resolve(root, 'src/engine/index.ts');
if (installed)
  for (const file of await readdir(resolve(installed, 'dist/package')))
    files.push(resolve(installed, 'dist/package', file));
const hashes = {};
for (const file of files) hashes[file] = hash(await readFile(resolve(root, file)));
await build({
  root,
  configFile: resolve(root, 'vite.config.ts'),
  build: { outDir: output, emptyOutDir: true, rollupOptions: { input: resolve(root, fixture) } },
});
for (const variant of ['first', 'second'])
  await build({
    root,
    configFile: false,
    build: {
      outDir: resolve(output, variant),
      emptyOutDir: true,
      lib: { entry, formats: ['es'], fileName: () => 'index.js' },
    },
  });
for (const file of files)
  if (hashes[file] !== hash(await readFile(resolve(root, file))))
    throw Error(`Source changed while building: ${file}`);
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
  installedPackage: installed,
  snapshot,
  createdAt: new Date().toISOString(),
  node: process.version,
  vite: vite.version,
  sourceSha256: hashes,
  artifactSha256: artifacts,
  scope:
    'Two independently compiled engines (installedPackage identifies installed archive, otherwise current source) at distinct URLs in one document. Byte-identical source, separate module/WASM state; shared DOM dispatcher qualification.',
};
await writeFile(resolve(output, 'metadata.json'), JSON.stringify(metadata, null, 2) + '\n');
await mkdir(cache, { recursive: true });
await writeFile(
  resolve(cache, 'latest.json'),
  JSON.stringify({ snapshot, output, url: `http://127.0.0.1:${port}/${fixture}` }, null, 2) + '\n',
);
console.log(
  JSON.stringify({ snapshot, output, url: `http://127.0.0.1:${port}/${fixture}` }, null, 2),
);
if (values.serve) {
  const server = await preview({
    root,
    configFile: false,
    build: { outDir: output },
    preview: { host: '127.0.0.1', port, strictPort: true },
  });
  server.printUrls();
}
