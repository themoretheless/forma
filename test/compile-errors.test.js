// Pins the message the component compiler gives for each kind of invalid project.
//
// The compiler has some ninety error paths and most were never exercised by a test, so a refactor
// could change or lose a diagnostic unnoticed. Each case is a minimal project that fails in one
// way; the messages are compared with a recorded file. Run with FORMA_BLESS=1 to record an
// intended change.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync, writeFileSync } from 'node:fs';
import { compileComponents } from '../src/components.js';

const metrics = { measureText: () => [80, 20] };
const demo = body => ({ 'ui/Demo.ui': `component Demo { ${body} }` });
const button = `component Button { width: 200; height: 50; text: 'Base';
 Rectangle { Frame { ContentPresenter { key: 'content'; Text { text: props.text; } } } PointerArea { clicked -> events.clicked(); } } }`;
/** The same button with a keyed outline, for overrides that address it. */
const keyedButton = button.replace('Rectangle {', "Rectangle { key: 'outline';");
const withButton = (body, extra = {}) => ({
  'components/Button.ui': button,
  ...demo(body),
  ...extra,
});
const chain = length =>
  Object.fromEntries(
    Array.from({ length }, (_, i) => [
      `components/C${i}.ui`,
      i ? `component C${i} : C${i - 1} { }` : `component C0 { Frame { Text { text: 'a'; } } }`,
    ]),
  );

/** A form that declares `events` and raises `handler` from a button. */
const withHandler = (events, handler) => ({
  'components/Button.ui': button,
  'ui/Demo.ui': `component Demo { ${events} Frame { Button { clicked -> ${handler}; } } }`,
});

/** A control whose content, the part drawn inside it, is `body`. */
const card = body => ({
  'components/Card.ui': `component Card { width: 200; height: 50; Rectangle { ${body} PointerArea { clicked -> events.clicked(); } } }`,
  ...demo(`Frame { Card { clicked -> actions.go(); } }`),
});

