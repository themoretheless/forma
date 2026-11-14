// A radio group: `value: 'a'; group <-> state.choice;` on each choice.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync, readdirSync } from 'node:fs';
import { compileComponents } from '../src/components.js';
import { parse } from '../src/language.js';
import { materializeTheme } from '../crates/forma/controls/tokens.js';

function library() {
  const files = {};
  for (const directory of ['components', 'assets']) {
    const url = new URL(`../crates/forma/controls/${directory}/`, import.meta.url);
    for (const name of readdirSync(url))
      files[`${directory}/${name}`] = readFileSync(new URL(name, url), 'utf8');
  }
  const components = Object.fromEntries(Object.entries(files).filter(([k]) => k.endsWith('.ui')));
  return { ...files, ...materializeTheme(components, 'light') };
}

const GROUP = `component Demo { state choice: String = 'b'; Frame {
  RadioButton { key: 'a'; value: 'a'; group <-> state.choice; }
  RadioButton { key: 'b'; value: 'b'; group <-> state.choice; }
  RadioButton { key: 'c'; value: 'c'; group <-> state.choice; } } }`;

const checked = (choice, source = GROUP) =>
  compileComponents(
    { ...library(), 'ui/Demo.ui': source },
    'ui/Demo.ui',
    { choice },
    {
      measureText: () => [80, 20],
    },
  ).previewControls.map(control => [control.props.key, control.props.checked]);

test('the choice the group holds is the only checked one', () => {
  assert.deepEqual(checked('b'), [
    ['a', false],
    ['b', true],
    ['c', false],
  ]);
  assert.deepEqual(checked('c'), [
    ['a', false],
    ['b', false],
    ['c', true],
  ]);
  assert.deepEqual(
    checked('z').map(([, on]) => on),
    [false, false, false],
    'a value no choice has',
  );
});

test('parsing moves the value of a choice aside and derives checked from the group', () => {
  const choice = parse(GROUP).nodes[0].children[0];
  assert.equal(choice.props.value, undefined, 'value is not a property of the control');
  assert.equal(choice.groupValue, 'a');
  assert.deepEqual(choice.bindings, { group: 'state.choice' });
  assert.deepEqual(choice.props.checked, {
    expression: { kind: 'binary', operator: '==', left: { expr: 'state.choice' }, right: 'a' },
  });
});

test('a numeric value compares as a number', () => {
  const source = `component Demo { state choice: Number = 2; Frame {
    RadioButton { key: 's'; value: 1; group <-> state.choice; }
    RadioButton { key: 'm'; value: 2; group <-> state.choice; } } }`;
  assert.deepEqual(checked(2, source), [
    ['s', false],
    ['m', true],
  ]);
});
