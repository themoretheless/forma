/** @vitest-environment jsdom */
import { beforeEach, expect, it, vi } from 'vitest';
import { bindProp, createRoot, h, mount, resetReactiveSystem, signal } from '../src/engine';

beforeEach(() => { resetReactiveSystem(); document.body.innerHTML = ''; });

it('removes model listeners on unmount even if callers retain the detached input', () => {
  const value = signal('before');
  const app = mount(() => h('input', { model: value }), document.body);
  const input = document.querySelector('input')!;
  app.unmount();
  input.value = 'after';
  input.dispatchEvent(new Event('input'));
  expect(value()).toBe('before');
});

it('removes delegated callbacks from retained nodes on owner disposal', () => {
  let calls = 0;
  const app = mount(() => h('button', { onClick: () => { calls++; } }, 'click'), document.body);
  const button = document.querySelector('button')!;
  app.unmount();
  document.body.append(button);
  button.click();
  expect(calls).toBe(0);
});

it('releases only callbacks owned by the disposed scope when a node has multiple bindings', () => {
  const button = document.createElement('button');
  document.body.append(button);
  const calls: string[] = [];
  let stopA!: () => void, stopB!: () => void;
  createRoot(stop => { stopA = stop; bindProp(button, 'onClick', () => { calls.push('a'); }); });
  createRoot(stop => { stopB = stop; bindProp(button, 'onClick', () => { calls.push('b'); }); });
  stopA(); button.click(); stopB(); button.click();
  expect(calls).toEqual(['b']);
});

it('delegates independently in each owner document', () => {
  const other = document.implementation.createHTMLDocument('other');
  const button = other.createElement('button');
  other.body.append(button);
  let calls = 0, stop!: () => void;
  createRoot(dispose => { stop = dispose; bindProp(button, 'onClick', () => { calls++; }); });
  button.click(); stop();
  expect(calls).toBe(1);
});

it('provides the bound element as currentTarget while retaining native event methods', () => {
  const button = document.createElement('button'); document.body.append(button);
  let target: EventTarget | null = null, stop!: () => void;
  createRoot(dispose => { stop = dispose; bindProp(button, 'onClick', (event: Event) => {
    target = event.currentTarget;
    event.preventDefault();
  }); });
  const event = new MouseEvent('click', { bubbles: true, cancelable: true });
  button.dispatchEvent(event); stop();
  expect(target).toBe(button);
  expect(event.defaultPrevented).toBe(true);
});

it('honors stopImmediatePropagation across callbacks on the same element', () => {
  const button = document.createElement('button'); document.body.append(button);
  const calls: string[] = []; let stop!: () => void;
  createRoot(dispose => {
    stop = dispose;
    bindProp(button, 'onClick', (event: Event) => { calls.push('first'); event.stopImmediatePropagation(); });
    bindProp(button, 'onClick', () => { calls.push('second'); });
  });
  button.click(); stop();
  expect(calls).toEqual(['first']);
});

it('uses the composed event path to reach controls inside an open shadow root', () => {
  const host = document.createElement('div'); document.body.append(host);
  const shadow = host.attachShadow({ mode: 'open' });
  const button = document.createElement('button'); shadow.append(button);
  let calls = 0, stop!: () => void;
  createRoot(dispose => { stop = dispose; bindProp(button, 'onClick', () => { calls++; }); });
  button.click(); stop();
  expect(calls).toBe(1);
});

it('mounting a replacement releases the old view and stale unmount handles leave it intact', () => {
  const value = signal('one');
  const first = mount(() => h('input', { model: value }), document.body);
  const oldInput = document.querySelector('input')!;
  const second = mount(() => h('p', null, 'replacement'), document.body);
  oldInput.value = 'late'; oldInput.dispatchEvent(new Event('input'));
  expect(value()).toBe('one');
  first.unmount();
  expect(document.body.textContent).toBe('replacement');
  second.unmount();
});

