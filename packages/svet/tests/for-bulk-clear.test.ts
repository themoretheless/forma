/** @vitest-environment jsdom */
import { beforeEach, expect, it } from 'vitest';
import {
  For,
  h,
  mount,
  onCleanup,
  signal,
  getGraphTelemetry,
  resetReactiveSystem,
} from '../src/engine';

beforeEach(() => {
  resetReactiveSystem();
  document.body.innerHTML = '';
});

const rowsOf = (n: number, from = 0) =>
  Array.from({ length: n }, (_, i) => ({ id: from + i, label: `row ${from + i}` }));

it('clearing a list that fills its parent runs every row cleanup and accepts new rows', () => {
  const items = signal(rowsOf(50));
  let cleaned = 0;
  const app = mount(
    () =>
      h(
        'ul',
        null,
        For({
          each: items,
          keyFn: item => item.id,
          children: (_item, _index, current) => {
            onCleanup(() => cleaned++);
            return h('li', null, () => current().label);
          },
        }),
      ),
    document.body,
  );
  const list = document.querySelector('ul')!;
  items.set([]);
  expect(cleaned).toBe(50);
  expect(list.querySelectorAll('li').length).toBe(0);
  items.set(rowsOf(3, 100));
  expect([...list.querySelectorAll('li')].map(li => li.textContent)).toEqual([
    'row 100',
    'row 101',
    'row 102',
  ]);
  app.unmount();
  items.dispose();
  expect(getGraphTelemetry().nodes).toBe(0);
});

it('clearing a list that shares its parent leaves the siblings in place', () => {
  const items = signal(rowsOf(5));
  const app = mount(
    () =>
      h(
        'div',
        null,
        h('b', null, 'before'),
        For({
          each: items,
          keyFn: item => item.id,
          children: item => h('i', null, item.label),
        }),
        h('b', null, 'after'),
      ),
    document.body,
  );
  items.set([]);
  expect(document.querySelector('div')!.textContent).toBe('beforeafter');
  items.set(rowsOf(2));
  expect(document.querySelector('div')!.textContent).toBe('beforerow 0row 1after');
  app.unmount();
  items.dispose();
});

it('reactive text that switches to a node and back keeps one live binding', () => {
  const value = signal<string | Node>('text');
  const app = mount(() => h('p', null, () => value()), document.body);
  const p = document.querySelector('p')!;
  expect(p.textContent).toBe('text');
  value.set('again');
  expect(p.textContent).toBe('again');
  value.set(h('em', null, 'node'));
  expect(p.querySelector('em')?.textContent).toBe('node');
  expect(p.textContent).toBe('node');
  value.set('back');
  expect(p.textContent).toBe('back');
  app.unmount();
  value.dispose();
  expect(getGraphTelemetry().nodes).toBe(0);
});
