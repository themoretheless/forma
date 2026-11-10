import assert from 'node:assert/strict';
import { readdir, readFile, writeFile } from 'node:fs/promises';
import { dirname, relative, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { createHash } from 'node:crypto';
const [rebuilt, output, installation] = process.argv.slice(2);
assert(rebuilt && output, 'Expected REBUILT_WASM REPORT [INSTALLED_PACKAGE]');
const root = resolve(dirname(fileURLToPath(import.meta.url)), '..'),
  hash = bytes => createHash('sha256').update(bytes).digest('hex');
async function collectRustSources(dir) {
  const entries = await readdir(dir, { withFileTypes: true });
  const files = [];
  for (const entry of entries) {
    const full = resolve(dir, entry.name);
    if (entry.isDirectory()) {
      files.push(...(await collectRustSources(full)));
    } else if (entry.isFile() && entry.name.endsWith('.rs')) {
      files.push(relative(root, full).replace(/\\/g, '/'));
    }
  }
  return files.sort();
}
const report = {
  status: 'failed',
  rebuilt: resolve(rebuilt),
  installedPackage: installation ? resolve(installation) : null,
  sourceSha256: {},
  scope:
    'Byte/export identity of caller-rebuilt WASM, checked-in embedded source and optional installed public asset/runtime. Caller must first build current Rust sources with locked pinned toolchain.',
};
try {
  const rustSources = await collectRustSources(resolve(root, 'crates/svet-wasm/src'));
  for (const file of [
    ...rustSources,
    'crates/svet-wasm/Cargo.toml',
    'crates/svet-wasm/Cargo.lock',
    '../../rust-toolchain.toml',
  ])
    report.sourceSha256[file] = hash(await readFile(resolve(root, file)));
  const source = await readFile(resolve(root, 'src/engine/wasm-binary.ts'), 'utf8'),
    encoded = source.match(/WASM_BASE64: string = "([^"]+)"/),
    declared = source.match(/WASM_EXPORTS = (\[.*\]) as const/);
  assert(encoded && declared, 'Generated source format not recognized');
  const bytes = await readFile(resolve(rebuilt)),
    embedded = Buffer.from(encoded[1], 'base64');
  report.rebuiltSha256 = hash(bytes);
  report.embeddedSha256 = hash(embedded);
  report.bytes = bytes.length;
  assert(bytes.equals(embedded), 'Rebuilt WASM differs from checked-in embedded binary');
  const exports = WebAssembly.Module.exports(new WebAssembly.Module(bytes)).sort((a, b) =>
    a.name.localeCompare(b.name),
  );
  const sorted = list => list.slice().sort((a, b) => a.name.localeCompare(b.name));
  assert.deepEqual(
    exports,
    sorted(JSON.parse(declared[1])),
    'Embedded export declaration differs from rebuilt module',
  );
  report.exports = exports;
  if (installation) {
    const packageRoot = resolve(installation),
      metadata = JSON.parse(await readFile(resolve(packageRoot, 'package.json'), 'utf8'));
    assert.equal(metadata.name, 'svet-engine');
    const asset = await readFile(resolve(packageRoot, metadata.exports['./core.wasm'].default));
    assert(asset.equals(bytes), 'Installed public WASM asset differs from rebuilt binary');
    const runtime = await import(
      pathToFileURL(resolve(packageRoot, 'dist/package/wasm-binary.js')).href
    );
    assert(
      Buffer.from(runtime.decodeWasmBinary()).equals(bytes),
      'Installed embedded runtime differs from rebuilt binary',
    );
    assert.deepEqual(sorted(runtime.WASM_EXPORTS), exports, 'Installed export declaration differs');
    report.installedAssetSha256 = hash(asset);
  }
  report.status = 'passed';
} catch (error) {
  report.error = String(error.stack ?? error);
  process.exitCode = 1;
}
report.checkerSha256 = hash(await readFile(new URL(import.meta.url)));
await writeFile(resolve(output), JSON.stringify(report, null, 2) + '\n');
console.log(
  JSON.stringify({
    status: report.status,
    bytes: report.bytes,
    sha256: report.rebuiltSha256,
    error: report.error,
  }),
);
