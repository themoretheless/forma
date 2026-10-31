/** @vitest-environment jsdom */
import {beforeEach, expect, it} from 'vitest';
import {createRoot, mount, Suspense, ErrorBoundary, createResource, createGpuTensor, effect, signal, getGraphTelemetry, resetReactiveSystem} from '../src/engine';

beforeEach(() => { resetReactiveSystem(); document.body.innerHTML = ''; });

it('Suspense unmount releases its pending counter and owned resource state', async () => {
  let resolve!: (value: string) => void;
  const pending = new Promise<string>(yes => { resolve = yes; });
  const app = mount(() => Suspense({fallback: 'waiting', children: () => { createResource(() => pending); return document.createTextNode('ready'); }}), document.body);
  app.unmount(); expect(getGraphTelemetry().nodes).toBe(0); resolve('late'); await pending;
  expect(getGraphTelemetry().nodes).toBe(0);
});

it.each([false,true])('ErrorBoundary unmount releases its independent %s branch root and internal signals', fallback => {
  const source = signal(0); let runs = 0;
  const content = () => { createGpuTensor([1]); effect(() => { source(); runs++; }); return document.createTextNode('content'); };
  const app = mount(() => ErrorBoundary({children: () => { if (fallback) throw new Error('failed'); return content(); }, fallback: content}),document.body);
  expect(runs).toBe(1); app.unmount(); source.set(1); expect(runs).toBe(1);
  expect(getGraphTelemetry().nodes).toBe(1); source.dispose();
});

it('failed Suspense construction releases content resources when fallback construction throws', () => {
  expect(() => mount(() => Suspense({children: () => { createGpuTensor([1]); return document.createTextNode('content'); }, fallback: () => { throw new Error('fallback failed'); }}), document.body)).toThrow('fallback failed');
  expect(getGraphTelemetry().nodes).toBe(0);
});

it('failed ErrorBoundary fallback construction releases its own internal signals', () => {
  expect(() => mount(() => ErrorBoundary({children: () => { throw new Error('content failed'); }, fallback: () => { createGpuTensor([1]); throw new Error('fallback failed'); }}),document.body)).toThrow('fallback failed');
  expect(getGraphTelemetry().nodes).toBe(0);
});

it('reset after an async failure reconstructs the content exactly once', async () => {
  let reject!: (reason: Error) => void, reset!: () => void, mounts = 0;
  const pending = new Promise<string>((_, no) => { reject = no; });
  const app = mount(() => ErrorBoundary({children: () => {
    mounts++; createResource(() => mounts === 1 ? pending : Promise.resolve('ready')); return document.createTextNode('content');
  },fallback: (_, retry) => { reset = retry; return document.createTextNode('error'); }}),document.body);
  reject(new Error('request failed')); await pending.catch(() => {});
  expect(document.body.textContent).toBe('error'); reset(); expect(mounts).toBe(2);
  app.unmount(); expect(getGraphTelemetry().nodes).toBe(0);
});

it.each(['Suspense','ErrorBoundary'] as const)('%s rejects a disposed owner before allocating state or calling children', kind => {
  createRoot(stop => {
    stop(); let called = false;
    const children = () => { called = true; return document.createTextNode('content'); };
    expect(() => kind === 'Suspense' ? Suspense({children, fallback: 'waiting'}) : ErrorBoundary({children, fallback: () => document.createTextNode('error')})).toThrow('disposed owner');
    expect(called).toBe(false); expect(getGraphTelemetry().nodes).toBe(0);
  });
});

it('a retained reset callback cannot remount an unmounted boundary', () => {
  let reset!: () => void, mounts = 0;
  const app = mount(() => ErrorBoundary({children: () => { mounts++; throw new Error('failed'); }, fallback: (_, retry) => { reset = retry; return document.createTextNode('error'); }}),document.body);
  app.unmount(); reset(); expect(mounts).toBe(1); expect(getGraphTelemetry().nodes).toBe(0); expect(document.body.childNodes).toHaveLength(0);
});
