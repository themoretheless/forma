/**
 * Per-operation cost as the reactive graph grows. A flat row means the
 * operation is O(1) per dependency; a row that climbs with the size is a
 * linear scan somewhere on the path. Exits non-zero when a row grows more
 * than `--limit` times (default 6) between the smallest and largest size.
 * Diagnostic timings on whatever host runs it, not a performance claim.
 *
 *   npm run build:package && node scripts/bench-graph-scaling.mjs [--limit 6] [--json out.json]
 */
import { writeFile } from 'node:fs/promises';
import { resolve, dirname } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const e = await import(pathToFileURL(resolve(root, 'dist/package/reactivity.js')));
const option = (name, fallback) => {
  const at = process.argv.indexOf(name);
  return at < 0 ? fallback : process.argv[at + 1];
};
const limit = Number(option('--limit', 6));
const sizes = [1000, 4000, 16000, 64000];
const median = values => [...values].sort((a, b) => a - b)[values.length >> 1];

/** `setup(n)` builds the graph and returns the timed part; one warmup, five samples. */
function nanosPerOp(setup, n, opsPerRun = n) {
  const samples = [];
  for (let sample = -1; sample < 5; sample++) {
    e.resetReactiveSystem();
    const run = setup(n);
    const start = performance.now();
    run();
    const elapsed = performance.now() - start;
    if (sample >= 0) samples.push(elapsed);
  }
  return (median(samples) * 1e6) / opsPerRun;
}
const signals = n => Array.from({ length: n }, (_, i) => e.signal(i));
const readAll = sources => () => {
  for (const source of sources) source();
};

const cases = [
  [
    '1 effect reading N signals: first run, per read',
    n =>
      (
        sources => () =>
          e.effect(readAll(sources))
      )(signals(n)),
  ],
  [
    '1 effect reading N signals: rerun, per read',
    n => {
      const sources = signals(n);
      e.effect(readAll(sources));
      return () => {
        for (let round = 1; round <= 5; round++) sources[0].set(-round);
      };
    },
    5,
  ],
  [
    '1 effect reading N signals twice each: rerun, per read',
    n => {
      const sources = signals(n);
      e.effect(() => {
        for (const source of sources) source() + source();
      });
      return () => {
        for (let round = 1; round <= 5; round++) sources[0].set(-round);
      };
    },
    10,
  ],
  [
    'batch of N writes read by 1 effect, per write',
    n => {
      const sources = signals(n);
      e.effect(readAll(sources));
      return () => e.batch(() => sources.forEach((source, i) => source.set(-i - 1)));
    },
  ],
  [
    'N effects on one signal: create, per effect',
    n => {
      const shared = e.signal(1);
      return () => {
        for (let i = 0; i < n; i++) e.effect(() => void shared());
      };
    },
  ],
  [
    'N effects on one signal: dispose, per effect',
    n => {
      const shared = e.signal(1);
      const handles = Array.from({ length: n }, () => e.effect(() => void shared()));
      return () => handles.forEach(handle => handle.dispose());
    },
  ],
  [
    'N computeds on one signal: create + read, per computed',
    n => {
      const shared = e.signal(1);
      return () => {
        for (let i = 0; i < n; i++) e.computed(() => shared() + i)();
      };
    },
  ],
  [
    'N computeds on one signal: dispose, per computed',
    n => {
      const shared = e.signal(1);
      const values = Array.from({ length: n }, (_, i) => {
        const value = e.computed(() => shared() + i);
        value();
        return value;
      });
      return () => values.forEach(value => value.dispose());
    },
  ],
  [
    '1 write fanning out to N effects, per effect',
    n => {
      const shared = e.signal(0);
      for (let i = 0; i < n; i++) e.effect(() => void shared());
      return () => shared.set(1);
    },
  ],
];

const rows = [];
console.log(`${''.padEnd(58)}${sizes.map(n => String(n).padStart(8)).join('')}   growth`);
for (const [name, setup, repeats = 1] of cases) {
  const nanos = sizes.map(n => nanosPerOp(setup, n, n * repeats));
  // Small graphs carry fixed costs and JIT warmup; compare the two ends' best.
  const growth = nanos.at(-1) / Math.min(...nanos.slice(0, 2));
  rows.push({ name, sizes, nanosPerOp: nanos, growth });
  console.log(
    `${name.padEnd(58)}${nanos.map(value => value.toFixed(0).padStart(8)).join('')}   ${growth.toFixed(1)}x${growth > limit ? '  <-- grows with N' : ''}`,
  );
}
const output = option('--json');
if (output)
  await writeFile(
    output,
    JSON.stringify(
      { status: 'diagnostic', node: process.version, unit: 'ns/op', limit, rows },
      null,
      2,
    ) + '\n',
  );
if (rows.some(row => row.growth > limit)) {
  console.error(
    `\nAt least one operation grew more than ${limit}x from the small sizes to ${sizes.at(-1)}.`,
  );
  process.exit(1);
}
