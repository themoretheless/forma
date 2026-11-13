// The HTML preview against a stand-in for the DOM: what it builds for a compiled document.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { parse } from '../src/language.js';
import { renderHtmlPreview } from '../src/html-preview.js';
import { sampleProject } from '../src/sample-project.js';
import { addableProperties, propertyStart } from '../src/property-catalog.js';

class FakeElement {
  constructor(tag) {
    this.tagName = tag.toUpperCase();
    this.className = '';
    this.dataset = {};
    this.style = {
      setProperty(name, value) {
        this[name] = value;
      },
    };
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
  fire(type, detail = {}) {
    const event = { type, target: this, stopPropagation() {}, preventDefault() {}, ...detail };
    for (const listener of this.listeners[type] ?? []) listener(event);
  }
  contains() {
    return false;
  }
  querySelectorAll() {
    return [];
  }
}

/** Renders `source` and returns the element of the preview root. */
function render(t, source, { interactive = true, state = {}, files, handlers = {} } = {}) {
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
    files,
    interactive,
    design: null,
    onSelect() {},
    onEvent() {},
    onLog() {},
    onChange() {},
    ...handlers,
  });
  return preview.children[0];
}

const SVG = '<svg viewBox="0 0 24 24"><circle cx="12" cy="12" r="8"/></svg>';

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

test('a text input can be multiline and colour its placeholder and focus border', t => {
  const [single, multi] = render(
    t,
    `component Demo { Frame {
       TextInput { placeholder: 'one'; placeholderColor: #98a4ba; focusBorderColor: #9ab3ff; }
       TextInput { multiline: true; }
     } }`,
  ).children;
  assert.equal(single.tagName, 'INPUT');
  assert.equal(single.style['--forma-placeholder'], '#98a4ba');
  assert.equal(single.dataset.placeholderColor, '');
  assert.equal(single.style['--forma-focus-border'], '#9ab3ff');
  assert.equal(single.dataset.focusBorder, '');
  assert.equal(multi.tagName, 'TEXTAREA');
});

test('every property the inspector offers for a control renders in the preview', t => {
  // Free placement reads the layout of the real DOM and is covered in the browser.
  const needsLayout = new Set(['x', 'y']);
  const rejected = [];
  for (const type of [
    'Button',
    'Text',
    'TextInput',
    'Frame',
    'Row',
    'Column',
    'Grid',
    'Stack',
    'Checkbox',
    'Slider',
    'Rectangle',
    'Image',
  ])
    for (const name of addableProperties(type)) {
      if (needsLayout.has(name)) continue;
      const source = `component Demo { Frame { ${type} { ${name}: ${propertyStart[name]}; } } }`;
      try {
        render(t, source, { files: { 'asset.png': SVG } });
      } catch (error) {
        rejected.push(`${type}.${name}: ${error.message}`);
      }
    }
  assert.deepEqual(rejected, []);
});

test('a rectangle is a box with its background, border and radius', t => {
  const box = render(
    t,
    'component Demo { Frame { Rectangle { width: 40; height: 20; background: #123456; radius: 4; } } }',
  ).children[0];
  assert.equal(box.tagName, 'DIV');
  assert.deepEqual(
    [box.style.width, box.style.height, box.style.background, box.style.borderRadius],
    ['40px', '20px', '#123456', '4px'],
  );
});

test('an image shows the project file it names', t => {
  const source =
    "component Demo { Frame { Image { source: 'assets/dot.svg'; width: 24; height: 24; } } }";
  const image = render(t, source, { files: { 'assets/dot.svg': SVG } }).children[0];
  assert.equal(image.tagName, 'IMG');
  assert.equal(image.src, 'data:image/svg+xml;charset=utf-8,' + encodeURIComponent(SVG));
  assert.equal(image.style.width, '24px');
  // An image is drawn, never run: markup inside it stays text of a data URL.
  const hostile = '<svg onload="alert(1)"><script>alert(2)</script></svg>';
  const drawn = render(t, source, { files: { 'assets/dot.svg': hostile } }).children[0];
  assert.equal(drawn.tagName, 'IMG');
  assert.ok(!drawn.src.includes('<'), drawn.src);

  for (const files of [{}, undefined])
    assert.throws(
      () => render(t, source, { files }),
      /SVG-файл не найден в проекте: assets\/dot.svg/,
    );
  // A name every object has is not a project file.
  assert.throws(
    () => render(t, source.replace('assets/dot.svg', 'toString'), { files: {} }),
    /SVG-файл не найден в проекте: toString/,
  );
});

/** Records what the preview reports to Studio. */
function recorder() {
  const calls = [];
  return {
    calls,
    handlers: {
      onSelect: node => calls.push(['select', node.type]),
      onEvent: (action, node, event = 'clicked') => calls.push(['event', node.type, event]),
      onLog: text => calls.push(['log', text]),
      onChange: () => calls.push(['change']),
    },
  };
}

const CLICKABLE = `component Demo { Frame { Button { text: 'Go'; clicked -> actions.go(); } } }`;

