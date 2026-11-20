// @ts-check
import { spawnSync } from 'node:child_process';
import { existsSync, readdirSync, readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { root } from './versions.mjs';

const TOP_LEVEL_FILES = [
  'design-data-runner.js',
  'form-project.js',
  'rust-runner.js',
  'vector-runner.js',
];
const SCANNED_DIRS = ['src', 'protocol', 'dev-server'];

function collectJsFiles(dir) {
  const base = resolve(root, dir);
  if (!existsSync(base)) return [];
  /** @type {string[]} */
  const out = [];
  for (const entry of readdirSync(base, { withFileTypes: true })) {
    const rel = `${dir}/${entry.name}`;
    if (entry.isDirectory()) out.push(...collectJsFiles(rel));
    else if (entry.isFile() && entry.name.endsWith('.js')) out.push(rel);
  }
  return out;
}

export function tsCheckedFiles() {
  const files = [
    ...TOP_LEVEL_FILES.filter(f => existsSync(resolve(root, f))),
    ...SCANNED_DIRS.flatMap(collectJsFiles),
  ].sort();
  return files.filter(file =>
    /^\/\/\s*@ts-check\b/m.test(readFileSync(resolve(root, file), 'utf8').slice(0, 512)),
  );
}

function resolveTscPath() {
  for (const candidate of [
    'node_modules/typescript/bin/tsc',
    'packages/svet/node_modules/typescript/bin/tsc',
  ]) {
    const full = resolve(root, candidate);
    if (existsSync(full)) return full;
  }
  return null;
}

export function runTypecheck() {
  const tsc = resolveTscPath();
  if (!tsc) {
    throw Error(
      'TypeScript compiler (tsc) not found in node_modules or packages/svet/node_modules',
    );
  }
  const result = spawnSync(
    process.execPath,
    [tsc, '-p', resolve(root, 'jsconfig.json'), '--noEmit'],
    {
      cwd: root,
      encoding: 'utf8',
      maxBuffer: 32 << 20,
    },
  );
  if (result.status !== 0) {
    const output = `${result.stdout ?? ''}${result.stderr ?? ''}`.trim();
    throw Error(`TypeScript check failed:\n${output}`);
  }
  return tsCheckedFiles();
}

const invoked =
  process.argv[1] && import.meta.url === new URL(`file://${resolve(process.argv[1])}`).href;
if (invoked) {
  const checked = runTypecheck();
  console.log(`typecheck: ${checked.length} @ts-check modules verified (0 errors)`);
}