/** Each project is compiled from `ui/Demo.ui`; `metrics: null` compiles without text metrics. */
const cases = {
  'cell together with row': demo(
    `Frame { rows: [10, 10]; columns: [10, 10]; Text { cell: 1 1; row: 1; text: 'a'; } }`,
  ),
  'cell outside the grid': demo(
    `Frame { rows: [10]; columns: [10]; Text { cell: 3 1; text: 'a'; } }`,
  ),
  'unknown text property': demo(`Frame { Text { text: 'a'; fontFamily: 'serif'; } }`),
  'wrap is not a boolean': demo(`Frame { Text { text: 'a'; wrap: 3; } }`),
  'unknown component': demo(`Frame { Video { } }`),
  'negative star weight': demo(`Frame { columns: [-1*]; Text { text: 'a'; } }`),
  'unsupported track size': demo(`Frame { columns: ['wide']; Text { text: 'a'; } }`),
  'children without a presenter': demo(`Frame { Text { text: 'a'; Text { text: 'b'; } } }`),
  'old import syntax': {
    'ui/Demo.ui': `import 'ui/None.ui'; component Demo { Frame { Text { text: 'a'; } } }`,
  },
  'unknown property of a component': withButton(`Frame { Button { nope: 1; } }`),
  'two root elements': demo(`Frame { Text { text: 'a'; } } Frame { Text { text: 'b'; } }`),
  'presenter without a key': {
    'components/Box.ui': `component Box { Frame { ContentPresenter { } } }`,
    ...demo(`Frame { Box { } }`),
  },
  'two presenters with one key': {
    'components/Box.ui': `component Box { Frame { ContentPresenter { key: 'a'; } ContentPresenter { key: 'a'; } } }`,
    ...demo(`Frame { Box { } }`),
  },
  'composition cycle': {
    'components/A.ui': `component A { Frame { B { } } }`,
    'components/B.ui': `component B { Frame { A { } } }`,
    ...demo(`Frame { A { } }`),
  },
  'missing state value': demo(
    `Frame { Text { text: 'a'; } } Modal { key: 'm'; open: state.open; Frame { Text { text: 'b'; } } }`,
  ),
  'empty key': demo(
    `Frame { Text { text: 'a'; } Modal { key: ''; Frame { Text { text: 'b'; } } } }`,
  ),
  'content given to a component without a presenter': {
    'components/Plain.ui': `component Plain { Frame { Text { text: 'p'; } } }`,
    ...demo(`Frame { Plain { Text { text: 'x'; } } }`),
  },
  'design statement without a block': {
    'ui/Demo.ui': `design 'ui/Demo.design'; component Demo { Frame { Text { text: 'a'; } } }`,
  },
  'enum declared twice with different members': {
    'components/A.ui': `enum Kind { one, two } component A { Frame { Text { text: 'a'; } } }`,
    'ui/Demo.ui': `enum Kind { three } component Demo { Frame { A { } } }`,
  },
  'forward at component level': {
    'ui/Demo.ui': `component Demo { forward clicked; Frame { Text { text: 'a'; } } }`,
  },
  'override that is not an element': {
    'components/Button.ui': button,
    'components/B2.ui': `component B2 : Button { override content: 5; }`,
    ...demo(`Frame { B2 { } }`),
  },
  'inheritance deeper than the limit': {
    ...chain(34),
    ...demo(`Frame { C33 { } }`),
  },
  'file declares another component': {
    'components/Button.ui': button.replace('component Button', 'component Other'),
    ...demo(`Frame { Button { } }`),
  },
  'scroll next to other controls': demo(
    `Frame { Scroll { Text { text: 'a'; } } Text { text: 'b'; } }`,
  ),
  'content padding of five sizes': card(`Frame { padding: 1 2 3 4 5; Text { text: 'a'; } }`),
  'content size above the limit': card(`Frame { Text { text: 'a'; fontSize: 5000; } }`),
  'content gap of three sizes': card(`Frame { gap: 1 2 3; Text { text: 'a'; } }`),
  'content font family': card(`Frame { Text { text: 'a'; fontFamily: 'serif'; } }`),
  'wrapped text without metrics': {
    metrics: null,
    ...card(`Frame { Text { text: 'a'; wrap: true; } }`),
  },
  'multiline text without metrics': { metrics: null, ...card(`Frame { Text { text: 'a\\nb'; } }`) },
  'content property not supported': card(`Frame { Text { text: 'a'; opacity: 1; } }`),
  'interactive element in content': card(`Frame { Text { text: 'a'; clicked -> actions.x(); } }`),
  'content brush background': card(
    `Rectangle { background: Brush { color: #123456; }; Text { text: 'a'; } }`,
  ),
  'content clip is not a boolean': card(`Row { clip: 3; Text { text: 'a'; } }`),
  'content text with children': card(`Frame { Text { text: 'a'; Text { text: 'b'; } } }`),
  'content image missing from the project': card(
    `Frame { Image { source: 'assets/none.svg'; width: 10; height: 10; } }`,
  ),
  'zero star weight': card(`Frame { columns: [0*]; Text { text: 'a'; } }`),
  'content wrap is not a boolean': card(`Frame { Text { text: 'a'; wrap: 3; } }`),
  'content cell outside the grid': card(
    `Frame { rows: [10]; columns: [10]; Text { cell: 1 4; text: 'a'; } }`,
  ),
  'modal handler other than dismissed': demo(
    `Frame { Text { text: 'a'; } Modal { key: 'm'; clicked -> actions.x(); Frame { Text { text: 'b'; } } } }`,
  ),
  'modal dismissed with arguments': demo(
    `Frame { Text { text: 'a'; } Modal { key: 'm'; dismissed -> actions.x(1); Frame { Text { text: 'b'; } } } }`,
  ),
  'clip on a nested container': demo(`Frame { Frame { clip: true; Text { text: 'a'; } } }`),
  'presenter with another property': {
    'components/Box.ui': `component Box { Frame { ContentPresenter { key: 'content'; width: 5; } } }`,
    ...demo(`Frame { Box { } }`),
  },
  'content property and children together': {
    'components/Box.ui': `component Box { Frame { ContentPresenter { key: 'content'; } } }`,
    ...demo(`Frame { Box { content: Text { text: 'a'; }; Text { text: 'b'; } } }`),
  },
  'override from an absolute path': {
    'components/Button.ui': keyedButton,
    'components/B2.ui': `component B2 : Button { override 'outline' from '/abs/Accent.ui' { width: 2; } }`,
    ...demo(`Frame { B2 { } }`),
  },
  'override from outside the project': {
    'components/Button.ui': keyedButton,
    'components/B2.ui': `component B2 : Button { override 'outline' from '../../Accent.ui' { width: 2; } }`,
    ...demo(`Frame { B2 { } }`),
  },
  'override from without a base component': {
    'components/B2.ui': `component B2 { override 'outline' from '../styles/Accent.ui' { width: 2; } Frame { Text { text: 'a'; } } }`,
    ...demo(`Frame { B2 { } }`),
  },
  'override of a key the base does not have': {
    'components/Button.ui': keyedButton,
    'components/B2.ui': `component B2 : Button { override 'nokey' from '../styles/Accent.ui' { width: 2; } }`,
    'styles/Accent.ui': `component Accent { Rectangle { } }`,
    ...demo(`Frame { B2 { } }`),
  },
  'forward in a component declaration': {
    'components/Lab.ui': `component Lab { forward props { text }; Frame { Text { text: 'a'; } } }`,
    ...demo(`Frame { Lab { } }`),
  },
  'fontSize together with font.size': {
    'components/Button.ui': button.replace("text: 'Base';", "text: 'Base'; fontSize: 12;"),
    ...demo(`Frame { Button { fontSize: 1; font.size: 2; } }`),
  },
  'design file missing': {
    'ui/Demo.ui': `#[design('./Demo.design.ui')]\ncomponent Demo { Frame { Text { key: 't'; text: 'a'; } } }`,
  },
  'design attribute on a component': {
    'components/Lab.ui': `#[design('./Lab.design.ui')]\ncomponent Lab { Frame { Text { text: 'a'; } } }`,
    ...demo(`Frame { Lab { } }`),
  },
  'one design key in two design files': {
    'ui/Demo.ui': `#[design('./A.design.ui')]\n#[design('./B.design.ui')]\ncomponent Demo { Frame { Text { key: 't'; text: 'a'; } } }`,
    'ui/A.design.ui': `design Demo { Text { key: 't'; text: 'x'; } }`,
    'ui/B.design.ui': `design Demo { Text { key: 't'; text: 'y'; } }`,
  },
  'override of a slot the component does not have': {
    'components/Lab.ui': `component Lab { override content: Text { text: 'a'; }; Frame { Text { text: 'a'; } } }`,
    ...demo(`Frame { Lab { } }`),
  },
  'handler on a plain element of a component': {
    'components/Lab.ui': `component Lab { Frame { Text { text: 'a'; clicked -> actions.x(); } } }`,
    ...demo(`Frame { Lab { } }`),
  },
  'key that is not a string': demo(
    `Frame { Text { text: 'a'; } Modal { key: 5; Frame { Text { text: 'b'; } } } }`,
  ),
  'event called with too few arguments': withHandler('event open(id: String);', 'events.open()'),
  'event that is not declared': withHandler('event open(id: String);', 'events.other()'),
  'event arguments without a declaration': withHandler('', `events.pressed('x')`),
  'event argument of another type': withHandler('event open(id: Int);', `events.open('x')`),
  'event argument of an unknown type': withHandler('event open(id: Blob);', `events.open('x')`),
  'override of properties on content that is not an element': {
    'components/Button.ui': button.replace(
      "ContentPresenter { key: 'content'; Text { text: props.text; } }",
      "ContentPresenter { key: 'content'; content: props.text; }",
    ),
    'components/B2.ui': `component B2 : Button { override content { color: #ffffff; } }`,
    ...demo(`Frame { B2 { } }`),
  },
  'content that cannot be drawn': {
    'components/Button.ui': `component Button { Rectangle { Frame { PointerArea { } } } }`,
    ...demo(`Frame { Button { } }`),
  },
  'content that cannot be measured': {
    'components/Button.ui': `component Button { Rectangle { Row { PointerArea { } } } }`,
    ...demo(`Frame { Button { } }`),
  },
};

function messageFor(project) {
  const { metrics: own, ...files } = project;
  try {
    compileComponents(files, 'ui/Demo.ui', {}, own === null ? {} : metrics);
    return '(compiles)';
  } catch (error) {
    return String(error.message).split('\n')[0];
  }
}

test('every invalid project fails with its recorded message', () => {
  const actual =
    Object.entries(cases)
      .map(([name, project]) => `${name}\n    ${messageFor(project)}`)
      .join('\n') + '\n';
  const golden = new URL('./golden/compile-errors.txt', import.meta.url);
  if (process.env.FORMA_BLESS) writeFileSync(golden, actual);
  assert.equal(actual, readFileSync(golden, 'utf8'));
  assert.ok(!actual.includes('(compiles)'), 'every case must be rejected');
});
