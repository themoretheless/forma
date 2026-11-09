/** @vitest-environment jsdom */
import { beforeEach, expect, it } from 'vitest';
import { bindProp, getGraphTelemetry, h, mount, resetReactiveSystem, signal } from '../src/engine';
beforeEach(() => {
  resetReactiveSystem();
  document.body.replaceChildren();
});
it('supports attribute-backed multiple DOM hosts without selectedOptions reflection', () => {
  const value = signal(['a', 'c']);
  let select!: HTMLSelectElement,
    setters = 0;
  const app = mount(() => {
    select = h(
      'select',
      { multiple: true },
      ...['a', 'b', 'c'].map(value => h('option', { value }, value)),
    ) as HTMLSelectElement;
    Object.defineProperty(select, 'multiple', { get: () => undefined });
    Object.defineProperty(select, 'selectedOptions', { get: () => undefined });
    for (const option of Array.from(select.options))
      Object.defineProperty(option, 'selected', {
        get: () => option.hasAttribute('selected'),
        set: () => {
          setters++;
          throw Error('host setter does not support multiple');
        },
      });
    bindProp(select, 'model', value);
    return select;
  }, document.body);
  const selected = () =>
    Array.from(select.options)
      .filter(option => option.selected)
      .map(option => option.value);
  expect(selected()).toEqual(['a', 'c']);
  value.set(['b']);
  expect(selected()).toEqual(['b']);
  select.options[0].setAttribute('selected', '');
  select.dispatchEvent(new Event('change'));
  expect(value()).toEqual(['a', 'b']);
  value.set([]);
  expect(selected()).toEqual([]);
  expect(setters).toBe(0);
  app.unmount();
  value.dispose();
  expect(getGraphTelemetry().nodes).toBe(0);
});
