/** @vitest-environment jsdom */
import { beforeEach, expect, it } from 'vitest';
import { For, getGraphTelemetry, h, mount, resetReactiveSystem, signal } from '../src/engine';
beforeEach(() => {
  resetReactiveSystem();
  document.body.replaceChildren();
});
const options = () => [h('option', { value: 'a' }, 'A'), h('option', { value: 'b' }, 'B')];
it('applies static select value after constructing its options', () => {
  const app = mount(() => h('select', { value: 'b' }, options()), document.body);
  expect(document.querySelector('select')!.value).toBe('b');
  app.unmount();
});
it('initializes and updates one-way select values without publishing DOM changes', () => {
  const value = signal('b'),
    app = mount(() => h('select', { value }, options()), document.body),
    select = document.querySelector('select')!;
  expect(select.value).toBe('b');
  value.set('a');
  expect(select.value).toBe('a');
  select.value = 'b';
  select.dispatchEvent(new Event('change'));
  expect(value()).toBe('a');
  value.set('missing');
  expect(select.selectedIndex).toBe(-1);
  app.unmount();
  value.dispose();
  expect(getGraphTelemetry().nodes).toBe(0);
});

const mutations = async () => {
  await Promise.resolve();
  await Promise.resolve();
};
it('reapplies one-way values after option insertions, removals and moves', async () => {
  const rows = signal(['a']),
    value = signal('b');
  const app = mount(
    () =>
      h(
        'select',
        { value },
        For({ each: rows, children: item => h('option', { value: item }, item) }),
      ),
    document.body,
  );
  const select = document.querySelector('select')!;
  expect(select.selectedIndex).toBe(-1);
  rows.set(['a', 'b']);
  await mutations();
  expect(select.value).toBe('b');
  rows.set(['b', 'a']);
  await mutations();
  expect(select.value).toBe('b');
  rows.set(['a']);
  await mutations();
  expect(select.selectedIndex).toBe(-1);
  expect(value()).toBe('b');
  app.unmount();
  rows.dispose();
  value.dispose();
  expect(getGraphTelemetry().nodes).toBe(0);
});
it('refreshes implicit text and explicit option values without reevaluating the binding', async () => {
  const label = signal('a'),
    optionValue = signal('b');
  let reads = 0;
  const app = mount(
    () =>
      h(
        'select',
        {
          value: () => {
            reads++;
            return 'target';
          },
        },
        h('option', null, label),
        h('option', { value: optionValue }, 'explicit'),
      ),
    document.body,
  );
  const select = document.querySelector('select')!;
  label.set('target');
  await mutations();
  expect(select.selectedIndex).toBe(0);
  label.set('gone');
  optionValue.set('target');
  await mutations();
  expect(select.selectedIndex).toBe(1);
  expect(reads).toBe(1);
  app.unmount();
  label.dispose();
  optionValue.dispose();
  expect(getGraphTelemetry().nodes).toBe(0);
});
it('disconnects queued selection refresh when unmounted', async () => {
  const value = signal('b'),
    app = mount(() => h('select', { value }, options()), document.body),
    select = document.querySelector('select')!;
  select.append(h('option', { value: 'b' }, 'late'));
  app.unmount();
  select.selectedIndex = 0;
  value.set('missing');
  await mutations();
  expect(select.selectedIndex).toBe(0);
  value.dispose();
  expect(getGraphTelemetry().nodes).toBe(0);
});

it('ignores queued selection work after a reactive generation reset', async () => {
  const value = signal('b'),
    app = mount(() => h('select', { value }, options()), document.body),
    select = document.querySelector('select')!;
  select.append(h('option', { value: 'b' }, 'queued'));
  resetReactiveSystem();
  const fresh = signal(9);
  select.selectedIndex = 0;
  await mutations();
  expect(select.selectedIndex).toBe(0);
  expect(fresh()).toBe(9);
  app.unmount();
  fresh.dispose();
  expect(getGraphTelemetry().nodes).toBe(0);
});
