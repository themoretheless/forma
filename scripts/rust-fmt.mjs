import { spawnSync } from 'node:child_process';
import { execFileSync } from 'node:child_process';
import { root } from './versions.mjs';

// There is no Cargo workspace yet, so `cargo fmt --all` has nothing to walk: every tracked
// manifest is formatted on its own. Frozen source snapshots kept as benchmark evidence are not
// part of any build and stay byte-for-byte as recorded.
const EVIDENCE = /\/(benchmarks|qualification)\//;
export const manifests = () =>
  execFileSync('git', ['ls-files', '*Cargo.toml'], { cwd: root, encoding: 'utf8' })
    .split('\n')
    .filter(file => file && !EVIDENCE.test(file));

if (import.meta.url === `file://${process.argv[1]}`) {
  const check = process.argv.includes('--check');
  const failed = [];
  for (const manifest of manifests()) {
    const args = ['fmt', '--manifest-path', manifest, ...(check ? ['--', '--check'] : [])];
    const result = spawnSync('cargo', args, { cwd: root, encoding: 'utf8' });
    if (result.status !== 0) {
      failed.push(manifest);
      process.stderr.write(`${manifest}\n${result.stdout}${result.stderr}`);
    }
  }
  if (failed.length) {
    console.error(`rustfmt: ${failed.length} manifest(s) need formatting: ${failed.join(', ')}`);
    process.exit(1);
  }
  console.log(`rustfmt: ${manifests().length} manifests ${check ? 'are formatted' : 'formatted'}`);
}
