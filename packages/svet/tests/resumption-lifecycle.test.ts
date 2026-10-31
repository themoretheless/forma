/** @vitest-environment jsdom */
import { beforeEach, expect, it } from 'vitest';
import { createRoot, disposeResumedContainer, getGraphTelemetry, resetReactiveSystem, resumeContainer } from '../src/engine';

beforeEach(() => { resetReactiveSystem(); document.body.innerHTML = ''; });
const snapshot = (bindings?: unknown[]) => JSON.stringify({ signals: { value: 'initial' }, bindings, timestamp: 0 });

it('limits ID bindings to the requested container even with duplicate IDs', () => {
  document.body.innerHTML = '<p id="label">outside</p><section><p id="label">inside</p></section>';
  const container = document.querySelector('section')!;
  const restored = resumeContainer(container, snapshot([{ elementId: 'label', signal: 'value', type: 'text' }]));
  restored.value.set('updated');
  expect(document.body.firstElementChild!.textContent).toBe('outside');
  expect(container.textContent).toBe('updated');
});

it('binds annotations on the container itself', () => {
  const container = document.createElement('p');
  container.dataset.svetText = 'value';
  const restored = resumeContainer(container, snapshot());
  expect(container.textContent).toBe('initial');
  restored.value.set('updated');
  expect(container.textContent).toBe('updated');
});

it('replaces previous resumption without retaining old model listeners or subscriptions', () => {
  const container = document.createElement('div');
  container.innerHTML = '<input data-svet-model="value">';
  const first = resumeContainer(container, snapshot());
  const second = resumeContainer(container, snapshot());
  const input = container.querySelector('input')!;
  input.value = 'typed';
  input.dispatchEvent(new Event('input'));
  expect(second.value()).toBe('typed');
  expect(first.value()).toBe('initial');
  first.value.set('stale');
  expect(input.value).toBe('typed');
  expect(getGraphTelemetry().nodes).toBe(2);
});

it('owner disposal removes resumed event listeners and reactive graph nodes', () => {
  const container = document.createElement('div');
  container.innerHTML = '<input data-svet-model="value">';
  let stop!: () => void;
  const restored = createRoot(dispose => { stop = dispose; return resumeContainer(container, snapshot()); });
  stop();
  const input = container.querySelector('input')!;
  input.value = 'after disposal';
  input.dispatchEvent(new Event('input'));
  expect(restored.value()).toBe('initial');
  expect(getGraphTelemetry().nodes).toBe(0);
});

it('explicit disposal is idempotent and releases both binding paths', () => {
  for (const table of [false, true]) {
    const container = document.createElement('div');
    container.innerHTML = '<input id="field" data-svet-model="value">';
    const restored = resumeContainer(container, snapshot(table ? [{ elementId: 'field', signal: 'value', type: 'model' }] : undefined));
    disposeResumedContainer(container);
    disposeResumedContainer(container);
    const input = container.querySelector('input')!;
    input.value = 'late';
    input.dispatchEvent(new Event('input'));
    restored.value.set('stale');
    expect(input.value).toBe('late');
    expect(getGraphTelemetry().nodes).toBe(0);
  }
});

it('accepts literal unusual IDs in detached containers without selector injection', () => {
  const container = document.createElement('div');
  const child = document.createElement('span');
  child.id = 'a"b]\\c';
  container.append(child);
  const restored = resumeContainer(container, snapshot([{ elementId: child.id, signal: 'value', type: 'text' }]));
  restored.value.set('updated');
  expect(child.textContent).toBe('updated');
  disposeResumedContainer(container);
});

it('rejects malformed snapshots before allocating nodes or replacing live bindings', () => {
  const container = document.createElement('div');
  container.innerHTML = '<input data-svet-model="value">';
  const restored = resumeContainer(container, snapshot());
  const count = getGraphTelemetry().nodes;
  for (const bad of ['null', '[]', '{}', '{"signals":[]}', '{"signals":{},"bindings":[null]}']) {
    expect(() => resumeContainer(container, bad)).toThrow(TypeError);
    expect(getGraphTelemetry().nodes).toBe(count);
  }
  restored.value.set('still live');
  expect(container.querySelector('input')!.value).toBe('still live');
  disposeResumedContainer(container);
});
