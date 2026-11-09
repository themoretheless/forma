/** @vitest-environment jsdom */
import { afterEach, expect, it, vi } from 'vitest';
import { h, onCleanup, getWasmCore } from '../src/engine';
import {
  renderToStringAsync,
  renderToChunksAsync,
  renderToReadableStreamAsync,
  runWithIsolatedContextAsync,
  runWithOwnerScopeAsync,
  SsrTimeoutError,
} from '../src/engine/server';
afterEach(() => vi.useRealTimers());
it('times out an uncooperative async render and releases its graph', async () => {
  let cleaned = 0,
    core!: ReturnType<typeof getWasmCore>;
  await expect(
    renderToStringAsync(
      () => {
        core = getWasmCore();
        return {
          node: h(async () => {
            onCleanup(() => cleaned++);
            await new Promise(() => {});
            return h('p', null);
          }, null),
        };
      },
      { timeoutMs: 10 },
    ),
  ).rejects.toBeInstanceOf(SsrTimeoutError);
  expect(cleaned).toBe(1);
  expect(core.svet_get_node_count()).toBe(0);
});
it('errors unread Web streams and pending chunk reads at the deadline', async () => {
  const view = () => ({
    node: h(async () => {
      await new Promise(() => {});
      return h('p', null);
    }, null),
  });
  const chunks = renderToChunksAsync(view, { timeoutMs: 10 });
  await chunks.next();
  await expect(chunks.next()).rejects.toBeInstanceOf(SsrTimeoutError);
  const reader = renderToReadableStreamAsync(view, { timeoutMs: 10 }).getReader();
  await reader.read();
  await expect(reader.closed).rejects.toBeInstanceOf(SsrTimeoutError);
});
it('keeps caller abort reason and removes success/cancel timers', async () => {
  vi.useFakeTimers();
  const abort = new AbortController(),
    reason = Error('caller abort');
  const pending = renderToStringAsync(() => new Promise(() => {}), {
    timeoutMs: 500,
    signal: abort.signal,
  });
  const rejected = expect(pending).rejects.toBe(reason);
  abort.abort(reason);
  await rejected;
  expect(vi.getTimerCount()).toBe(0);
  await renderToStringAsync(() => ({ node: h('p', null, 'ok') }), { timeoutMs: 500 });
  expect(vi.getTimerCount()).toBe(0);
  const stream = renderToChunksAsync(() => ({ node: h('p', null) }), { timeoutMs: 500 });
  await stream.return!();
  expect(vi.getTimerCount()).toBe(0);
});
it('bounds a child without aborting its parent and rejects invalid deadlines', async () => {
  await runWithIsolatedContextAsync(async () => {
    await expect(
      runWithOwnerScopeAsync([], () => new Promise(() => {}), { timeoutMs: 5 }),
    ).rejects.toBeInstanceOf(SsrTimeoutError);
    expect(h('b', null, 'parent').textContent).toBe('parent');
  });
  for (const timeoutMs of [0, -1, NaN, Infinity, 1.5, 2147483648])
    await expect(
      renderToStringAsync(() => ({ node: h('p', null) }), { timeoutMs }),
    ).rejects.toThrow(/timeoutMs/);
});

it('does not construct timeout diagnostics for an already aborted caller', async () => {
  const abort = new AbortController(),
    reason = Error('already aborted');
  abort.abort(reason);
  const previous = Error.prepareStackTrace;
  let pending: ReturnType<typeof renderToStringAsync>;
  try {
    Error.prepareStackTrace = () => {
      throw Error('diagnostic hook must not run');
    };
    pending = renderToStringAsync(
      () => {
        throw Error('view must not run');
      },
      { signal: abort.signal, timeoutMs: 100 },
    );
  } finally {
    Error.prepareStackTrace = previous;
  }
  await expect(pending!).rejects.toBe(reason);
});

it('retires an expired request even when stack formatting throws', async () => {
  let cleaned = 0,
    formatted = 0,
    core!: ReturnType<typeof getWasmCore>;
  const previous = Error.prepareStackTrace;
  let failure: unknown;
  try {
    Error.prepareStackTrace = () => {
      formatted++;
      throw Error('formatter failed');
    };
    failure = await renderToStringAsync(
      () => {
        core = getWasmCore();
        return {
          node: h(async () => {
            onCleanup(() => cleaned++);
            await new Promise(() => {});
            return h('p', null);
          }, null),
        };
      },
      { timeoutMs: 5 },
    ).then(
      () => undefined,
      error => error,
    );
  } finally {
    Error.prepareStackTrace = previous;
  }
  expect(formatted).toBeGreaterThan(0);
  expect(failure).toBeInstanceOf(SsrTimeoutError);
  expect((failure as SsrTimeoutError).timeoutMs).toBe(5);
  expect((failure as Error).stack).toBe('SsrTimeoutError: SSR request timed out after 5ms');
  expect(cleaned).toBe(1);
  expect(core.svet_get_node_count()).toBe(0);
});
