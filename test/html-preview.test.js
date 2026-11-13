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
function render(t, source, { interactive = true, state = {}, files } = {}) {
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