it('failed mount cleans prepared bindings and preserves an already mounted view', () => {
  const first = mount(() => h('p', null, 'original'), document.body);
  const value = signal('before'); let input!: HTMLInputElement;
  expect(() => mount(() => {
    input = h('input', { model: value }) as HTMLInputElement;
    throw new Error('mount failed');
  }, document.body)).toThrow('mount failed');
  input.value = 'late'; input.dispatchEvent(new Event('input'));
  expect(value()).toBe('before');
  expect(document.body.textContent).toBe('original');
  first.unmount();
});

it('parent owner disposal unmounts a nested mount', () => {
  let stop!: () => void;
  createRoot(dispose => { stop = dispose; mount(() => h('p', null, 'child'), document.body); });
  stop();
  expect(document.body.textContent).toBe('');
});

it('removes obsolete object style properties when a reactive style changes shape', () => {
  const style = signal<Record<string, string>>({ color: 'red', backgroundColor: 'black', '--accent': 'blue' });
  const app = mount(() => h('p', { style }, 'styled'), document.body);
  const element = document.querySelector('p')!;
  style.set({ color: 'green' });
  expect(element.style.color).toBe('green');
  expect(element.style.backgroundColor).toBe('');
  expect(element.style.getPropertyValue('--accent')).toBe('');
  app.unmount();
});

it('clears previously bound styles when switching between string, object and null', () => {
  const style = signal<string | Record<string, string> | null>('color: red; background-color: black');
  const app = mount(() => h('p', { style }, 'styled'), document.body);
  const element = document.querySelector('p')!;
  style.set({ color: 'green' });
  expect(element.style.backgroundColor).toBe('');
  style.set(null);
  expect(element.style.cssText).toBe('');
  app.unmount();
});

it('shares a single document listener and releases it when its final binding is disposed', () => {
  const doc = document.implementation.createHTMLDocument('delegated');
  const add = vi.spyOn(doc, 'addEventListener'), remove = vi.spyOn(doc, 'removeEventListener');
  let stopA!: () => void, stopB!: () => void;
  const a = doc.createElement('button'), b = doc.createElement('button');
  createRoot(stop => { stopA = stop; bindProp(a, 'onClick', () => {}); });
  createRoot(stop => { stopB = stop; bindProp(b, 'onClick', () => {}); });
  expect(add.mock.calls.filter(call => call[0] === 'click')).toHaveLength(1);
  stopA();
  expect(remove.mock.calls.filter(call => call[0] === 'click')).toHaveLength(0);
  stopB();
  expect(remove.mock.calls.filter(call => call[0] === 'click')).toHaveLength(1);
  add.mockRestore(); remove.mockRestore();
});

it('forwards event property setters and clears currentTarget after dispatch', () => {
  const button = document.createElement('button'); document.body.append(button);
  let saved!: Event, stop!: () => void;
  createRoot(dispose => { stop = dispose; bindProp(button, 'onClick', (event: Event) => {
    saved = event; event.returnValue = false;
  }); });
  const native = new MouseEvent('click', { bubbles: true, cancelable: true });
  button.dispatchEvent(native); stop();
  expect(native.defaultPrevented).toBe(true);
  expect(saved.currentTarget).toBeNull();
});

it('retargets events separately for shadow-root and outer-document handlers', () => {
  const host = document.createElement('div'); document.body.append(host);
  const shadow = host.attachShadow({ mode: 'open' });
  const button = document.createElement('button'), span = document.createElement('span');
  button.append(span); shadow.append(button);
  const observed: Array<[EventTarget | null, EventTarget | null]> = [];
  let stop!: () => void;
  createRoot(dispose => {
    stop = dispose;
    bindProp(button, 'onClick', (event: Event) => { observed.push([event.target, event.currentTarget]); });
    bindProp(host, 'onClick', (event: Event) => { observed.push([event.target, event.currentTarget]); });
  });
  span.dispatchEvent(new MouseEvent('click', { bubbles: true, composed: true }));
  stop();
  expect(observed).toEqual([[span, button], [host, host]]);
});
