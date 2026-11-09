/** @vitest-environment jsdom */
import { beforeEach, expect, it } from 'vitest';
import {
  createSsrStreamReceiver,
  disposeResumedContainer,
  getGraphTelemetry,
  resetReactiveSystem,
  resumeContainer,
  serializeState,
  signal,
} from '../src/engine';
beforeEach(() => resetReactiveSystem());
const shell = (sequence = 0) =>
  ({
    version: 1,
    sequence,
    streamId: 'v1',
    payload: { type: 'shell', html: '<!--svet:v1:1--><!--svet:v1:1:end-->' },
  }) as any;
it('rejects unknown versions before inspecting payload or changing DOM', () => {
  const host = document.createElement('div');
  host.innerHTML = '<b>existing</b>';
  const old = host.firstChild,
    receiver = createSsrStreamReceiver(host);
  let reads = 0;
  expect(() =>
    receiver.apply({
      version: 2,
      sequence: 0,
      streamId: 'future',
      get payload() {
        reads++;
        throw Error('must not inspect');
      },
    } as any),
  ).toThrow(/Unsupported SSR stream version/);
  expect(reads).toBe(0);
  expect(host.firstChild).toBe(old);
  expect(receiver.active).toBe(false);
});
it('rejects unversioned legacy records instead of silently interpreting them as v1', () => {
  const host = document.createElement('div');
  host.textContent = 'existing';
  const receiver = createSsrStreamReceiver(host);
  expect(() => receiver.apply({ type: 'shell', streamId: 'legacy', html: 'wrong' } as any)).toThrow(
    /Unsupported SSR stream version/,
  );
  expect(host.textContent).toBe('existing');
});
it('requires sequence zero and contiguous records; rejects duplicate patches before their HTML getter', () => {
  const host = document.createElement('div'),
    receiver = createSsrStreamReceiver(host);
  receiver.apply(shell());
  receiver.apply({
    version: 1,
    sequence: 1,
    streamId: 'v1',
    payload: { type: 'patch', boundary: 1, html: '<b>first</b>' },
  } as any);
  let reads = 0;
  const node = host.querySelector('b');
  expect(() =>
    receiver.apply({
      version: 1,
      sequence: 1,
      streamId: 'v1',
      get payload() {
        reads++;
        return { type: 'patch', boundary: 1, html: 'wrong' };
      },
    } as any),
  ).toThrow(/sequence/);
  expect(reads).toBe(0);
  expect(host.querySelector('b')).toBe(node);
  const other = createSsrStreamReceiver(host);
  expect(() => other.apply(shell(1))).toThrow(/sequence/);
  expect(host.querySelector('b')).toBe(node);
});
it('keeps pre-existing resumed bindings when protocol validation fails before a shell is accepted', () => {
  const host = document.createElement('div');
  host.innerHTML = '<input data-svet-model="count" type="number">';
  const source = signal(3),
    snapshot = serializeState({ count: source });
  source.dispose();
  const state = resumeContainer(host, snapshot),
    input = host.querySelector('input')!,
    receiver = createSsrStreamReceiver(host);
  expect(() => receiver.apply({ version: 99 } as any)).toThrow(/Unsupported SSR stream version/);
  input.value = '5';
  input.dispatchEvent(new Event('input'));
  expect(state.count()).toBe(5);
  disposeResumedContainer(host);
  expect(getGraphTelemetry().nodes).toBe(0);
});
it('rejects skipped and non-integer sequence values without reading payload', () => {
  for (const sequence of [2, -1, 1.5, NaN, Infinity, Number.MAX_SAFE_INTEGER + 1]) {
    const host = document.createElement('div'),
      receiver = createSsrStreamReceiver(host);
    receiver.apply(shell());
    const old = host.firstChild;
    let reads = 0;
    expect(() =>
      receiver.apply({
        version: 1,
        sequence,
        streamId: 'v1',
        get payload() {
          reads++;
          return { type: 'patch', boundary: 1, html: 'wrong' };
        },
      } as any),
    ).toThrow(/sequence/);
    expect(reads).toBe(0);
    expect(host.firstChild).toBe(old);
  }
});
it('rejects crossing or noncanonical boundary markers before replacing an existing shell', () => {
  for (const html of [
    '<!--svet:v1:1--><!--svet:v1:2--><!--svet:v1:1:end--><!--svet:v1:2:end-->',
    '<!--svet:v1:01--><!--svet:v1:01:end-->',
    '<!--svet:v1:1e0--><!--svet:v1:1e0:end-->',
    '<!--svet:v1:1--><span><!--svet:v1:1:end--></span>',
  ]) {
    const host = document.createElement('div');
    host.innerHTML = '<input value="preserve">';
    const original = host.firstChild,
      receiver = createSsrStreamReceiver(host);
    expect(() =>
      receiver.apply({ version: 1, sequence: 0, streamId: 'v1', payload: { type: 'shell', html } }),
    ).toThrow(/boundary|range/);
    expect(host.firstChild).toBe(original);
    expect(receiver.active).toBe(false);
  }
});
it('accepts properly nested and sibling boundaries in one parent', () => {
  const host = document.createElement('div'),
    receiver = createSsrStreamReceiver(host);
  receiver.apply({
    version: 1,
    sequence: 0,
    streamId: 'v1',
    payload: {
      type: 'shell',
      html: '<!--svet:v1:1--><!--svet:v1:2--><!--svet:v1:2:end--><!--svet:v1:1:end--><!--svet:v1:3--><!--svet:v1:3:end-->',
    },
  });
  receiver.apply({
    version: 1,
    sequence: 1,
    streamId: 'v1',
    payload: { type: 'patch', boundary: 2, html: '<b>nested</b>' },
  });
  expect(host.querySelector('b')?.textContent).toBe('nested');
  receiver.dispose();
});
it('validates patch boundaries before removing the previously delivered contents', () => {
  for (const html of [
    '<!--svet:v1:2--><!--svet:v1:3--><!--svet:v1:2:end--><!--svet:v1:3:end-->',
    '<!--svet:v1:1--><!--svet:v1:1:end-->',
    '<!--svet:v1:2-->',
  ]) {
    const host = document.createElement('div'),
      receiver = createSsrStreamReceiver(host);
    receiver.apply(shell());
    receiver.apply({
      version: 1,
      sequence: 1,
      streamId: 'v1',
      payload: { type: 'patch', boundary: 1, html: '<input value="keep">' },
    });
    const input = host.querySelector('input');
    expect(() =>
      receiver.apply({
        version: 1,
        sequence: 2,
        streamId: 'v1',
        payload: { type: 'patch', boundary: 1, html },
      }),
    ).toThrow(/boundary|range/);
    expect(host.querySelector('input')).toBe(input);
    expect(receiver.active).toBe(false);
  }
});

