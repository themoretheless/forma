import test from 'node:test';
import assert from 'node:assert/strict';
import { parse, formatSource, lintDocument } from '../src/language.js';
import { compileComponents } from '../src/components.js';
import { expandStructure } from '../src/component-semantics.js';

const metrics = {
  measureText: (t, s) => [String(t ?? '').length * s * 0.5, s * 1.2],
};

test('1. Declarative type constraints (#[range], #[pattern], isValid)', () => {
  const files = {
    'ui/Main.ui': `
      component Main {
        #[range(0, 100)]
        prop progress: Number = 50;

        #[range(2, 10)]
        prop label: String = 'OK';

        struct User {
          #[range(18, 120)]
          age: Int;
        }
        prop user: User = { age: 25; };

        Frame {
          Text { text: '\${progress}% \${label} (\${user.age})'; }
        }
      }
    `,
  };
  const compiled = compileComponents(files, 'ui/Main.ui', {}, metrics);
  assert.ok(compiled.visualNodes.some(v => v.props.text === '50% OK (25)'));

  assert.throws(
    () => compileComponents(files, 'ui/Main.ui', {}, { ...metrics, properties: { progress: 150 } }),
    /вне допустимого диапазона/
  );

  assert.throws(
    () => compileComponents(files, 'ui/Main.ui', {}, { ...metrics, properties: { label: 'A' } }),
    /вне допустимого диапазона/
  );

  assert.throws(
    () => compileComponents(files, 'ui/Main.ui', {}, { ...metrics, properties: { user: { age: 12 } } }),
    /вне допустимого диапазона/
  );

  const doc = parse(`
    component Form {
      #[pattern('email')]
      prop email: String = 'dev@forma.ui';

      #[pattern('^#[0-9a-fA-F]{6}$')]
      prop hexCode: String = '#38bdf8';

      let emailOk = isValid(email, 'email');
      let rangeOk = isValid(42, 10, 50);
      let regexOk = isValid('item_42', '^\\\\w+_\\\\d+$');

      Frame {
        Text { text: '\${emailOk}-\${rangeOk}-\${regexOk}'; }
      }
    }
  `);
  const expanded = expandStructure(doc.nodes, doc.defaults, {}, {});
  assert.equal(expanded[0].children[0].props.text, 'true-true-true');

  const badFiles = {
    'ui/Form.ui': `
      component Form {
        #[pattern('email')]
        prop email: String = 'invalid-email';
        Frame { Text { text: email; } }
      }
    `,
  };
  assert.throws(
    () => compileComponents(badFiles, 'ui/Form.ui', {}, metrics),
    /не соответствует шаблону/
  );
});

test('2. Component-scoped local state (state name: Type = default;)', () => {
  const doc = parse(`
    component Accordion {
      state expanded: Bool = false;
      state count: Int = 3;

      Frame {
        Text { text: expanded ? 'Открыто (\${count})' : 'Свёрнуто (\${count})'; }
      }
    }
  `);
  assert.deepEqual(doc.stateDefinitions.expanded, { type: 'Bool', default: false });
  assert.deepEqual(doc.stateDefinitions.count, { type: 'Int', default: 3 });

  const stateDefaults = { expanded: false, count: 3 };
  const closed = expandStructure(doc.nodes, doc.defaults, {}, { stateDefaults });
  assert.equal(closed[0].children[0].props.text, 'Свёрнуто (3)');

  const opened = expandStructure(doc.nodes, doc.defaults, { expanded: true, count: 7 }, { stateDefaults });
  assert.equal(opened[0].children[0].props.text, 'Открыто (7)');
});

test('3. Named style presets (style name { ... } and apply)', () => {
  const files = {
    'ui/theme.ui': `
      style cardBase {
        radius: 12;
        padding: 16;
      }
    `,
    'components/Card.ui': `
      #[use('../ui/theme.ui')]
      component Card {
        style accentCard {
          radius: 18;
        }
        Rectangle {
          background: #1e293b;
          Frame {
            apply cardBase, accentCard;
            radius: 20;
            Text { text: 'Styled'; }
          }
        }
      }
    `,
    'ui/Main.ui': `
      #[use('./theme.ui')]
      component Main {
        Frame {
          apply cardBase;
          Card {}
        }
      }
    `,
  };
  const compiled = compileComponents(files, 'ui/Main.ui', {}, metrics);
  const innerFrame = compiled.visualNodes.find(v => v.type === 'Frame' && v.props.radius === 20);
  assert.ok(innerFrame);
  assert.equal(innerFrame.props.padding, 16);
});

