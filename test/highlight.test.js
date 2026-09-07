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
test('highlight grammar covers contracts, expressions, grouped variants and keyed children',()=>{
 assert.deepEqual(errors(`component Notice {
   enum Tone { Accent, Danger, }
   required prop title: String;
   prop compact: Bool = false;
   prop subtitle: String?;
   prop tone: Tone = Tone.Accent;
   event selected(id: String, index: Int);
   match props.tone { Tone.Danger => { color: #ff0000; } _ => { color: #ffffff; } }
   Column {
     if state.loading { Text { text: 'Loading'; } } else if state.failed { Text { text: 'Error'; } } else { Text {} }
     for item in state.items key item.id {
       Button { forward props { color, fontSize }; text: 'Found: \${item.count + 1}'; disabled: state.busy || !state.valid; clicked -> events.selected(item.id, 1 + 2); }
     } empty { Text { text: 'Nothing'; } }
     Text { text: state.items?.[0]?.name ?? 'None'; width: 10 + 2 * 3; }
   }
 }`),[]);
 assert.deepEqual(errors(`component Demo { Text { text: 'Hello \${state.name ?? 'guest'}'; } }`),[]);
 assert.deepEqual(errors(`component Demo { Text { text: "Hello \${state.name ?? "guest"}"; } }`),[]);
});
test('the complete Studio highlight extension imports and installs in an editor state',async()=>{
 const {formaHighlight}=await import('../src/forma-highlight.js');
 const {EditorState}=await import('@codemirror/state');
 const {syntaxTree}=await import('@codemirror/language');
 const state=EditorState.create({doc:'component Demo { Text { width: 1 + 2 * 3 / 2; disabled: !state.ready; } }',extensions:formaHighlight});
 assert.equal(syntaxTree(state).type.name,'Document');
 assert.ok(syntaxTree(state).toString().includes('BinaryExpression'));
});
