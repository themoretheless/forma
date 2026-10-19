import test from 'node:test';
import assert from 'node:assert/strict';
import { parse, formatSource, lintDocument } from '../src/language.js';
import { compileComponents } from '../src/components.js';

const metrics = {
  measureText: (t, s) => [String(t ?? '').length * s * 0.5, s * 1.2],
};

test('1. Collection filtering (for ... where) and projection functions (sortBy, pluck, count, any, all)', () => {
  const source = `
component UsersView {
  let sorted = sortBy(state.users, 'score');
  let names = pluck(sorted, 'name');
  let totalCount = count(names);
  let hasAdmin = any(pluck(state.users, 'admin'));
  let allActive = all(pluck(state.users, 'active'));

  Column {
    width: 360; height: 220;
    Text { key: 'summary'; text: '\${totalCount}:\${join(names, ",")}:\${hasAdmin}:\${allActive}'; }
    for u, i in sorted where u.active key u.id {
      Text { key: 'u_\${u.id}'; text: '\${i}:\${u.name}'; }
    } empty {
      Text { key: 'empty'; text: 'Никого нет'; }
    }
  }
}
`;
  const state = {
    users: [
      { id: 'b', name: 'Борис', score: 20, active: true, admin: false },
      { id: 'a', name: 'Анна', score: 10, active: false, admin: true },
      { id: 'v', name: 'Вера', score: 30, active: true, admin: false },
    ],
  };

  const compiled = compileComponents({ 'main.ui': source }, 'main.ui', state, metrics);
  const texts = compiled.previewNodes[0].children.filter(v => v.type === 'Text').map(v => v.props.text);
  assert.deepEqual(texts, [
    '3:Анна,Борис,Вера:true:false',
    '1:Борис',
    '2:Вера',
  ]);

  // When all items are filtered out by `where`, `empty` block should render
  const emptyState = {
    users: [{ id: 'a', name: 'Анна', score: 10, active: false, admin: false }],
  };
  const compiledEmpty = compileComponents({ 'main.ui': source }, 'main.ui', emptyState, metrics);
  const emptyTexts = compiledEmpty.previewNodes[0].children.filter(v => v.type === 'Text').map(v => v.props.text);
  assert.deepEqual(emptyTexts, ['1:Анна:false:false', 'Никого нет']);
});

test('2. Local let bindings inside for, if, and container blocks', () => {
  const source = `
component OrderList {
  Column {
    width: 360; height: 220;
    let baseTax = 0.1;
    for item in state.items key item.id {
      let subtotal = item.price * item.qty;
      let total = subtotal * (1 + baseTax);
      if total >= 100 {
        let label = upper(item.title) + ': ' + formatNumber(total, 0);
        Text { key: item.id; text: label; }
      }
    }
  }
}
`;
  const state = {
    items: [
      { id: '1', title: 'Книга', price: 30, qty: 2 },
      { id: '2', title: 'Монитор', price: 100, qty: 2 },
    ],
  };
  const compiled = compileComponents({ 'main.ui': source }, 'main.ui', state, metrics);
  const texts = compiled.previewNodes[0].children.filter(v => v.type === 'Text').map(v => v.props.text);
  assert.deepEqual(texts, ['МОНИТОР: 220']);
});

test('3. Shared imports via #[use(...)] and use declarations for tokens, enums, and structs', () => {
  const themeUi = `
tokens color {
  brand: #3366ff;
  surface: #1e293b;
}
enum Status { Idle, Active }
struct BadgeInfo {
  label: String,
  status: Status,
}
`;
  const mainUi = `
#[use('./theme.ui')]
component App {
  prop info: BadgeInfo = { label: 'Готово'; status: Status.Active; };
  Column {
    width: 360; height: 220;
    Text {
      width: 120; height: 40;
      background: match info.status {
        Status.Active => tokens.color.brand;
        _ => tokens.color.surface;
      };
      text: info.label;
    }
  }
}
`;
  const compiled = compileComponents(
    { 'theme.ui': themeUi, 'main.ui': mainUi },
    'main.ui',
    {},
    metrics,
  );
  const textNode = compiled.previewNodes[0].children[0];
  assert.equal(textNode.props.background.expr, '#3366ff');
});

