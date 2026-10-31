/** @vitest-environment jsdom */
import { beforeEach, expect, it } from 'vitest';
import { createResource, effect, getGraphTelemetry, getWasmCore, h, onCleanup, resetReactiveSystem, resumeContainer, runWithIsolatedContext, signal } from '../src/engine';

import { renderToStringAsync, runWithIsolatedContextAsync } from '../src/engine/server';

beforeEach(() => resetReactiveSystem());
function gate() {
  let release!: () => void;
  const promise = new Promise<void>(resolve => { release = resolve; });
  return { promise, release };
}

it('isolates genuinely interleaved requests across await boundaries', async () => {
  const first = gate(), second = gate();
  const defaultCore = getWasmCore();
  const run = (value: number, wait: Promise<void>) => runWithIsolatedContextAsync(async () => {
    const core = getWasmCore();
    const before = signal(value);
    await wait;
    expect(getWasmCore()).toBe(core);
    const after = signal(value + 1);
    const values: number[] = [];
    effect(() => { values.push(before() + after()); });
    before.set(value + 2);
    return { values, nodes: getGraphTelemetry().nodes, core };
  });
  const a = run(10, first.promise);
  const b = run(100, second.promise);
  second.release();
  const resultB = await b;
  first.release();
  const resultA = await a;
  expect(resultA.values).toEqual([21, 23]);
  expect(resultB.values).toEqual([201, 203]);
  expect(resultA.nodes).toBe(3);
  expect(resultB.nodes).toBe(3);
  expect(resultA.core).not.toBe(resultB.core);
  expect(getWasmCore()).toBe(defaultCore);
  expect(getGraphTelemetry().nodes).toBe(0);
});

it('cleans up resources created after await on both success and rejection', async () => {
  for (const fail of [false, true]) {
    let cleanups = 0;
    let core!: ReturnType<typeof getWasmCore>;
    let requestSignal!: AbortSignal;
    const run = runWithIsolatedContextAsync(async () => {
      await Promise.resolve();
      core = getWasmCore();
      onCleanup(() => { cleanups++; });
      createResource(ctx => {
        requestSignal = ctx.signal;
        return new Promise<string>(() => {});
      });
      if (fail) throw new Error('request failure');
      return 'done';
    });
    if (fail) await expect(run).rejects.toThrow('request failure');
    else expect(await run).toBe('done');
    expect(cleanups).toBe(1);
    expect(requestSignal.aborted).toBe(true);
    expect(core.svet_get_node_count()).toBe(0);
    expect(getGraphTelemetry().nodes).toBe(0);
  }
});

it('restores parent request context after nested async and synchronous isolation', async () => {
  await runWithIsolatedContextAsync(async () => {
    const parent = getWasmCore();
    const value = signal(10);
    const child = await runWithIsolatedContextAsync(async () => {
      await Promise.resolve();
      expect(getWasmCore()).not.toBe(parent);
      return signal(20)();
    });
    expect(child).toBe(20);
    expect(getWasmCore()).toBe(parent);
    const syncCore = runWithIsolatedContext(() => getWasmCore());
    expect(syncCore).not.toBe(parent);
    expect(getWasmCore()).toBe(parent);
    value.set(11);
    expect(value()).toBe(11);
  });
});

it('does not let abandoned async work fall back into the default graph', async () => {
  const wait = gate();
  let late!: Promise<unknown>;
  await runWithIsolatedContextAsync(() => {
    late = wait.promise.then(() => getWasmCore());
    return 'done';
  });
  const assertion = expect(late).rejects.toThrow('already completed');
  wait.release();
  await assertion;
  expect(getGraphTelemetry().nodes).toBe(0);
});

it('renders and resumes independently interleaved async views with escaped snapshots', async () => {
  const waitA = gate(), waitB = gate();
  const render = (title: string, wait: Promise<void>) => renderToStringAsync(async () => {
    const value = signal(title);
    await wait;
    const node = h('p', { 'data-svet-text': 'value' }, () => value());
    return { node, signals: { value } };
  });
  const a = render('A </script><img src=x>', waitA.promise);
  const b = render('B & <other>', waitB.promise);
  waitB.release();
  const resultB = await b;
  waitA.release();
  const resultA = await a;
  for (const [result, title] of [[resultA, 'A </script><img src=x>'], [resultB, 'B & <other>']] as const) {
    expect(result.stateJson).not.toContain('</script>');
    const container = document.createElement('div');
    container.innerHTML = result.html;
    expect(container.querySelectorAll('script')).toHaveLength(1);
    expect(container.querySelector('img')).toBeNull();
    expect(container.querySelector('p')!.textContent).toBe(title);
    const restored = resumeContainer(container, result.stateJson);
    expect(restored.value()).toBe(title);
    restored.value.set('resumed');
    expect(container.querySelector('p')!.textContent).toBe('resumed');
  }
});

it('aborting a request tears down the graph and closes the context of late work', async () => {
  const controller = new AbortController();
  const wait = gate();
  let late!: Promise<unknown>;
  let cleaned = 0;
  let core!: ReturnType<typeof getWasmCore>;
  const run = runWithIsolatedContextAsync(() => {
    core = getWasmCore();
    onCleanup(() => { cleaned++; });
    signal(10);
    late = wait.promise.then(() => getWasmCore());
    return late;
  }, { signal: controller.signal });
  // The cancellable callback starts in a microtask.
  await Promise.resolve();
  const rejected = expect(run).rejects.toThrow('disconnected');
  controller.abort(new Error('disconnected'));
  await rejected;
  expect(cleaned).toBe(1);
  expect(core.svet_get_node_count()).toBe(0);
  const lateRejected = expect(late).rejects.toThrow('already completed');
  wait.release();
  await lateRejected;
  expect(getGraphTelemetry().nodes).toBe(0);
});

it('does not invoke a callback if the request was already aborted', async () => {
  const controller = new AbortController();
  controller.abort(new Error('already cancelled'));
  let called = false;
  await expect(runWithIsolatedContextAsync(() => { called = true; }, {
    signal: controller.signal,
  })).rejects.toThrow('already cancelled');
  expect(called).toBe(false);
  expect(getGraphTelemetry().nodes).toBe(0);
});
