import {test} from 'node:test';
import assert from 'node:assert/strict';
import {parse} from '../src/language.js';
import {propertyEdit} from '../src/property-edit.js';
import {EditorState} from '@codemirror/state';
import {history,undo,redo} from '@codemirror/commands';

const apply=(source,start,key,value)=>{const e=propertyEdit(source,start,key,value);return source.slice(0,e.from)+e.insert+source.slice(e.to);};
test('inspector replaces a whole quoted value, retaining comments and semicolons',()=>{
 const s="component Demo { Text { text: /* keep */ 'one;two\\'three'; // after\n color:#ffffff; } }";
 const next=apply(s,parse(s).nodes[0].start,'text',"new; 'text'\nline");
 assert.equal(parse(next).nodes[0].props.text,"new; 'text'\nline");assert.ok(next.includes('/* keep */'));assert.ok(next.includes('// after'));
});
test('inspector targets the parent property even when child property comes first',()=>{
 const s='component Demo { Frame { Text { width:20; } width:100; } }';
 const next=parse(apply(s,parse(s).nodes[0].start,'width',200));
 assert.equal(next.nodes[0].props.width,200);assert.equal(next.nodes[0].children[0].props.width,20);
});
test('inspector rejects missing and nonliteral targets without changing source',()=>{
 const s='component Demo { Text { text: \'ok\'; } }',start=parse(s).nodes[0].start;
 assert.throws(()=>propertyEdit(s,start,'missing',1));assert.throws(()=>propertyEdit(s,start,'text',{}));assert.throws(()=>propertyEdit(s,start,'text',NaN));
});
test('a property edit remains one reversible CodeMirror transaction',()=>{
 const source="component Demo { Text { text:'one;two'; } }";
 let state=EditorState.create({doc:source,extensions:[history()]});
 const change=propertyEdit(source,parse(source).nodes[0].start,'text','new');
 state=state.update({changes:change,userEvent:'input.inspector'}).state;
 const changed=state.doc.toString(),dispatch=tr=>{state=tr.state;};
 assert.equal(undo({state,dispatch}),true);assert.equal(state.doc.toString(),source);
 assert.equal(redo({state,dispatch}),true);assert.equal(state.doc.toString(),changed);
});
test('escaped backslash n is not decoded twice into a newline',()=>{
 const source="component Demo { Text { text:'old'; } }";
 const value=String.raw`literal\npath\name`;
 assert.equal(parse(apply(source,parse(source).nodes[0].start,'text',value)).nodes[0].props.text,value);
});