test('4. Extended match patterns: OR-patterns (|), range patterns (.., ..=), and pattern guards (if)', () => {
  const source = `
component GradeBadge {
  prop score: Number = 85;
  prop bonus: Bool = true;
  prop role: String = 'editor';

  let grade = match score {
    90..=100 => 'A';
    80..90 if bonus => 'A-';
    80..90 => 'B';
    0..80 => 'C';
    _ => 'N/A';
  };

  let access = match role {
    'admin' | 'owner' | 'editor' => 'full';
    'viewer' | 'guest' => 'read';
    _ => 'none';
  };

  Column {
    width: 360; height: 220;
    Text { text: '\${grade}:\${access}'; }
  }
}
`;
  const compiled = compileComponents({ 'main.ui': source }, 'main.ui', {}, metrics);
  const text = compiled.previewNodes[0].children[0];
  assert.equal(text.props.text, 'A-:full');

  const compiledNoBonus = compileComponents(
    { 'main.ui': source },
    'main.ui',
    {},
    { ...metrics, properties: { bonus: false, role: 'guest' } },
  );
  assert.equal(compiledNoBonus.previewNodes[0].children[0].props.text, 'B:read');
});

test('5. Color manipulation functions in stdlib (alpha, mix, lighten, darken)', () => {
  const source = `
component ColorDemo {
  tokens {
    primary: #3366ff;
    accent: #ff3366;
  }
  Column {
    width: 360; height: 220;
    Text {
      key: 'r1'; width: 40; height: 20;
      background: alpha(tokens.primary, 0.5);
    }
    Text {
      key: 'r2'; width: 40; height: 20;
      background: lighten(#000000, 0.5);
    }
    Text {
      key: 'r3'; width: 40; height: 20;
      background: darken(#ffffff, 0.5);
    }
    Text {
      key: 'r4'; width: 40; height: 20;
      background: mix(#ff0000, #0000ff, 0.5);
    }
  }
}
`;
  const compiled = compileComponents({ 'main.ui': source }, 'main.ui', {}, metrics);
  const rects = compiled.previewNodes[0].children.map(v => v.props.background.expr);
  assert.deepEqual(rects, ['#3366ff80', '#808080', '#808080', '#800080']);
});

test('6. Responsive viewport context (viewport.width, viewport.height, viewport.compact)', () => {
  const source = `
component Responsive {
  Column {
    width: 360; height: 220;
    if viewport.compact {
      Text { text: 'Compact \${viewport.width}x\${viewport.height}'; }
    } else {
      Text { text: 'Wide \${viewport.width}x\${viewport.height}'; }
    }
  }
}
`;
  const compiledCompact = compileComponents({ 'main.ui': source }, 'main.ui', {}, metrics);
  assert.equal(compiledCompact.previewNodes[0].children[0].props.text, 'Compact 360x220');

  const wideSource = source.replace('width: 360;', 'width: 800;');
  const compiledWide = compileComponents({ 'main.ui': wideSource }, 'main.ui', {}, metrics);
  assert.equal(compiledWide.previewNodes[0].children[0].props.text, 'Wide 800x220');
});

test('7. Scoped slots with parameters: slot name(param: expr) and @name(localParam)', () => {
  const cardUi = `
component UserCard {
  prop userName: String = 'Алиса';
  prop role: String = 'Инженер';
  width: 240; height: 80;
  Rectangle {
    background: #1e293b;
    slot header(title: upper(userName), badge: role) {
      Text { text: title; }
    }
  }
}
`;
  const mainUi = `
component Main {
  Column {
    width: 360; height: 220;
    UserCard {
      userName: 'Дмитрий';
      role: 'Архитектор';
      @header(uTitle, uBadge) {
        Text { text: '\${uTitle} [\${uBadge}]'; }
      }
    }
  }
}
`;
  const compiled = compileComponents(
    { 'components/UserCard.ui': cardUi, 'main.ui': mainUi },
    'main.ui',
    {},
    metrics,
  );
  const text = compiled.visualNodes.find(v => v.type === 'Text');
  assert.equal(text.props.text, 'ДМИТРИЙ [Архитектор]');
});

test('8. Canonical formatter (formatSource) and static linter (lintDocument)', () => {
  const messy = `component   Demo{prop unusedProp:String='hi';let unusedLet=1+2;let usedLet=10;shortcut 'CmdOrCtrl+S' -> actions.save();shortcut 'CmdOrCtrl+S' -> actions.saveAgain();Column{width:360;height:220;Text{text:match usedLet { _ => 'first'; 10 => 'unreachable'; };}}}`;
  const formatted = formatSource(messy);
  assert.ok(formatted.includes('component Demo {\n'));
  assert.ok(formatted.includes('  prop unusedProp: String = \'hi\';'));
  // Formatted code parses identically
  const parsed = parse(formatted);
  assert.equal(parsed.name, 'Demo');

  const warnings = lintDocument(messy);
  const codes = warnings.map(w => w.code);
  assert.ok(codes.includes('unused-prop'), 'Should detect unused-prop');
  assert.ok(codes.includes('unused-let'), 'Should detect unused-let');
  assert.ok(codes.includes('duplicate-shortcut'), 'Should detect duplicate-shortcut');
  assert.ok(codes.includes('unreachable-branch'), 'Should detect unreachable-branch');
});