test('4. Collection transformation & aggregation (unique, find, compact, first, last, groupBy)', () => {
  const doc = parse(`
    component DataView {
      Frame {
        let uniqNums = [1, 2, 2, 3, 1] |> unique();
        let uniqUsers = state.users |> unique('role');
        let admin = state.users |> find('role', 'admin');
        let cleaned = ['a', '', null, 'b', false, 'c'] |> compact();
        let head = uniqNums |> first();
        let tail = uniqNums |> last();
        let groups = state.users |> groupBy('role');

        Text {
          text: '\${join(uniqNums, "-")}|u:\${count(uniqUsers)}|adm:\${admin.name}|c:\${join(cleaned, "")}|fl:\${head}-\${tail}|g:\${count(groups)}';
        }
      }
    }
  `);
  const state = {
    users: [
      { name: 'Alice', role: 'admin' },
      { name: 'Bob', role: 'editor' },
      { name: 'Charlie', role: 'admin' },
    ],
  };
  const expanded = expandStructure(doc.nodes, doc.defaults, state, {});
  assert.equal(expanded[0].children[0].props.text, '1-2-3|u:2|adm:Alice|c:abc|fl:1-3|g:2');
});

test('5. Markup-level match inside container children', () => {
  const doc = parse(`
    component StatusView {
      prop status: String = 'ok';
      prop count: Int = 5;

      Frame {
        match status {
          'loading' => Text { text: 'Загрузка...'; }
          'error' => {
            Text { text: 'Ошибка'; }
            Text { text: 'Повторите попытку'; }
          }
          'ok' if count > 0 => Text { text: 'Найдено: \${count}'; }
          _ => Text { text: 'Пусто'; }
        }
      }
    }
  `);
  const okNodes = expandStructure(doc.nodes, doc.defaults, {}, {});
  assert.equal(okNodes[0].children.length, 1);
  assert.equal(okNodes[0].children[0].props.text, 'Найдено: 5');

  const errNodes = expandStructure(doc.nodes, { status: 'error', count: 0 }, {}, {});
  assert.deepEqual(errNodes[0].children.map(c => c.props.text), ['Ошибка', 'Повторите попытку']);

  const emptyNodes = expandStructure(doc.nodes, { status: 'ok', count: 0 }, {}, {});
  assert.equal(emptyNodes[0].children[0].props.text, 'Пусто');
});

test('6. Localization & pluralization (i18n and plural)', () => {
  const doc = parse(`
    component Inbox {
      i18n ru {
        title: 'Входящие';
      }
      i18n en {
        title: 'Inbox';
      }
      prop count: Int = 21;

      Frame {
        Text {
          text: '\${i18n.title}: \${count} \${plural(count, "сообщение", "сообщения", "сообщений")} (\${plural(1, "item", "items")})';
        }
      }
    }
  `);
  const ruExpanded = expandStructure(doc.nodes, doc.defaults, {}, { i18n: doc.i18n, locale: 'ru' });
  assert.equal(ruExpanded[0].children[0].props.text, 'Входящие: 21 сообщение (item)');

  const enExpanded = expandStructure(doc.nodes, { count: 5 }, {}, { i18n: doc.i18n, locale: 'en' });
  assert.equal(enExpanded[0].children[0].props.text, 'Inbox: 5 сообщений (item)');

  const fewExpanded = expandStructure(doc.nodes, { count: 3 }, {}, { i18n: doc.i18n, locale: 'ru' });
  assert.equal(fewExpanded[0].children[0].props.text, 'Входящие: 3 сообщения (item)');
});

test('7. Extended Linter (unused-state, unused-style) & Formatter', () => {
  const src = `component Demo{state usedState:Bool=true;state unusedState:Int=0;style usedStyle{padding:8;}style unusedStyle{padding:16;}Frame{apply usedStyle;Text{text:usedState?'A':'B';}}}`;
  const formatted = formatSource(src);
  assert.ok(formatted.includes('state usedState: Bool = true;'));
  assert.ok(formatted.includes('style usedStyle {'));
  assert.ok(formatted.includes('apply usedStyle;'));

  const warnings = lintDocument(formatted);
  assert.ok(warnings.map(w => w.code).includes('unused-state'));
  assert.ok(warnings.map(w => w.code).includes('unused-style'));
  assert.ok(warnings.some(w => w.name === 'unusedState'));
  assert.ok(warnings.some(w => w.name === 'unusedStyle'));
  assert.ok(!warnings.some(w => w.name === 'usedState'));
  assert.ok(!warnings.some(w => w.name === 'usedStyle'));
});
