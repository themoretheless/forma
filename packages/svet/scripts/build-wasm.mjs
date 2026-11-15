// Builds the Svet WASM core the one way every consumer must build it, then embeds it.
//
// `--no-embed` stops after the build (CI rebuilds only to compare bytes). The build honours
// CARGO_TARGET_DIR. Invoking `rustc` directly from the repository root with fixed crate metadata
// and repository-relative paths keeps the output byte-identical across checkout paths and OSes
// (Cargo hashes the absolute path of path-dependencies into `-C metadata`, and `rlib` alongside
// `cdylib` disables Fat LTO). `cargo metadata --locked` still enforces the lockfile and triggers
// rustup's `rust-toolchain.toml` installation on fresh runners.
import { mkdirSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const packageRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const repositoryRoot = resolve(packageRoot, '../..');
const crate = resolve(packageRoot, 'crates/svet-wasm');
const targetRoot = process.env.CARGO_TARGET_DIR
  ? resolve(process.cwd(), process.env.CARGO_TARGET_DIR)
  : resolve(crate, 'target');
const releaseDir = resolve(targetRoot, 'wasm32-unknown-unknown/release');
const depsDir = resolve(releaseDir, 'deps');

function run(command, args, cwd = repositoryRoot, env = process.env, stdio = 'inherit') {
  const result = spawnSync(command, args, { cwd, env, stdio });
  if (result.error) throw result.error;
  if (result.status !== 0) process.exit(result.status ?? 1);
}

run(
  'cargo',
  ['metadata', '--locked', '--format-version', '1', '--no-deps'],
  crate,
  { ...process.env, CARGO_TERM_COLOR: process.env.CARGO_TERM_COLOR ?? 'always' },
  ['ignore', 'ignore', 'inherit'],
);

mkdirSync(depsDir, { recursive: true });

const commonFlags = [
  '--edition=2024',
  '--target',
  'wasm32-unknown-unknown',
  '-C',
  'opt-level=z',
  '-C',
  'lto=fat',
  '-C',
  'codegen-units=1',
  '-C',
  'panic=abort',
  '-C',
  'strip=symbols',
  '-C',
  'target-feature=+simd128',
  `--remap-path-prefix=${repositoryRoot}=.`,
];

const rlibs = [
  ['svet_graph', 'crates/svet-graph/src/lib.rs', ['--forbid=unsafe_code']],
  ['svet_reconcile', 'crates/svet-reconcile/src/lib.rs', ['--forbid=unsafe_code']],
  ['svet_gpu_arena', 'crates/svet-gpu-arena/src/lib.rs', []],
];

for (const [name, source, extra] of rlibs) {
  run('rustc', [
    '--crate-name',
    name,
    source,
    '--crate-type',
    'rlib',
    ...commonFlags,
    '-C',
    'embed-bitcode=yes',
    '-C',
    `metadata=${name}`,
    ...extra,
    '-o',
    resolve(depsDir, `lib${name}.rlib`),
  ]);
}

run('rustc', [
  '--crate-name',
  'svet_wasm',
  'packages/svet/crates/svet-wasm/src/lib.rs',
  '--crate-type',
  'cdylib',
  ...commonFlags,
  '-C',
  'metadata=svet_wasm',
  '-L',
  `dependency=${depsDir}`,
  ...rlibs.flatMap(([name]) => ['--extern', `${name}=${resolve(depsDir, `lib${name}.rlib`)}`]),
  '-o',
  resolve(releaseDir, 'svet_wasm.wasm'),
]);

if (!process.argv.includes('--no-embed')) {
  run(process.execPath, [resolve(packageRoot, 'scripts/embed-wasm.mjs')], repositoryRoot, {
    ...process.env,
    CARGO_TARGET_DIR: targetRoot,
  });
}
