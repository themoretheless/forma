/** @vitest-environment jsdom */
import { beforeEach, expect, it } from 'vitest';
import { createResource, createRoot, resetReactiveSystem, signal, Suspense } from '../src/engine';

beforeEach(() => resetReactiveSystem());

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}

it('abort settles loading even when the fetcher ignores cancellation', async () => {
  const request = deferred<string>();
  const resource = createResource(() => request.promise);
  expect(resource.loading()).toBe(true);
  resource.abort();
  expect(resource.loading()).toBe(false);
  request.resolve('stale');
  await request.promise;
  expect(resource()).toBeUndefined();
  expect(resource.error()).toBeNull();
});

it('keeps Suspense pending when an obsolete request settles before its replacement', async () => {
  const requests = [deferred<string>(), deferred<string>()];
  const source = signal(0);
  let resource!: ReturnType<typeof createResource<string, number>>;
  const container = document.createElement('div');
  let dispose!: () => void;
  createRoot(stop => {
    dispose = stop;
    container.append(Suspense({
      fallback: 'waiting',
      children: () => {
        resource = createResource(source, value => requests[value].promise);
        return document.createTextNode('ready');
      },
    }));
  });
  const fallback = container.querySelector<HTMLElement>('[data-svet-suspense-fallback]')!;
  source.set(1);
  requests[0].resolve('stale');
  await requests[0].promise;
  expect(resource.loading()).toBe(true);
  expect(fallback.style.display).toBe('contents');
  requests[1].resolve('current');
  await requests[1].promise;
  expect(resource()).toBe('current');
  expect(resource.loading()).toBe(false);
  expect(fallback.style.display).toBe('none');
  dispose();
});

it('disabling the source cancels pending work and ignores its rejection', async () => {
  const source = signal<number | false>(1);
  const request = deferred<string>();
  let requestSignal!: AbortSignal;
  const resource = createResource(source, (_, ctx) => {
    requestSignal = ctx.signal;
    return request.promise;
  });
  source.set(false);
  expect(requestSignal.aborted).toBe(true);
  expect(resource.loading()).toBe(false);
  request.reject(new Error('obsolete'));
  await request.promise.catch(() => {});
  expect(resource.error()).toBeNull();
});

it('refetch after owner disposal does not start another request', async () => {
  let calls = 0;
  let dispose!: () => void;
  const resource = createRoot(stop => {
    dispose = stop;
    return createResource(async () => { calls++; return 'value'; });
  });
  dispose();
  expect(resource.loading()).toBe(false);
  await resource.refetch();
  expect(calls).toBe(1);
  expect(resource()).toBeUndefined();
});
