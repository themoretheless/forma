import {test} from 'node:test';
import assert from 'node:assert/strict';
import {parse,resolve} from '../src/language.js';
test('component exposes defaults separately from primitives',()=>{const p=parse("component Button { width: 100; text: 'Кнопка'; fontSize: 16; Rectangle { radius: props.radius; } }");assert.deepEqual(p.defaults,{width:100,text:'Кнопка',fontSize:16});assert.equal(p.nodes.length,1);assert.equal(p.nodes[0].type,'Rectangle');});
test('design attribute, bindings and actions',()=>{const p=parse("#[design('./demo.design.ui')] component Demo { TextInput { value <-> state.query; } Button { clicked -> actions.search(); disabled: !state.loading; } }");assert.deepEqual(p.designs,['./demo.design.ui']);assert.equal(p.nodes[0].bindings.value,'state.query');assert.equal(p.nodes[1].events.clicked,'actions.search');assert.equal(resolve(p.nodes[1].props.disabled,{loading:true}),false);});
test('legacy preview is rejected',()=>assert.throws(()=>parse("preview 'Data' { state: { loading: false; }; }"),/больше не поддерживается/));
test('broken input fails instead of silently dropping source',()=>{assert.throws(()=>parse("component Demo { Text { text: 'Hi'; }"));assert.throws(()=>parse('component Demo {} ???'));assert.throws(()=>parse("component Demo { Text { text: 'a'; text: 'b'; } }"));});
