import {test} from 'node:test';
import assert from 'node:assert/strict';
import {writePreviewBinding} from '../src/preview-state.js';
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
