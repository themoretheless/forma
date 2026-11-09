import { spawnSync } from 'node:child_process';
import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { root } from './versions.mjs';

// Lint debt may only shrink. The baseline records how many times each clippy lint fires in each
// package; a run fails when any count grows or a new lint appears, and asks for the baseline to be
// lowered when a count drops, so progress is locked in by the next commit. Lints that clippy denies
// by default are capped to warnings here: an error would stop the build before dependents are
// checked and hide their counts.
const BASELINE = 'docs/refactor/lint-baseline.json';
const ARGS = [
  'clippy',
  '--locked',
  '--workspace',
  '--exclude',
  'forma-binding-example',
  '--exclude',
  'forma-language-example',
  '--all-targets',
  '--features',
  'forma/native',
  '--message-format=json',
  '--',
  '--cap-lints',
  'warn',
];

export function count(lines) {
  const counts = {},
    seen = new Set();
  for (const line of lines) {
    if (!line.startsWith('{')) continue;
    let event;
    try {
      event = JSON.parse(line);
    } catch {
      continue;
    }
    const message = event.message,
      code = message?.code?.code;
    if (event.reason !== 'compiler-message' || !code) continue;
    // One finding is reported once per target that compiles the file (lib, lib test, bin).
    const span = message.spans?.[0] ?? {},
      key = [code, span.file_name, span.line_start, span.column_start].join(':');
    if (seen.has(key)) continue;
    seen.add(key);
    const name = event.package_id.split('#')[0].split('/').pop();
    (counts[name] ??= {})[code] = (counts[name][code] ?? 0) + 1;
  }
  return sorted(counts);
}
const sorted = value =>
  Object.fromEntries(
    Object.keys(value)
      .sort()
      .map(key => [
        key,
        typeof value[key] === 'object' && value[key] ? sorted(value[key]) : value[key],
      ]),
  );

export function compare(baseline, current) {
  const worse = [],
    better = [];
  for (const name of new Set([...Object.keys(baseline), ...Object.keys(current)]))
    for (const lint of new Set([
      ...Object.keys(baseline[name] ?? {}),
      ...Object.keys(current[name] ?? {}),
    ])) {
      const before = baseline[name]?.[lint] ?? 0,
        now = current[name]?.[lint] ?? 0;
      if (now > before) worse.push(`${name}: ${lint} ${before} -> ${now}`);
      if (now < before) better.push(`${name}: ${lint} ${before} -> ${now}`);
    }
  return { worse, better };
}
const total = counts =>
  Object.values(counts).reduce(
    (sum, lints) => sum + Object.values(lints).reduce((a, b) => a + b, 0),
    0,
  );

if (import.meta.url === `file://${process.argv[1]}`) {
  const run = spawnSync('cargo', ARGS, { cwd: root, encoding: 'utf8', maxBuffer: 512 << 20 });
  if (run.status !== 0) {
    process.stderr.write(run.stderr.split('\n').slice(-30).join('\n'));
    console.error('clippy did not finish; nothing was compared');
    process.exit(1);
  }
  const current = count(run.stdout.split('\n')),
    path = resolve(root, BASELINE);
  if (process.argv.includes('--update') || !existsSync(path)) {
    writeFileSync(path, JSON.stringify(current, null, 2) + '\n');
    console.log(`clippy: baseline written, ${total(current)} findings`);
  } else {
    const { worse, better } = compare(JSON.parse(readFileSync(path, 'utf8')), current);
    if (worse.length) {
      console.error(`clippy: lint debt grew\n  ${worse.join('\n  ')}`);
      process.exit(1);
    }
    if (better.length) {
      console.error(
        `clippy: lint debt shrank; lock it in with node scripts/clippy-ratchet.mjs --update\n  ${better.join('\n  ')}`,
      );
      process.exit(1);
    }
    console.log(`clippy: ${total(current)} findings, unchanged from the baseline`);
  }
}
