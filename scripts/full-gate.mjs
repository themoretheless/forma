import { spawnSync, execSync } from 'node:child_process';
import { existsSync, readFileSync } from 'node:fs';
import { hostname, release, cpus, loadavg } from 'node:os';
import { resolve } from 'node:path';
import { root, versions } from './versions.mjs';
import { runtimeSourceDigest, artifactDigests, runtimeArtifacts } from './runtime-digest.mjs';
import { PACKAGE_LINK, linkTokenizerPackage, tokenizerDir } from './tokenizer.mjs';

// CI sat red on two of three runner platforms for eleven runs while every local command stayed
// green, because no single local step builds the native bin for a foreign target. This is that
// step: the whole matrix, one exit code, and an environment block so a printed result says where
// it came from.
const CI_TARGETS = ['x86_64-unknown-linux-gnu', 'x86_64-pc-windows-msvc'];
// Platform code no CI runner builds: the Android JNI module and the iOS branches of the binary.
// Nothing compiled them, so they could rot unseen. Each runs when its target is installed.
const MOBILE_TARGETS = [
  {
    target: 'aarch64-linux-android',
    id: 'mobile-android',
    name: 'Android library (JNI entry points) compiles',
    args: ['--lib'],
  },
  {
    target: 'aarch64-apple-ios-sim',
    id: 'mobile-ios',
    name: 'iOS simulator binary compiles',
    args: ['--features', 'native', '--bin', 'forma'],
  },
];
const BUNDLED = ['build-info.json', ...runtimeArtifacts];
// Workspace members besides forma; compute-mlx and math-compute have steps of their own.
export const WORKSPACE_CRATES = [
  'gpu-shader',
  'gpu-runtime',
  'raster-core',
  'osv-math',
  'tensor-core',
  'compute-core',
  'compute-cuda',
  'memory-budget',
  'swap-store',
];
export function steps({ platform, installedTargets, viteEntry }) {
  return {
    list: [
      {
        id: 'tokenizer',
        name: 'Sibling tokenizer matches the release locked in Cargo.lock',
        argv: process.execPath,
        args: ['scripts/tokenizer.mjs', 'check'],
        requiresOutput: true,
      },
      {
        id: 'version-check',
        name: 'Version manifests agree with the pinned toolchain',
        argv: process.execPath,
        args: ['scripts/versions.mjs', 'check'],
        requiresOutput: true,
      },
      {
        id: 'studio-js',
        name: 'Studio test suite (node --test)',
        argv: process.execPath,
        args: ['--test', 'test/*.test.js'],
        requiresSummary: true,
      },
      // Every designer commit message ends with "the build is clean"; this is where that stops
      // being a promise. Running vite through its own entry keeps the step platform-neutral.
      ...(viteEntry
        ? [
            {
              id: 'studio-build',
              name: 'Studio production build ships the tested runtime',
              argv: process.execPath,
              args: [viteEntry, 'build'],
              checksDist: true,
            },
          ]
        : []),
      // Formatting is checked, not trusted: an unformatted file turns every later diff into noise.
      {
        id: 'format-js',
        name: 'JavaScript and TypeScript are formatted (prettier)',
        argv: process.execPath,
        args: ['node_modules/prettier/bin/prettier.cjs', '--check', '--log-level', 'warn', '.'],
      },
      // Findings that predate the linter are listed in eslint-suppressions.json; a new one fails.
      {
        id: 'lint-js',
        name: 'JavaScript passes ESLint (suppressions may only shrink)',
        argv: process.execPath,
        args: ['node_modules/eslint/bin/eslint.js', '--max-warnings', '0', '.'],
      },
      {
        id: 'format-rust',
        name: 'Every tracked Rust manifest is formatted (rustfmt)',
        argv: process.execPath,
        args: ['scripts/rust-fmt.mjs', '--check'],
        requiresOutput: true,
      },
      {
        id: 'layering',
        name: 'Workspace crates depend only on the layers below them',
        argv: process.execPath,
        args: ['scripts/check-layering.mjs'],
        requiresOutput: true,
      },
      // Every load and compile number in PRODUCTION_READINESS.md was produced by these three, and
      // nothing ran them: the suite covers the APIs they call, so a bench could die on a refactor and
      // the discovery would come mid-measurement. They write into .forma, never into a tracked path.
      {
        id: 'bench-js-compiler',
        name: 'Compiler bench runs and reports a median',
        argv: process.execPath,
        args: ['scripts/compiler-bench.mjs', '.forma/perf/gate/compiler'],
        requiresOutput: true,
      },
      {
        id: 'bench-js-session',
        name: 'Session-compiler bench runs across scene sizes',
        argv: process.execPath,
        args: ['scripts/compiler-session-bench.mjs', '.forma/perf/gate/compiler-session'],
        requiresOutput: true,
      },
      {
        id: 'bench-js-tools',
        name: 'Designer-tools bench runs on its fixed fixtures',
        argv: process.execPath,
        args: ['scripts/studio-tools-bench.mjs', '.forma/perf/gate/studio-tools'],
        requiresOutput: true,
      },
      {
        id: 'runtime-gpu',
        name: 'Forma Rust tests with GPU features',
        argv: 'cargo',
        args: [
          'test',
          '--locked',
          '--manifest-path',
          'crates/forma/Cargo.toml',
          '--features',
          'gpu',
          '--lib',
          '--tests',
        ],
      },
      {
        id: 'svet-wasm',
        name: 'Svet WASM reactive DAG and UMA/DUMA arena Rust tests',
        argv: 'cargo',
        args: [
          'test',
          '--locked',
          '--manifest-path',
          'packages/svet/crates/svet-wasm/Cargo.toml',
          '--lib',
          '--tests',
        ],
      },
      {
        id: 'binding-native',
        name: 'Native binding example',
        argv: 'cargo',
        args: [
          'test',
          '--locked',
          '--manifest-path',
          'crates/forma/examples/binding-app/Cargo.toml',
          '--features',
          'native',
        ],
      },
      // CI names only the binding example, so the second generated-form app compiled for nobody:
      // its build.rs reruns the JS generator whenever src/ changes, and then nothing built or ran
      // the Rust that came out. This is the only place the generated form is proven to behave.
      {
        id: 'language-example',
        name: 'Generated Rust form builds and behaves end to end',
        argv: 'cargo',
        args: [
          'run',
          '--locked',
          '--quiet',
          '--manifest-path',
          'crates/forma/examples/language-app/Cargo.toml',
        ],
        requiresOutput: true,
      },
      // CI checks only the bin, so nothing compiled the thirteen bench examples the runtime work
      // keeps editing. The host is where they cannot rot, and a foreign triple stays at CI's bin.
      {
        id: 'native-host',
        name: 'Native bin and every bench target compile on the host',
        argv: 'cargo',
        args: [
          'check',
          '--locked',
          '--manifest-path',
          'crates/forma/Cargo.toml',
          '--features',
          'native',
          '--all-targets',
        ],
      },
      ...CI_TARGETS.filter(target => installedTargets.includes(target)).map(target => ({
        id: `native-${target}`,
        name: `Native bin compiles for ${target}`,
        argv: 'cargo',
        args: [
          'check',
          '--locked',
          '--manifest-path',
          'crates/forma/Cargo.toml',
          '--features',
          'native',
          '--bin',
          'forma',
          '--target',
          target,
        ],
      })),
      // The GPU and compute crates were never built by any gate or CI job. On macOS the hardware
      // flags are set, because without them every device test returns early and proves nothing.
      {
        id: 'crates-test',
        name: 'GPU and compute crates build and pass their tests',
        argv: 'cargo',
        args: [
          'test',
          '--locked',
          ...WORKSPACE_CRATES.flatMap(name => ['-p', name]),
          '--features',
          'tensor-core/conformance',
        ],
        flags: platform === 'darwin' ? { COMPUTE_REQUIRE_GPU: '1' } : undefined,
      },
      {
        id: 'crates-math-compute',
        name: 'math-compute adapters pass against their backends',
        argv: 'cargo',
        args: [
          'test',
          '--locked',
          '-p',
          'osv-math-compute',
          '--features',
          platform === 'darwin' ? 'gpu,tensor,tensor-wgsl,tensor-mlx' : 'gpu,tensor,tensor-wgsl',
        ],
        flags:
          platform === 'darwin'
            ? { COMPUTE_REQUIRE_GPU: '1', COMPUTE_REQUIRE_MLX: '1' }
            : undefined,
      },
      // MLX exists on Apple Silicon only; elsewhere the crate has nothing to run against.
      ...(platform === 'darwin'
        ? [
            {
              id: 'crates-mlx',
              name: 'compute-mlx passes on the MLX runtime',
              argv: 'cargo',
              args: ['test', '--locked', '-p', 'compute-mlx'],
              flags: { COMPUTE_REQUIRE_MLX: '1' },
            },
          ]
        : []),
      // gpu-kit stays outside the workspace for the same reason and has its own lockfile.
      {
        id: 'crates-gpu-kit',
        name: 'gpu-kit builds and passes on its own lockfile',
        argv: 'cargo',
        // Its GPU render tests are #[ignore]d for machines without a GPU; a Mac has Metal.
        args: [
          'test',
          '--locked',
          '--manifest-path',
          'crates/gpu-kit/Cargo.toml',
          ...(platform === 'darwin' ? ['--', '--include-ignored'] : []),
        ],
      },
      // gpu-compute stays outside the workspace (see the root Cargo.toml) and has its own lockfile.
      {
        id: 'crates-gpu-compute',
        name: 'gpu-compute builds and passes on its own lockfile',
        argv: 'cargo',
        args: ['test', '--locked', '--manifest-path', 'crates/gpu-compute/Cargo.toml'],
        // A developer Mac has real GPU timers, so the profiling duration ratios are checked here;
        // CI runners have virtualized GPUs and skip them.
        flags:
          platform === 'darwin'
            ? { COMPUTE_REQUIRE_GPU: '1', COMPUTE_REQUIRE_TIMESTAMPS: '1' }
            : undefined,
      },
      // Needs `cargo install cargo-deny`; a missing tool fails the step instead of skipping it.
      {
        id: 'deny',
        name: 'Dependency sources, licences and advisories pass the policy (cargo-deny)',
        argv: 'cargo',
        args: ['deny', '--locked', 'check'],
      },
      // Counts come from a macOS build with the native feature, so the comparison is only
      // meaningful on the platform that recorded the baseline.
      ...(platform === 'darwin'
        ? [
            {
              id: 'clippy',
              name: 'Clippy lint debt has not grown (ratchet)',
              argv: process.execPath,
              args: ['scripts/clippy-ratchet.mjs'],
              requiresOutput: true,
            },
          ]
        : []),
      // The library half of forma.wasm, checked without the release build that build:wasm does.
      {
        id: 'wasm-lib',
        name: 'Library compiles for wasm32',
        argv: 'cargo',
        args: ['check', '--locked', '-p', 'forma', '--lib', '--target', 'wasm32-unknown-unknown'],
      },
      ...MOBILE_TARGETS.filter(({ target }) => installedTargets.includes(target)).map(
        ({ target, id, name, args }) => ({
          id,
          name,
          argv: 'cargo',
          args: ['check', '--locked', '-p', 'forma', ...args, '--target', target],
        }),
      ),
      // The WebView host generates its actions module from a fixture, exactly as ci.yml runs it.
      ...(platform === 'darwin'
        ? [
            {
              id: 'studio-legacy',
              name: 'Studio legacy WebView host',
              argv: 'cargo',
              args: ['check', '--locked', '--manifest-path', 'native-app/Cargo.toml'],
              env: { FORMA_ACTIONS: '.github/fixtures/actions.rs' },
            },
          ]
        : []),
    ],
    skipped: [
      ...MOBILE_TARGETS.filter(({ target }) => !installedTargets.includes(target)).map(
        ({ target, id }) => ({ id, reason: `not installed; rustup target add ${target}` }),
      ),
      ...CI_TARGETS.filter(target => !installedTargets.includes(target)).map(target => ({
        id: `native-${target}`,
        reason: `not installed; rustup target add ${target}`,
      })),
      ...(viteEntry ? [] : [{ id: 'studio-build', reason: 'vite is not installed; run npm ci' }]),
    ],
  };
}
export function summary(text) {
  const number = key =>
    Number(new RegExp(`^\\s*(?:#|ℹ)\\s*${key}\\s+(\\d+)$`, 'm').exec(text)?.[1] ?? NaN);
  const tests = number('tests'),
    pass = number('pass'),
    fail = number('fail');
  if (!Number.isInteger(tests) || !Number.isInteger(pass) || !Number.isInteger(fail)) return null;
  return { tests, pass, fail, skipped: number('skipped') };
}
export function evaluate(step, { status, output, dist }) {
  if (status !== 0) return { ok: false, detail: `exit ${status}` };
  // A silent check is not a pass: the versions CLI once exited 0 without printing anything, which
  // would have waved a release tag through, and an empty run here is the same failure shape.
  if (step.requiresOutput && !output.trim()) return { ok: false, detail: 'printed nothing' };
  if (step.requiresSummary) {
    const counts = summary(output);
    return counts
      ? { ok: counts.tests > 0 && counts.fail === 0, detail: `${counts.pass}/${counts.tests} pass` }
      : { ok: false, detail: 'no test summary in output' };
  }
  // The build copies public/vector-pkg, so a dist that still holds the previous runtime is a
  // studio bundle the tests never ran — the same drift npm run package:release refuses to ship.
  if (step.checksDist)
    return dist.length
      ? { ok: false, detail: `stale in dist: ${dist.join(', ')}` }
      : { ok: true, detail: `${BUNDLED.length} runtime files match public/` };
  return { ok: true, detail: '' };
}
export function distDrift() {
  const missing = [],
    stale = [];
  for (const file of BUNDLED) {
    const built = resolve(root, 'dist/vector-pkg', file),
      tested = resolve(root, 'public/vector-pkg', file);
    if (!existsSync(built) || !existsSync(tested)) missing.push(file);
    else if (!readFileSync(built).equals(readFileSync(tested))) stale.push(file);
  }
  return [...missing.map(file => `${file} missing`), ...stale];
}
export function provenance() {
  let info = null;
  try {
    info = JSON.parse(readFileSync(resolve(root, 'public/vector-pkg/build-info.json'), 'utf8'));
  } catch {
    return { state: 'missing', detail: 'public/vector-pkg is not built; run npm run build:wasm' };
  }
  if (info.sourceDigest !== runtimeSourceDigest(root))
    return {
      state: 'stale',
      detail: 'runtime sources changed since the WASM build; rebuild and retest',
    };
  if (
    JSON.stringify(info.artifacts) !==
    JSON.stringify(artifactDigests(resolve(root, 'public/vector-pkg')))
  )
    return { state: 'stale', detail: 'WASM artifacts changed since the build' };
  return {
    state: 'fresh',
    detail: `source ${String(info.sourceDigest).slice(0, 16)} · commit ${String(info.commit).slice(0, 8)}${info.dirty ? ' dirty' : ''}`,
  };
}
export function environment() {
  const git = args => spawnSync('git', args, { cwd: root, encoding: 'utf8' }).stdout?.trim() ?? '';
  let installedTargets = [];
  try {
    installedTargets = execSync('rustup target list --installed', { encoding: 'utf8' })
      .split('\n')
      .map(line => line.trim())
      .filter(Boolean);
  } catch {}
  const vite = 'node_modules/vite/bin/vite.js';
  return {
    node: process.version,
    rustc:
      spawnSync('rustc', ['--version'], { encoding: 'utf8' }).stdout?.trim() ?? 'rustc unavailable',
    wasmBindgen: versions().wasmBindgen,
    hostname: hostname(),
    system: `${process.platform} ${release()} ${process.arch}`,
    cores: cpus().length,
    load: loadavg().map(value => Number(value.toFixed(2))),
    installedTargets,
    viteEntry: existsSync(resolve(root, vite)) ? vite : null,
    branch: git(['rev-parse', '--abbrev-ref', 'HEAD']),
    head: git(['rev-parse', '--short', 'HEAD']),
    dirty: !!git(['status', '--porcelain']).length,
  };
}
const invoked =
  process.argv[1] && import.meta.url === new URL(`file://${resolve(process.argv[1])}`).href;
