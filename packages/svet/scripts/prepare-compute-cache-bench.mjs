import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { mkdir, readFile, readdir, writeFile } from 'node:fs/promises';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import ts from 'typescript';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const source = resolve(root, 'src/engine');
const output = resolve(root, 'node_modules/.cache/svet-compute-cache-bench');
const revision = process.argv[2] ?? 'HEAD';
const baselineGpu = execFileSync('git', ['show', `${revision}:packages/svet/src/engine/gpu.ts`], {
  cwd: root,
  encoding: 'utf8',
});
const candidateGpu = await readFile(resolve(source, 'gpu.ts'), 'utf8');
if (
  baselineGpu.includes('computePipelineCaches') ||
  !candidateGpu.includes('computePipelineCaches')
) {
  throw Error('Choose a pre-cache baseline revision and a candidate containing the pipeline cache');
}
const snapshot = `snapshot-${Date.now()}`;
const metadata = {
  snapshot,
  createdAt: new Date().toISOString(),
  baselineRevision: execFileSync('git', ['rev-parse', revision], {
    cwd: root,
    encoding: 'utf8',
  }).trim(),
  node: process.version,
  typescript: ts.version,
  fixtureSha256: createHash('sha256')
    .update(await readFile(resolve(root, 'tests/browser/compute-cache-bench.html')))
    .digest('hex'),
  chainFixtureSha256: createHash('sha256')
    .update(await readFile(resolve(root, 'tests/browser/compute-chain-bench.html')))
    .digest('hex'),
  variants: {},
  scope: 'Only gpu.ts differs; all other current engine sources and embedded WASM are identical.',
};
for (const variant of ['baseline', 'candidate']) {
  const directory = resolve(output, snapshot, variant);
  await mkdir(directory, { recursive: true });
  const hashes = {};
  for (const file of (await readdir(source)).filter(file => file.endsWith('.ts')).sort()) {
    const text =
      file === 'gpu.ts'
        ? variant === 'baseline'
          ? baselineGpu
          : candidateGpu
        : await readFile(resolve(source, file), 'utf8');
    const result = ts.transpileModule(text, {
      fileName: file,
      compilerOptions: {
        target: ts.ScriptTarget.ES2022,
        module: ts.ModuleKind.ESNext,
        rewriteRelativeImportExtensions: true,
      },
    });
    hashes[file] = {
      source: createHash('sha256').update(text).digest('hex'),
      javascript: createHash('sha256').update(result.outputText).digest('hex'),
    };
    await writeFile(resolve(directory, file.replace(/\.ts$/, '.js')), result.outputText);
  }
  metadata.variants[variant] = hashes;
}
await writeFile(resolve(output, 'metadata.json'), JSON.stringify(metadata, null, 2) + '\n');
console.log(
  JSON.stringify(
    {
      output,
      snapshot,
      baselineRevision: metadata.baselineRevision,
      fixture: '/tests/browser/compute-cache-bench.html',
    },
    null,
    2,
  ),
);
