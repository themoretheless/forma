import { spawnSync } from 'node:child_process';
import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import { resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

// noUncheckedIndexedAccess cannot be switched on per folder: tsc reports errors in every file a
// checked file imports. So the flag is ratcheted instead. The baseline records how many findings
// each file has; a run fails when a file gains one, and asks for the baseline to be lowered when a
// file loses one, so fixes are locked in. A file at zero can never regress.
const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const BASELINE = resolve(root, 'indexed-access-baseline.json');

export function count(output) {
  const counts = {};
  for (const match of output.matchAll(/^(\S+?)\(\d+,\d+\): error TS\d+/gm))
    counts[match[1]] = (counts[match[1]] ?? 0) + 1;
  return Object.fromEntries(Object.entries(counts).sort(([a], [b]) => (a < b ? -1 : 1)));
}

export function compare(baseline, current) {
  const worse = [],
    better = [];
  for (const file of new Set([...Object.keys(baseline), ...Object.keys(current)])) {
    const before = baseline[file] ?? 0,
      now = current[file] ?? 0;
    if (now > before) worse.push(`${file}: ${before} -> ${now}`);
    if (now < before) better.push(`${file}: ${before} -> ${now}`);
  }
  return { worse, better };
}

if (import.meta.url === `file://${process.argv[1]}`) {
  const tsc = resolve(root, 'node_modules/typescript/bin/tsc');
  const run = spawnSync(
    process.execPath,
    [tsc, '--noEmit', '--noUncheckedIndexedAccess', '--pretty', 'false'],
    { cwd: root, encoding: 'utf8', maxBuffer: 64 << 20 },
  );
  const current = count(run.stdout);
  const total = Object.values(current).reduce((a, b) => a + b, 0);
  if (process.argv.includes('--update') || !existsSync(BASELINE)) {
    writeFileSync(BASELINE, JSON.stringify(current, null, 2) + '\n');
    console.log(`indexed access: baseline written, ${total} findings`);
  } else {
    const { worse, better } = compare(JSON.parse(readFileSync(BASELINE, 'utf8')), current);
    if (worse.length) {
      console.error(`indexed access: new unchecked index reads\n  ${worse.join('\n  ')}`);
      process.exit(1);
    }
    if (better.length) {
      console.error(
        `indexed access: findings dropped; lock it in with npm run typecheck:indexed -- --update\n  ${better.join('\n  ')}`,
      );
      process.exit(1);
    }
    console.log(`indexed access: ${total} findings, unchanged from the baseline`);
  }
}