if (invoked) {
  const args = process.argv.slice(2);
  const option = name => args.find(arg => arg.startsWith(`--${name}=`))?.slice(name.length + 3);
  const env = environment(),
    { list, skipped } = steps({
      platform: option('platform') ?? process.platform,
      installedTargets: env.installedTargets,
      viteEntry: env.viteEntry,
    });
  if (args.includes('--list')) {
    for (const step of list) console.log(`${step.id}\t${step.name}`);
    process.exit(0);
  }
  const only = args.filter(arg => !arg.startsWith('-'));
  const chosen = only.length ? list.filter(step => only.includes(step.id)) : list;
  if (!chosen.length) {
    console.error(`no such step: ${only.join(' ')}\ntry: ${list.map(step => step.id).join(', ')}`);
    process.exit(2);
  }
  console.log(
    `forma full gate · ${env.branch}@${env.head}${env.dirty ? ' (dirty)' : ''} · ${env.system}, ${env.cores} cores, load ${env.load.join(' ')}`,
  );
  console.log(`  node ${env.node} · ${env.rustc} · wasm-bindgen ${env.wasmBindgen}`);
  // The JS steps must test the tokenizer checkout the `tokenizer` step verifies.
  const restoreTokenizer = linkTokenizerPackage();
  if (process.env.FORMA_TOKENIZER_DIR)
    console.log(`  tokenizer ${tokenizerDir} (npm link ${PACKAGE_LINK} for this run)`);
  process.on('exit', restoreTokenizer);
  for (const signal of ['SIGINT', 'SIGTERM'])
    process.on(signal, () => {
      restoreTokenizer();
      process.exit(130);
    });
  const results = [];
  for (const step of chosen) {
    const started = Date.now();
    const run = spawnSync(step.argv, step.args, {
      cwd: root,
      encoding: 'utf8',
      maxBuffer: 64 << 20,
      // `env` values are paths resolved against the repository root; `flags` are passed as given.
      env: (step.env || step.flags) && {
        ...process.env,
        ...Object.fromEntries(
          Object.entries(step.env ?? {}).map(([key, value]) => [key, resolve(root, value)]),
        ),
        ...step.flags,
      },
    });
    const output = `${run.stdout ?? ''}${run.stderr ?? ''}`;
    const verdict = evaluate(step, {
      status: run.status ?? 1,
      output,
      dist: step.checksDist ? distDrift() : [],
    });
    results.push(verdict);
    console.log(
      `  ${verdict.ok ? 'ok  ' : 'FAIL'} ${step.id.padEnd(32)}${((Date.now() - started) / 1000).toFixed(1)}s ${verdict.detail}`,
    );
    if (!verdict.ok && process.env.GATE_VERBOSE)
      console.log(output.trimEnd().replace(/^/gm, '       '));
  }
  restoreTokenizer();
  for (const target of skipped) console.log(`  skip ${target.id.padEnd(30)}${target.reason}`);
  const source = provenance();
  console.log(
    `  ${source.state === 'fresh' ? 'ok  ' : 'FAIL'} wasm-provenance          ${source.detail}`,
  );
  // A pass here must never rest on evidence that is not there: without public/vector-pkg the JS
  // suite skips its WASM integration tests silently, which is the hole ci.yml plugs with an
  // explicit asset check. So the provenance decides the exit code even for a filtered run.
  console.log('  remaining limits and contract-decision levers: PRODUCTION_READINESS.md');
  process.exit(results.every(result => result.ok) && source.state === 'fresh' ? 0 : 1);
}
