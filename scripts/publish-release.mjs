import { spawnSync } from 'node:child_process';
import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { createHash } from 'node:crypto';
import { root, release, versions } from './versions.mjs';
const current = versions(),
  tag = process.env.GITHUB_REF_NAME;
if (process.env.GITHUB_REF_TYPE !== 'tag') throw Error('Releases require a pushed tag');
const info = release(tag, current);
// Both bundles were built and tested by the CI run this workflow reused; publish exactly those.
const names = [`forma-${info.version}-wasm.tar.gz`, `forma-studio-${info.version}-web.tar.gz`];
const files = names.flatMap(name => [name, `${name}.sha256`]);
for (const file of files)
  if (!existsSync(resolve(root, '.release', file)))
    throw Error(`Missing tested CI artifact ${file}`);
for (const name of names) {
  const expected = readFileSync(resolve(root, '.release', `${name}.sha256`), 'utf8').split(
    /\s+/,
  )[0];
  if (
    createHash('sha256')
      .update(readFileSync(resolve(root, '.release', name)))
      .digest('hex') !== expected
  )
    throw Error(`Release artifact checksum mismatch: ${name}`);
}
const title = `Forma ${info.version}`;
const notes = `${title}\n\nCommit: ${process.env.GITHUB_SHA}\n\n${names[0]}: WASM bindings and binary. Rust sources are available at this tag; no crates.io/npm publication.\n\n${names[1]}: static Studio build including the tested WASM runtime. Native launch, Rust execution and MCP need the local development server; they are not part of the static bundle.\n`;
writeFileSync(resolve(root, '.release/notes.md'), notes);
const args = [
  'release',
  'create',
  tag,
  ...files.map(file => resolve(root, '.release', file)),
  '--verify-tag',
  '--title',
  title,
  '--notes-file',
  resolve(root, '.release/notes.md'),
];
if (info.prerelease) args.push('--prerelease');
const result = spawnSync('gh', args, { cwd: root, stdio: 'inherit' });
if (result.error || result.status !== 0)
  throw Error(`GitHub release failed: ${result.error?.message ?? result.status}`);
