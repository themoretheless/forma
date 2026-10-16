import test from 'node:test';
import assert from 'node:assert/strict';
import { parse, resolve } from '../src/language.js';
import { compileComponents } from '../src/components.js';
import { renderHtmlPreview } from '../src/html-preview.js';

function createFakeDom() {
  class FakeElement {
    constructor(tagName) {
      this.tagName = tagName.toUpperCase();
      this.className = '';
      this.dataset = {};
      this.style = {};
      this.children = [];
      this.textContent = '';
      this.value = '';
      this.placeholder = '';
      this.disabled = false;
      this.checked = false;
      this.hidden = false;
      this.tabIndex = -1;
      this.listeners = new Map();
      this.classList = {
        add: (...names) => {
          const set = new Set(this.className.split(/\s+/).filter(Boolean));
          for (const n of names) set.add(n);
          this.className = [...set].join(' ');
        },
        remove: (...names) => {
          const set = new Set(this.className.split(/\s+/).filter(Boolean));
          for (const n of names) set.delete(n);
          this.className = [...set].join(' ');
        },
        contains: name => this.className.split(/\s+/).includes(name),
      };
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
    addEventListener(type, fn) {
      if (!this.listeners.has(type)) this.listeners.set(type, []);
      this.listeners.get(type).push(fn);
    }
    removeEventListener(type, fn) {
      const list = this.listeners.get(type);
      if (list) this.listeners.set(type, list.filter(item => item !== fn));
    }
    dispatchEvent(event) {
      const list = this.listeners.get(event.type) ?? [];
      for (const fn of [...list]) fn(event);
    }
    contains(other) {
      if (other === this) return true;
      return this.children.some(c => c.contains?.(other));
    }
    getBoundingClientRect() {
      return { left: 0, top: 0, width: 100, height: 30, right: 100, bottom: 30 };
    }
    querySelectorAll(selector) {
      const out = [];
      const matches = el => {
        if (selector.startsWith('.')) return el.classList.contains(selector.slice(1));
        if (selector === '[data-start]') return el.dataset.start !== undefined;
        return el.tagName.toLowerCase() === selector.toLowerCase();
      };
      const walk = el => {
        for (const child of el.children) {
          if (matches(child)) out.push(child);
          walk(child);
        }
      };
      walk(this);
      return out;
    }
    querySelector(selector) {
      return this.querySelectorAll(selector)[0] ?? null;
    }
  }
  const preview = new FakeElement('div');
  const document = {
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
  return { preview, document };
}

function withDom(run) {
  const { preview, document } = createFakeDom();
  const prevDoc = globalThis.document;
  globalThis.document = document;
  try {
    return run(preview);
  } finally {
    globalThis.document = prevDoc;
  }
}

test('1. computed let bindings inside component work with bare names and props.*', () => {
  const files = {
    'components/PriceCard.ui': `component PriceCard {
      prop price: Number = 100;
      prop qty: Number = 3;
      let subtotal = price * qty;
      let label = 'Итого: ' + formatNumber(subtotal, 2);
      Rectangle {
        width: 240; height: 60; background: #1e2536;
        Text { text: label; color: #ffffff; fontSize: 16; }
      }
    }`,
    'ui/Main.ui': `component Main {
      prop tax: Number = 20;
      let totalWidth = 240 + tax;
      Frame {
        width: totalWidth; height: 120;
        PriceCard { key: 'card'; price: 50; qty: 4; }
      }
    }`
  };
  const compiled = compileComponents(files, 'ui/Main.ui', {}, { measureText: (t, s) => [t.length * s * 0.6, s * 1.2] });
  assert.match(compiled.template, /Итого: 200\.00/);
  assert.equal(compiled.visualNodes[0].bounds[2], 260);
});

test('2. for loop index (for item, idx in ...) and range expressions (a..b, a..=b)', () => {
  const doc = parse(`component ListDemo {
    Column {
      for n, i in 1..=3 key i {
        Text { key: 'row'; text: String(i) + ':' + String(n); }
      }
      for x in 0..2 {
        Text { key: 'item'; text: String(x); }
      }
    }
  }`);
  withDom(preview => {
    renderHtmlPreview({
      preview,
      compiled: doc,
      state: {},
      interactive: false,
      design: null,
      onSelect() {},
      onEvent() {},
      onLog() {},
      onChange() {},
    });
    const texts = preview.querySelectorAll('.text').map(el => el.textContent);
    assert.deepEqual(texts, ['0:1', '1:2', '2:3', '0', '1']);
  });
});

test('3. named slots (slot header / @header { ... }) work alongside default content', () => {
  const files = {
    'components/PanelBox.ui': `component PanelBox {
      width: 260; height: 90;
      Rectangle {
        background: #1a2130;
        Column {
          slot header { Text { text: 'Default Header'; } }
          slot body { Text { text: 'Default Body'; } }
        }
      }
    }`,
    'components/HostButton.ui': `component HostButton {
      width: 260; height: 90;
      Rectangle {
        background: #20283a;
        PanelBox {
          @header { Text { text: 'Custom Header'; } }
        }
      }
    }`,
    'ui/App.ui': `component App {
      Frame {
        HostButton { key: 'btn'; }
      }
    }`
  };
  const compiled = compileComponents(files, 'ui/App.ui', {}, { measureText: (t, s) => [t.length * s * 0.6, s * 1.2] });
  assert.match(compiled.template, /Custom Header/);
  assert.match(compiled.template, /Default Body/);
  assert.doesNotMatch(compiled.template, /Default Header/);
});

test('4. expanded standard library pure functions and pipe operator |>', () => {
  assert.equal(resolve(parse(`component X { v: '  hello forma  ' |> trim |> upper; }`).defaults.v, {}), 'HELLO FORMA');
  assert.equal(resolve(parse(`component X { v: 'Hello World' |> lower |> replace('world', 'forma'); }`).defaults.v, {}), 'hello forma');
  assert.equal(resolve(parse(`component X { v: contains('forma studio', 'studio') && startsWith('forma', 'fo') && endsWith('forma', 'ma'); }`).defaults.v, {}), true);
  assert.equal(resolve(parse(`component X { v: contains([1, 2, 3], 2); }`).defaults.v, {}), true);
  assert.equal(resolve(parse(`component X { v: 'a,b,c,d' |> split(',') |> reverse |> take(3) |> join('-'); }`).defaults.v, {}), 'd-c-b');
  assert.equal(resolve(parse(`component X { v: [10, 20, 30, 40] |> skip(1) |> take(2) |> sum; }`).defaults.v, {}), 50);
  assert.equal(resolve(parse(`component X { v: slice('abcdef', 1, 4); }`).defaults.v, {}), 'bcd');
  assert.equal(resolve(parse(`component X { v: formatNumber(12.3456, 2); }`).defaults.v, {}), '12.35');
  assert.deepEqual(resolve(parse(`component X { v: range(2, 5); }`).defaults.v, {}), [2, 3, 4]);
});

test('5. design tokens (tokens { ... } and tokens.*) resolve in HTML and Vector compilation', () => {
  const files = {
    'components/ThemedBox.ui': `component ThemedBox {
      tokens {
        color.surface: #123456;
        color.text: #fedcba;
      }
      width: 200; height: 50;
      Rectangle {
        background: tokens.color.surface;
        Text { text: 'Token Text'; color: tokens.color.text; fontSize: 14; }
      }
    }`,
    'ui/Main.ui': `component Main {
      tokens {
        space.pad: 16;
      }
      Column {
        padding: tokens.space.pad;
        ThemedBox { key: 'box'; }
      }
    }`
  };
  const compiled = compileComponents(files, 'ui/Main.ui', {}, { measureText: (t, s) => [t.length * s * 0.6, s * 1.2] });
  assert.match(compiled.template, /#123456/);
  assert.match(compiled.template, /#fedcba/);
  assert.equal(compiled.previewControls[0].props.x, 16);
});

test('6. declarative transition property works in HTML preview and is accepted in Vector components', () => {
  const doc = parse(`component Anim {
    Column {
      transition: 'all 180ms ease';
      Text { key: 't'; text: 'Smooth'; transition: 'opacity 120ms linear'; }
    }
  }`);
  withDom(preview => {
    renderHtmlPreview({
      preview,
      compiled: doc,
      state: {},
      interactive: false,
      design: null,
      onSelect() {},
      onEvent() {},
      onLog() {},
      onChange() {},
    });
    const col = preview.querySelector('.column');
    const txt = preview.querySelector('.text');
    assert.equal(col.style.transition, 'all 180ms ease');
    assert.equal(txt.style.transition, 'opacity 120ms linear');
  });
});

test('7. keyboard shortcuts and TextInput submitted event trigger actions in interactive HTML preview', () => {
  const doc = parse(`component ShortcutsDemo {
    shortcut 'CmdOrCtrl+K' -> actions.openSearch('quick');
    Column {
      TextInput {
        key: 'search';
        value <-> state.query;
        submitted -> actions.submitQuery(state.query);
      }
    }
  }`);
  withDom(preview => {
    const fired = [];
    const state = { query: 'forma' };
    renderHtmlPreview({
      preview,
      compiled: doc,
      state,
      interactive: true,
      design: null,
      onSelect() {},
      onEvent(action, node, kind) {
        fired.push({ action, kind, args: node.eventArgs?.[kind] ?? [] });
      },
      onLog() {},
      onChange() {},
    });
    const input = preview.querySelector('input');
    input.dispatchEvent({ type: 'keydown', key: 'Enter', preventDefault() {}, stopPropagation() {} });
    preview.dispatchEvent({ type: 'keydown', key: 'k', ctrlKey: true, preventDefault() {}, stopPropagation() {} });
    assert.deepEqual(fired, [
      { action: 'actions.submitQuery', kind: 'submitted', args: ['forma'] },
      { action: 'actions.openSearch', kind: 'shortcut', args: ['quick'] },
    ]);
  });
});

test('8. struct declarations and List<T> type contracts validate nested objects and lists', () => {
  const baseFiles = {
    'components/UserBadge.ui': `component UserBadge {
      struct User {
        name: String;
        age: Int;
        role: String?;
      }
      required prop user: User;
      prop tags: List<String> = ['dev'];
      width: 200; height: 40;
      Rectangle {
        background: #222b3a;
        Text { text: props.user.name + ' (' + join(props.tags, '/') + ')'; }
      }
    }`,
  };
  const validFiles = {
    ...baseFiles,
    'ui/Main.ui': `component Main {
      Frame {
        UserBadge {
          key: 'u1';
          user: { name: 'Alice'; age: 29; };
          tags: ['admin', 'core'];
        }
      }
    }`
  };
  const compiled = compileComponents(validFiles, 'ui/Main.ui', {}, { measureText: (t, s) => [t.length * s * 0.6, s * 1.2] });
  assert.match(compiled.template, /Alice \(admin\/core\)/);

  const invalidStructFiles = {
    ...baseFiles,
    'ui/Main.ui': `component Main {
      Frame {
        UserBadge {
          key: 'u1';
          user: { name: 'Alice'; age: 'not-an-int'; };
        }
      }
    }`
  };
  assert.throws(
    () => compileComponents(invalidStructFiles, 'ui/Main.ui', {}, { measureText: (t, s) => [t.length * s * 0.6, s * 1.2] }),
    /UserBadge\.user: ожидался User/
  );

  const invalidListFiles = {
    ...baseFiles,
    'ui/Main.ui': `component Main {
      Frame {
        UserBadge {
          key: 'u1';
          user: { name: 'Alice'; age: 29; };
          tags: ['ok', 42];
        }
      }
    }`
  };
  assert.throws(
    () => compileComponents(invalidListFiles, 'ui/Main.ui', {}, { measureText: (t, s) => [t.length * s * 0.6, s * 1.2] }),
    /UserBadge\.tags: ожидался List<String>/
  );
});
