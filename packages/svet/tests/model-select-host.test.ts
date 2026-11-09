/** @vitest-environment jsdom */
import { beforeEach, expect, it } from 'vitest';
import { bindProp, getGraphTelemetry, h, mount, resetReactiveSystem, signal } from '../src/engine';
beforeEach(() => {
  resetReactiveSystem();
  document.body.replaceChildren();
});
it('models a getter-only select value through options and preserves first-match semantics', () => {
  const selected = signal('b');
  let select!: HTMLSelectElement;
  const app = mount(() => {
    select = h(
      'select',
      null,
      h('option', { value: 'a' }, 'A'),
      h('option', { value: 'b' }, 'B'),
      h('option', { value: 'b' }, 'duplicate'),
    ) as HTMLSelectElement;
    Object.defineProperty(select, 'value', {
      get: () => Array.from(select.options).find(option => option.selected)?.value ?? '',
    });
    bindProp(select, 'model', selected);
    return select;
  }, document.body);
  expect(select.value).toBe('b');
  expect(select.selectedIndex).toBe(1);
  selected.set('missing');
  expect(select.selectedIndex).toBe(-1);
  expect(select.value).toBe('');
  selected.set('a');
  expect(select.selectedIndex).toBe(0);
  selected.set('b');
  expect(Array.from(select.options, option => option.selected)).toEqual([false, true, false]);
  select.selectedIndex = 0;
  select.dispatchEvent(new Event('change'));
  expect(selected()).toBe('a');
  app.unmount();
  selected.dispose();
  expect(getGraphTelemetry().nodes).toBe(0);
});