test('a click runs the handler while interacting and selects the node while designing', t => {
  const live = recorder();
  render(t, CLICKABLE, { handlers: live.handlers }).children[0].fire('click');
  assert.deepEqual(live.calls, [['event', 'Button', 'clicked']]);

  const design = recorder();
  render(t, CLICKABLE, { interactive: false, handlers: design.handlers }).children[0].fire('click');
  assert.deepEqual(design.calls, [['select', 'Button']]);
});

test('a click on a control without a handler reports nothing', t => {
  const live = recorder();
  const source = "component Demo { Frame { Button { text: 'Go'; } } }";
  render(t, source, { handlers: live.handlers }).children[0].fire('click');
  assert.deepEqual(live.calls, []);
});

test('typing into a bound input writes the state, logs it and asks for a redraw', t => {
  const live = recorder();
  const state = { query: 'old' };
  const input = render(t, `component Demo { Frame { TextInput { value <-> state.query; } } }`, {
    state,
    handlers: live.handlers,
  }).children[0];
  assert.equal(input.value, 'old');
  input.value = 'new';
  input.fire('input');
  assert.equal(state.query, 'new');
  assert.deepEqual(live.calls, [['log', 'state.query = "new"'], ['change']]);
});

test('Enter submits a text input only while interacting', t => {
  const source = `component Demo { Frame { TextInput { submitted -> actions.find(); } } }`;
  const live = recorder();
  const input = render(t, source, { handlers: live.handlers }).children[0];
  input.fire('keydown', { key: 'a' });
  assert.deepEqual(live.calls, []);
  input.fire('keydown', { key: 'Enter' });
  assert.deepEqual(live.calls, [['event', 'TextInput', 'submitted']]);

  const design = recorder();
  render(t, source, { interactive: false, handlers: design.handlers }).children[0].fire('keydown', {
    key: 'Enter',
  });
  assert.deepEqual(design.calls, []);
});

test('a closed modal is hidden and an open one dims what is behind it', t => {
  const modal = open =>
    render(
      t,
      `component Demo { Frame { Modal { open: ${open}; backdrop: #00000099;
         dismissed -> actions.close(); Frame { Text { text: 'a'; } } } } }`,
    ).children[0];
  assert.equal(modal(false).hidden, true);
  const shown = modal(true);
  assert.notEqual(shown.hidden, true);
  assert.equal(shown.style.boxShadow, '0 0 0 9999px #00000099');
  assert.equal(shown.style.zIndex, '10');
});

test('a double click dismisses a modal that allows it', t => {
  const source = dismiss => `component Demo { Frame { Modal { ${dismiss}
    dismissed -> actions.close(); Frame { Text { text: 'a'; } } } } }`;
  const live = recorder();
  render(t, source(''), { handlers: live.handlers }).children[0].fire('dblclick');
  assert.deepEqual(live.calls, [['event', 'Modal', 'dismissed']]);

  const locked = recorder();
  render(t, source('dismiss: false;'), { handlers: locked.handlers }).children[0].fire('dblclick');
  assert.deepEqual(locked.calls, []);

  const design = recorder();
  render(t, source(''), { interactive: false, handlers: design.handlers }).children[0].fire(
    'dblclick',
  );
  assert.deepEqual(design.calls, []);
});

test('text takes a monospace family and a wrapping mode, and rejects other values', t => {
  const text = body =>
    render(t, `component Demo { Frame { Text { text: 'a'; ${body} } } }`).children[0];
  assert.equal(text("fontFamily: 'mono';").style.fontFamily, 'ui-monospace, monospace');
  assert.equal(text("fontFamily: 'sans';").style.fontFamily, undefined);
  assert.equal(text('wrap: true;').style.whiteSpace, 'pre-wrap');
  assert.equal(text('wrap: false;').style.whiteSpace, 'pre');
  assert.throws(() => text("fontFamily: 'serif';"), /Text\.fontFamily ожидает 'sans' или 'mono'/);
  assert.throws(() => text('wrap: 3;'), /Text\.wrap ожидает true или false/);
  assert.throws(
    () => render(t, "component Demo { Frame { Button { text: 'a'; wrap: true; } } }"),
    /Text\.wrap ожидает true или false/,
  );
});

test('a slider is a range input on the scale its properties give', t => {
  const slider = body =>
    render(t, `component Demo { state level: Number = 40; Frame { Slider { ${body} } } }`, {
      state: { level: 40 },
    }).children[0];
  const scaled = slider('min: 0; max: 100; step: 5; value <-> state.level;');
  assert.equal(scaled.type, 'range');
  assert.deepEqual([scaled.min, scaled.max, scaled.step], ['0', '100', '5']);
  assert.equal(scaled.value, 40);
  // Without a scale the input runs from zero to one and accepts any value in between.
  const plain = slider('value <-> state.level;');
  assert.deepEqual([plain.min, plain.max, plain.step], ['0', '1', 'any']);
});
