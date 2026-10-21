import {test} from 'node:test';
import assert from 'node:assert/strict';
import {writePreviewBinding, executePreviewStateAction, matchPreviewShortcut} from '../src/preview-state.js';
import {expandStructure} from '../src/component-semantics.js';
import {parse} from '../src/language.js';

test('preview bindings update nested state and the keyed row object after reordering', () => {
  const first={id:'a',name:'A'}, second={id:'b',name:'B'};
  const state={customer:{name:'N'},items:[first,second]};
  const doc=parse(`component Demo { Frame {
    for item in state.items key item.id { TextField { value <-> item.name; } }
  } }`);
  const rows=expandStructure(doc.nodes,{},state)[0].children;
  state.items.reverse();
  assert.equal(writePreviewBinding(state,rows[0],'item.name','Edited'),true);
  assert.equal(state.items[1].name,'Edited');
  assert.equal(state.items[0].name,'B');
  assert.equal(writePreviewBinding(state,rows[0],'item.name','Edited'),false);
  writePreviewBinding(state,{},'state.customer.name','Other');
  assert.equal(state.customer.name,'Other');
  assert.throws(()=>writePreviewBinding(state,{},'state.missing.field','X'),/Нет значения/);
});

test('executePreviewStateAction supports clear, toggle, increment, decrement, push, remove, and reset', () => {
  const state = { count: 2, active: false, items: ['a', 'b'], filter: 'abc', meta: { tag: 1 } };
  assert.equal(executePreviewStateAction(state, 'state.toggle', ['active']).handled, true);
  assert.equal(state.active, true);
  executePreviewStateAction(state, 'state.increment', ['count', 3]);
  assert.equal(state.count, 5);
  executePreviewStateAction(state, 'state.decrement', ['count', 2]);
  assert.equal(state.count, 3);
  executePreviewStateAction(state, 'state.push', ['items', 'c']);
  assert.deepEqual(state.items, ['a', 'b', 'c']);
  executePreviewStateAction(state, 'state.remove', ['items', 1]);
  assert.deepEqual(state.items, ['a', 'c']);
  executePreviewStateAction(state, 'state.clear', ['items']);
  assert.deepEqual(state.items, []);
  executePreviewStateAction(state, 'state.clear', ['filter']);
  assert.equal(state.filter, '');
  executePreviewStateAction(state, 'state.clear', ['meta']);
  assert.deepEqual(state.meta, {});
  executePreviewStateAction(state, 'state.reset', [], { count: 0, active: false });
  assert.deepEqual(state, { count: 0, active: false });
});

test('matchPreviewShortcut normalizes key and modifier aliases', () => {
  assert.equal(matchPreviewShortcut({ key: 'Escape', metaKey: false, ctrlKey: false, shiftKey: false, altKey: false }, 'Esc'), true);
  assert.equal(matchPreviewShortcut({ key: 'Enter', metaKey: true, ctrlKey: false, shiftKey: false, altKey: false }, 'Mod+Return'), true);
  assert.equal(matchPreviewShortcut({ key: ' ', metaKey: false, ctrlKey: true, shiftKey: false, altKey: false }, 'Ctrl+Space'), true);
  assert.equal(matchPreviewShortcut({ key: 'ArrowDown', metaKey: false, ctrlKey: false, shiftKey: false, altKey: true }, 'Opt+Down'), true);
  assert.equal(matchPreviewShortcut({ key: 'Delete', metaKey: false, ctrlKey: false, shiftKey: true, altKey: false }, 'Shift+Del'), true);
});

