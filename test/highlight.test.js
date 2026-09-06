import {test} from 'node:test';
import assert from 'node:assert/strict';
import {parser} from '../src/forma-parser.js';
function errors(source){const out=[];parser.parse(source).iterate({enter:n=>{if(n.type.isError)out.push(n.from);}});return out;}
test('highlight grammar understands layout, bindings, design data and comments',()=>{
 assert.deepEqual(errors(`#[design('./Demo.design.ui')]
 component Demo {
   Frame { columns: [-, *, 2*]; rows: [48, -]; gap: 16 10;
     // Keep } inside comments
     TextInput { value <-> state.query; placeholder: 'Find'; }
     Button { cell: 2 2; disabled: !state.loading; clicked -> actions.search(); }
   }
 }`),[]);
 assert.deepEqual(errors("design Demo { TextInput { key: 'query'; value: 'Hello'; } }"),[]);
});
test('highlight grammar recognizes proposed match and constraint syntax',()=>{
 assert.deepEqual(errors(`component Demo { Frame {
 columns: match (state.compact, viewport.width) {
   (true, _) => [*];
   (false, < 600) => [c(120, *, 500), *];
   _ => [200, *];
 };
 } }`),[]);
});
test('incomplete source recovers and retains recognized tokens',()=>{
 const tree=parser.parse("component Demo { Text { text: 'Hello'; padding:");
 assert.ok(tree.toString().includes('String'));assert.ok(tree.toString().includes('⚠'));
});