it('rejects truncated EOF and retires partial receivers', () => {
  const host = document.createElement('div'),
    receiver = createSsrStreamReceiver(host);
  receiver.apply(shell());
  expect(() => receiver.finish()).toThrow(/ended before complete/);
  expect(receiver.active).toBe(false);
  expect(() => receiver.apply(shell())).toThrow(/disposed/);
});
it('keeps completed resumed bindings active after EOF validation', () => {
  const host = document.createElement('div'),
    receiver = createSsrStreamReceiver(host),
    source = signal(3),
    stateJson = serializeState({ count: source });
  source.dispose();
  receiver.apply({
    version: 1,
    sequence: 0,
    streamId: 'v1',
    payload: { type: 'shell', html: '<input data-svet-model="count" type="number">' },
  });
  const state = receiver.apply({
    version: 1,
    sequence: 1,
    streamId: 'v1',
    payload: { type: 'complete', html: '<input data-svet-model="count" type="number">', stateJson },
  })!;
  receiver.finish();
  receiver.finish();
  const input = host.querySelector('input')!;
  input.value = '9';
  input.dispatchEvent(new Event('input'));
  expect(state.count()).toBe(9);
  expect(receiver.active).toBe(true);
  receiver.dispose();
  expect(getGraphTelemetry().nodes).toBe(0);
});
it('preserves unrelated resumed bindings when EOF arrives before any shell', () => {
  const host = document.createElement('div');
  host.innerHTML = '<input data-svet-model="count" type="number">';
  const source = signal(2),
    snapshot = serializeState({ count: source });
  source.dispose();
  const state = resumeContainer(host, snapshot);
  const receiver = createSsrStreamReceiver(host);
  expect(() => receiver.finish()).toThrow(/before complete/);
  const input = host.querySelector('input')!;
  input.value = '4';
  input.dispatchEvent(new Event('input'));
  expect(state.count()).toBe(4);
  disposeResumedContainer(host);
});
