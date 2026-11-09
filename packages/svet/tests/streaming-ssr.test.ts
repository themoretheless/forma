/** @vitest-environment jsdom */
import { beforeEach, expect, it } from 'vitest';
import {
  createRoot,
  createSsrStreamReceiver,
  SSR_STREAM_VERSION,
  getGraphTelemetry,
  getWasmCore,
  h,
  onCleanup,
  resetReactiveSystem,
  signal,
  type SsrStreamChunk,
} from '../src/engine';
import { renderToChunksAsync } from '../src/engine/server';
beforeEach(() => resetReactiveSystem());
const gate = () => {
  let release!: () => void;
  const promise = new Promise<void>(r => (release = r));
  return { promise, release };
};
it('delivers a shell before slow components, then patches and resumes state without replacing matching nodes', async () => {
  const slow = gate(),
    signals: Record<string, any> = {};
  let cleaned = 0,
    core!: ReturnType<typeof getWasmCore>;
  const stream = renderToChunksAsync(() => {
    core = getWasmCore();
    return {
      node: h(
        'main',
        null,
        h('b', null, 'shell'),
        h(async () => {
          onCleanup(() => cleaned++);
          await slow.promise;
          signals.count = signal(7);
          onCleanup(signals.count.dispose);
          return h('input', { type: 'number', 'data-svet-model': 'count', model: signals.count });
        }, null),
      ),
      signals,
    };
  });
  const first = await stream.next();
  expect(first.value?.payload.type).toBe('shell');
  expect(first.value?.payload.html).toContain('shell');
  expect(first.value?.payload.html).not.toContain('<input');
  const host = document.createElement('div'),
    receiver = createSsrStreamReceiver(host);
  receiver.apply(first.value!);
  expect(host.querySelector('b')?.textContent).toBe('shell');
  slow.release();
  const patch = await stream.next();
  expect(patch.value?.payload.type).toBe('patch');
  receiver.apply(patch.value!);
  const input = host.querySelector('input')!;
  expect(input.value).toBe('7');
  const complete = await stream.next();
  expect(complete.value?.payload.type).toBe('complete');
  expect([first.value!.version, patch.value!.version, complete.value!.version]).toEqual([
    SSR_STREAM_VERSION,
    SSR_STREAM_VERSION,
    SSR_STREAM_VERSION,
  ]);
  expect([first.value!.sequence, patch.value!.sequence, complete.value!.sequence]).toEqual([
    0, 1, 2,
  ]);
  const state = receiver.apply(complete.value!)!;
  expect(host.querySelector('input')).toBe(input);
  input.value = '9';
  input.dispatchEvent(new Event('input'));
  expect(state.count()).toBe(9);
  expect((await stream.next()).done).toBe(true);
  expect(cleaned).toBe(1);
  expect(core.svet_get_node_count()).toBe(0);
  receiver.dispose();
  expect(getGraphTelemetry().nodes).toBe(0);
});
it('orders nested patches and reconciles cross-boundary reactive writes in the final snapshot', async () => {
  const outer = gate(),
    inner = gate();
  let value!: ReturnType<typeof signal<string>>;
  const stream = renderToChunksAsync(() => {
    value = signal('old');
    onCleanup(value.dispose);
    return {
      node: h(
        'main',
        null,
        h(async () => {
          await outer.promise;
          return h(
            'section',
            null,
            h('b', null, value),
            h(async () => {
              await inner.promise;
              value.set('new');
              return h('i', null, 'nested');
            }, null),
          );
        }, null),
      ),
    };
  });
  const host = document.createElement('div'),
    receiver = createSsrStreamReceiver(host);
  receiver.apply((await stream.next()).value!);
  outer.release();
  receiver.apply((await stream.next()).value!);
  expect(host.querySelector('b')?.textContent).toBe('old');
  inner.release();
  for await (const chunk of stream) receiver.apply(chunk);
  expect(host.textContent).toBe('newnested');
  expect(host.innerHTML).not.toContain('svet:');
  receiver.dispose();
});
it('return cancels an outstanding next and cleans up pending component ownership', async () => {
  const wait = gate();
  let cleaned = 0,
    core!: ReturnType<typeof getWasmCore>;
  const stream = renderToChunksAsync(() => {
    core = getWasmCore();
    return {
      node: h(async () => {
        onCleanup(() => cleaned++);
        await wait.promise;
        return h('p', null, 'late');
      }, null),
    };
  });
  await stream.next();
  const pending = stream.next();
  const rejected = expect(pending).rejects.toThrow(/consumer closed/);
  await stream.return!();
  await rejected;
  expect(cleaned).toBe(1);
  expect(core.svet_get_node_count()).toBe(0);
  wait.release();
  for (let i = 0; i < 20; i++) await Promise.resolve();
});
it('propagates component failure after a shell without waiting for an uncooperative sibling', async () => {
  const fail = gate(),
    failure = Error('stream component failed');
  let cleaned = 0;
  const stream = renderToChunksAsync(() => ({
    node: h(
      'main',
      null,
      h(async () => {
        onCleanup(() => cleaned++);
        await new Promise(() => {});
        return document.createElement('i');
      }, null),
      h(async () => {
        await fail.promise;
        throw failure;
      }, null),
    ),
  }));
  expect((await stream.next()).value?.payload.type).toBe('shell');
  fail.release();
  await expect(stream.next()).rejects.toBe(failure);
  expect(cleaned).toBe(1);
});
it('refuses stale/malformed chunks and lets a replacement receiver own the container', () => {
  const host = document.createElement('div'),
    a = createSsrStreamReceiver(host);
  a.apply({ version: 1, sequence: 0, streamId: 'a', payload: { type: 'shell', html: 'A' } });
  const b = createSsrStreamReceiver(host);
  b.apply({ version: 1, sequence: 0, streamId: 'b', payload: { type: 'shell', html: 'B' } });
  a.dispose();
  expect(host.textContent).toBe('B');
  expect(() =>
    a.apply({
      version: 1,
      sequence: 1,
      streamId: 'a',
      payload: { type: 'patch', boundary: 1, html: 'late' },
    }),
  ).toThrow();
  expect(() =>
    b.apply({
      version: 1,
      sequence: 1,
      streamId: 'wrong',
      payload: { type: 'patch', boundary: 1, html: 'wrong' },
    }),
  ).toThrow(/another stream/);
  expect(b.active).toBe(false);
});
it('enforces stream limits and owner disposal while a consumer is paused', async () => {
  await expect(
    renderToChunksAsync(() => ({ node: h('p', null, 'too large') }), { maxChunkBytes: 1 }).next(),
  ).rejects.toThrow(/maxChunkBytes/);
  const wait = gate();
  const stream = renderToChunksAsync(
    () => ({
      node: h(
        'main',
        null,
        h(async () => {
          await wait.promise;
          return document.createElement('b');
        }, null),
        h(async () => {
          await wait.promise;
          return document.createElement('i');
        }, null),
      ),
    }),
    { maxBoundaries: 1 },
  );
  await expect(stream.next()).rejects.toThrow(/maxBoundaries/);
  wait.release();
  let stop!: () => void;
  const owned = createRoot(dispose => {
    stop = dispose;
    return renderToChunksAsync(() => ({ node: h('p', null, 'ready') }));
  });
  await owned.next();
  stop();
  await expect(owned.next()).rejects.toThrow(/owner disposed/);
});
it('preserves SVG namespace for contextual patches and receiver parent teardown', () => {
  const host = document.createElement('div');
  let stop!: () => void;
  const receiver = createRoot(dispose => {
    stop = dispose;
    return createSsrStreamReceiver(host);
  });
  receiver.apply({
    version: 1,
    sequence: 0,
    streamId: 'svg',
    payload: { type: 'shell', html: '<svg><!--svet:svg:1--><!--svet:svg:1:end--></svg>' },
  });
  receiver.apply({
    version: 1,
    sequence: 1,
    streamId: 'svg',
    payload: { type: 'patch', boundary: 1, html: '<circle r="3"></circle>' },
  });
  expect(host.querySelector('circle')?.namespaceURI).toBe('http://www.w3.org/2000/svg');
  stop();
  expect(receiver.active).toBe(false);
});
it('cleans paused backpressure work and removes external cancellation hooks', async () => {
  const wait = gate(),
    abort = new AbortController();
  let cleaned = 0;
  const hooks = new Set<unknown>();
  const add = abort.signal.addEventListener.bind(abort.signal),
    remove = abort.signal.removeEventListener.bind(abort.signal);
  abort.signal.addEventListener = ((type: string, fn: any, options: any) => {
    if (type === 'abort') hooks.add(fn);
    add(type, fn, options);
  }) as any;
  abort.signal.removeEventListener = ((type: string, fn: any, options: any) => {
    hooks.delete(fn);
    remove(type, fn, options);
  }) as any;
  const stream = renderToChunksAsync(
    () => ({
      node: h(
        'main',
        null,
        ...[1, 2].map(i =>
          h(async () => {
            onCleanup(() => cleaned++);
            await wait.promise;
            return h('b', null, i);
          }, null),
        ),
      ),
    }),
    { signal: abort.signal },
  );
  await stream.next();
  wait.release();
  for (let i = 0; i < 30; i++) await Promise.resolve();
  expect(cleaned).toBe(0);
  const reason = Error('disconnect');
  abort.abort(reason);
  await expect(stream.next()).rejects.toBe(reason);
  expect(cleaned).toBe(2);
  expect(hooks.size).toBe(0);
});
it('preserves undefined rejections and closes receiver resources on graph reset', async () => {
  const wait = gate(),
    stream = renderToChunksAsync(() => ({
      node: h(async () => {
        await wait.promise;
        throw undefined;
      }, null),
    }));
  await stream.next();
  wait.release();
  await expect(stream.next()).rejects.toBeUndefined();
  const host = document.createElement('div'),
    receiver = createSsrStreamReceiver(host);
  receiver.apply({
    version: 1,
    sequence: 0,
    streamId: 'reset',
    payload: { type: 'shell', html: 'pending' },
  });
  resetReactiveSystem();
  expect(receiver.active).toBe(false);
  expect(() =>
    receiver.apply({
      version: 1,
      sequence: 1,
      streamId: 'reset',
      payload: { type: 'complete', html: 'done', stateJson: '{}' },
    }),
  ).toThrow();
});
it('keeps the pending read usable when a concurrent next call is rejected', async () => {
  const wait = gate();
  const stream = renderToChunksAsync(async () => {
    await wait.promise;
    return { node: h('p', null, 'ready') };
  });
  const pending = stream.next();
  await expect(stream.next()).rejects.toThrow(/Concurrent/);
  wait.release();
  expect((await pending).value?.payload.type).toBe('shell');
  expect((await stream.next()).value?.payload.type).toBe('complete');
  expect((await stream.next()).done).toBe(true);
  expect((await stream.return!()).done).toBe(true);
  await expect(stream.throw!(Error('caller failure'))).rejects.toThrow('caller failure');
  expect((await stream.next()).done).toBe(true);
});
it('retires externally aborted streams without requiring another read and reports the reason once', async () => {
  for (const started of [false, true]) {
    const abort = new AbortController(),
      reason = { disconnect: started };
    let calls = 0;
    const stream = renderToChunksAsync(
      () => {
        calls++;
        return { node: h('p', null, 'ready') };
      },
      { signal: abort.signal },
    );
    if (started) await stream.next();
    abort.abort(reason);
    await new Promise(resolve => setTimeout(resolve, 0));
    await expect(stream.next()).rejects.toBe(reason);
    expect((await stream.next()).done).toBe(true);
    expect(calls).toBe(started ? 1 : 0);
    expect((await stream.return!()).done).toBe(true);
  }
  const abort = new AbortController(),
    reason = Error('already aborted');
  abort.abort(reason);
  const stream = renderToChunksAsync(
    () => {
      throw Error('view must not run');
    },
    { signal: abort.signal },
  );
  await expect(stream.next()).rejects.toBe(reason);
  expect((await stream.next()).done).toBe(true);
});
