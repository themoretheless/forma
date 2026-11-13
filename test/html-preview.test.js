// The HTML preview against a stand-in for the DOM: what it builds for a compiled document.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { parse } from '../src/language.js';
import { renderHtmlPreview } from '../src/html-preview.js';
import { sampleProject } from '../src/sample-project.js';

class FakeElement {
  constructor(tag) {
    this.tagName = tag.toUpperCase();
    this.className = '';
    this.dataset = {};
    this.style = {};
    this.children = [];
    this.textContent = '';
    this.listeners = {};
    this.classList = { add() {}, remove() {}, contains: () => false };
  }
  append(...nodes) {
    for (const node of nodes) {
      if (node?.isFragment) this.children.push(...node.children);
      else if (node) this.children.push(node);
    }
  }
  replaceChildren(...nodes) {
    this.children = [];
    this.append(...nodes);
  }
  addEventListener(type, listener) {
    (this.listeners[type] ??= []).push(listener);
  }
  removeEventListener() {}
  /** Runs the listeners a real element would run for `type`. */
  fire(type) {
    for (const listener of this.listeners[type] ?? []) listener({ type, target: this });
  }
  contains() {
    return false;
  }
  querySelectorAll() {
    return [];
  }
}

/** Renders `source` and returns the element of the preview root. */
function render(t, source, { interactive = true, state = {} } = {}) {
  const previous = globalThis.document;
  globalThis.document = {
    activeElement: null,
    createElement: tag => new FakeElement(tag),
    createDocumentFragment: () => ({
      isFragment: true,
      children: [],
      append(...nodes) {
        this.children.push(...nodes);
      },
    }),
  };
  t.after(() => (globalThis.document = previous));
  const preview = new FakeElement('div');
  renderHtmlPreview({
    preview,
    compiled: parse(source),
    state,
    interactive,
    design: null,
    onSelect() {},
    onEvent() {},
    onLog() {},
    onChange() {},
  });
  return preview.children[0];
}

const BUTTON = `component Demo { Frame { Button { text: 'Go'; background: #aad9bd;
  hoverBackground: #c7eed7; pressedBackground: #78b493; disabledBackground: #596273; STATE } } }`;

test('every form of the sample project renders in the HTML preview', t => {
  for (const [path, source] of Object.entries(sampleProject))
    if (path.startsWith('ui/') && path.endsWith('.ui') && !path.endsWith('.design.ui'))
      assert.doesNotThrow(() => render(t, source), path);
});

test('a button shows its hover and pressed backgrounds while the pointer is on it', t => {
  const button = render(t, BUTTON.replace('STATE', '')).children[0];
  assert.equal(button.style.background, '#aad9bd');
  button.fire('pointerenter');
  assert.equal(button.style.background, '#c7eed7');
  button.fire('pointerdown');
  assert.equal(button.style.background, '#78b493');
  button.fire('pointerup');
  assert.equal(button.style.background, '#c7eed7');
  button.fire('pointerdown');
  button.fire('pointerleave');
  assert.equal(button.style.background, '#aad9bd', 'leaving ends both states');
});

test('a disabled button takes its disabled background and ignores the pointer', t => {
  const button = render(t, BUTTON.replace('STATE', 'disabled: true;')).children[0];
  assert.equal(button.style.background, '#596273');
  button.fire('pointerenter');
  button.fire('pointerdown');
  assert.equal(button.style.background, '#596273');
});

test('in design mode the pointer does not repaint a button', t => {
  const button = render(t, BUTTON.replace('STATE', ''), { interactive: false }).children[0];
  button.fire('pointerenter');
  assert.equal(button.style.background, '#aad9bd');
});

test('a property the preview has no meaning for is still an error', t => {
  assert.throws(
    () => render(t, "component Demo { Frame { Button { text: 'Go'; sparkle: 3; } } }"),
    /Свойство sparkle пока не поддерживается/,
  );
});
