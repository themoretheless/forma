/** @vitest-environment jsdom */
import { beforeEach, expect, it } from 'vitest';
import {
  createRoot,
  h,
  onCleanup,
  resetReactiveSystem,
  getWasmCore,
  createSsrStreamTransportDecoder,
} from '../src/engine';
import { renderToReadableStreamAsync } from '../src/engine/server';
beforeEach(() => resetReactiveSystem());
const gate = () => {
  let release!: () => void;
  const promise = new Promise<void>(resolve => (release = resolve));
  return { promise, release };
};
const parse = (bytes: Uint8Array) => JSON.parse(new TextDecoder().decode(bytes));
it('starts only on demand and emits one complete NDJSON record per pull', async () => {
  let calls = 0,
    cleaned = 0;
  const stream = renderToReadableStreamAsync(() => {
    calls++;
    onCleanup(() => cleaned++);
    return { node: h('p', null, '🦀') };
  });
  await Promise.resolve();
  expect(calls).toBe(0);
  const reader = stream.getReader();
  expect(calls).toBe(0);
  const shell = await reader.read();
  expect(shell.done).toBe(false);
  expect(parse(shell.value!).payload.html).toBe('<p>🦀</p>');
  expect(cleaned).toBe(0);
  expect(parse((await reader.read()).value!).payload.type).toBe('complete');
  expect((await reader.read()).done).toBe(true);
  expect(cleaned).toBe(1);
});
it('cancel before the first pull never invokes the view', async () => {
  let calls = 0;
  const stream = renderToReadableStreamAsync(() => {
    calls++;
    throw Error('must not run');
  });
  await stream.cancel();
  expect(calls).toBe(0);
});
it('cancel interrupts a pending component read and releases its request', async () => {
  const wait = gate();
  let cleaned = 0,
    core!: ReturnType<typeof getWasmCore>;
  const stream = renderToReadableStreamAsync(() => {
    core = getWasmCore();
    return {
      node: h(
        'main',
        null,
        h(async () => {
          onCleanup(() => cleaned++);
          await wait.promise;
          return h('p', null, 'late');
        }, null),
      ),
    };
  });
  const reader = stream.getReader();
  await reader.read();
  const pending = reader.read();
  await reader.cancel('disconnect');
  expect((await pending).done).toBe(true);
  expect(cleaned).toBe(1);
  expect(core.svet_get_node_count()).toBe(0);
  wait.release();
  await Promise.resolve();
});
it('external abort rejects closed immediately without another read, preserving reason identity', async () => {
  const abort = new AbortController(),
    reason = { disconnect: true };
  let calls = 0;
  const reader = renderToReadableStreamAsync(
    () => {
      calls++;
      return { node: h('p', null) };
    },
    { signal: abort.signal },
  ).getReader();
  const closed = expect(reader.closed).rejects.toBe(reason);
  abort.abort(reason);
  await closed;
  await expect(reader.read()).rejects.toBe(reason);
  expect(calls).toBe(0);
});
it('calling owner disposal errors a pending read and retires the graph', async () => {
  const wait = gate();
  let stop!: () => void,
    cleaned = 0;
  const reader = createRoot(dispose => {
    stop = dispose;
    return renderToReadableStreamAsync(() => ({
      node: h(async () => {
        onCleanup(() => cleaned++);
        await wait.promise;
        return h('p', null);
      }, null),
    })).getReader();
  });
  await reader.read();
  const pending = expect(reader.read()).rejects.toThrow(/owner disposed/);
  stop();
  await pending;
  expect(cleaned).toBe(1);
  wait.release();
});
it('encodes reference frames that decode to the original authoritative final HTML', async () => {
  const wait = gate(),
    html = 'x'.repeat(2048),
    decoder = createSsrStreamTransportDecoder();
  const reader = renderToReadableStreamAsync(
    () => ({
      node: h(
        'main',
        null,
        h(async () => {
          await wait.promise;
          return h('p', null, html);
        }, null),
      ),
    }),
    { codec: 'svet-html-ref-v1' },
  ).getReader();
  expect(decoder.decode(parse((await reader.read()).value!)).payload.type).toBe('shell');
  wait.release();
  decoder.decode(parse((await reader.read()).value!));
  const frame = parse((await reader.read()).value!);
  expect(frame.codec).toBe('svet-html-ref-v1');
  expect(decoder.decode(frame).payload.html).toBe('<main><p>' + html + '</p></main>');
  expect((await reader.read()).done).toBe(true);
});
it('propagates producer errors and validates options before invoking the view', async () => {
  const reason = Error('render failed'),
    reader = renderToReadableStreamAsync(() => {
      throw reason;
    }).getReader();
  await expect(reader.read()).rejects.toBe(reason);
  expect(() =>
    renderToReadableStreamAsync(
      () => {
        throw Error('must not run');
      },
      { codec: 'future' as any },
    ),
  ).toThrow(/codec/);
});
it('reports a background producer failure through reader.closed without a second read', async () => {
  const wait = gate(),
    reason = Error('background component failed');
  let cleaned = 0;
  const reader = renderToReadableStreamAsync(() => ({
    node: h(
      'main',
      null,
      h(async () => {
        onCleanup(() => cleaned++);
        await wait.promise;
        throw reason;
      }, null),
    ),
  })).getReader();
  await reader.read();
  const observed = reader.closed.catch(error => error);
  wait.release();
  let timer: ReturnType<typeof setTimeout> | undefined;
  try {
    expect(
      await Promise.race([
        observed,
        new Promise(resolve => {
          timer = setTimeout(() => resolve('still pending'), 100);
        }),
      ]),
    ).toBe(reason);
  } finally {
    clearTimeout(timer);
    await reader.cancel().catch(() => {});
  }
  expect(cleaned).toBe(1);
});
it('closes after the complete frame without requiring an extra consumer read', async () => {
  const reader = renderToReadableStreamAsync(() => ({ node: h('p', null, 'done') })).getReader();
  await reader.read();
  expect(parse((await reader.read()).value!).payload.type).toBe('complete');
  let timer: ReturnType<typeof setTimeout> | undefined;
  try {
    expect(
      await Promise.race([
        reader.closed.then(() => true),
        new Promise(resolve => {
          timer = setTimeout(() => resolve(false), 100);
        }),
      ]),
    ).toBe(true);
  } finally {
    clearTimeout(timer);
    await reader.cancel();
  }
});
