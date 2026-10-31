import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { expect, it } from 'vitest';

it('runs public package exports in real Node without a bundler or a DOM global', () => {
  const program = `
    import assert from 'node:assert/strict';
    import { signal, effect, getGraphTelemetry } from 'svet-engine';
    import { runWithIsolatedContextAsync } from 'svet-engine/server';
    const results = await Promise.all([1, 10, 100].map(start =>
      runWithIsolatedContextAsync(async () => {
        const value = signal(start);
        await new Promise(resolve => setTimeout(resolve, 1));
        const observed = [];
        effect(() => observed.push(value()));
        value.set(start + 1);
        return observed;
      })
    ));
    assert.deepEqual(results, [[1, 2], [10, 11], [100, 101]]);
    assert.equal(getGraphTelemetry().nodes, 0);
    assert.equal(typeof document, 'undefined');
    console.log('server entry verified');
  `;
  const result = spawnSync(process.execPath, ['--input-type=module', '-e', program], {
    cwd: fileURLToPath(new URL('..', import.meta.url)), encoding: 'utf8', timeout: 10000,
  });
  expect(result.error).toBeUndefined();
  expect(result.status, result.stderr).toBe(0);
  expect(result.stdout.trim()).toBe('server entry verified');
});
