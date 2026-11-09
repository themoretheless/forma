import { rm, writeFile } from 'node:fs/promises';
import { createRequire } from 'node:module';
import { dirname, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { spawnSync } from 'node:child_process';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const require = createRequire(import.meta.url);
await rm(resolve(root, 'dist/package'), { recursive: true, force: true });
const result = spawnSync(
  process.execPath,
  [require.resolve('typescript/bin/tsc'), '-p', 'tsconfig.package.json'],
  {
    cwd: root,
    stdio: 'inherit',
  },
);
if (result.error) throw result.error;
if (result.status !== 0) process.exit(result.status ?? 1);

const { decodeWasmBinary } = await import(
  pathToFileURL(resolve(root, 'dist/package/wasm-binary.js')).href
);
await writeFile(resolve(root, 'dist/package/svet-core.wasm'), decodeWasmBinary());
await writeFile(
  resolve(root, 'dist/package/svet-core.wasm.d.ts'),
  'declare const module: WebAssembly.Module;\nexport default module;\n',
);
