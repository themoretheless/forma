// Builds the Svet WASM core the one way every consumer must build it, then embeds it.
//
// `--no-embed` stops after the build (CI rebuilds only to compare bytes). The build honours
// CARGO_TARGET_DIR. Source paths are remapped to be relative to the repository root: without
// that the binary carries the absolute path of the checkout in its panic locations, so two
// machines can never produce the same bytes.
import { spawnSync } from 'node:child_process';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const packageRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const repositoryRoot = resolve(packageRoot, '../..');
const crate = resolve(packageRoot, 'crates/svet-wasm');

function run(command, args, env) {
  const result = spawnSync(command, args, { cwd: crate, env, stdio: 'inherit' });
  if (result.error) throw result.error;
  if (result.status !== 0) process.exit(result.status ?? 1);
}

run('cargo', ['build', '--locked', '--target', 'wasm32-unknown-unknown', '--release'], {
  ...process.env,
  RUSTFLAGS: `-C target-feature=+simd128 --remap-path-prefix=${repositoryRoot}=.`,
});
if (!process.argv.includes('--no-embed')) {
  run(process.execPath, [resolve(packageRoot, 'scripts/embed-wasm.mjs')], process.env);
}
