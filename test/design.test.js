import {test} from 'node:test';
import assert from 'node:assert/strict';
import {parse,validateDesign} from '../src/language.js';

test('design overrides properties by key without changing source bindings',()=>{
  const c=parse("component SearchWindow { Frame { key: 'root'; Text { key: 'status'; text: state.status; } TextInput { key: 'query'; value <-> state.query; } } }");
  const d=parse("design SearchWindow { Text { key: 'status'; text: 'Demo'; color: #8899aa; } TextInput { key: 'query'; value: 'Example'; } }");
  validateDesign(c,d);
  assert.equal(d.overrides.status.text,'Demo');
  assert.equal(c.nodes[0].children[0].props.text.expr,'state.status');
  assert.equal(c.nodes[0].children[1].bindings.value,'state.query');
});
test('design keys are validated',()=>{
  assert.throws(()=>parse("design A { 'x' { text: 'a'; } }"),/явным key/);
  assert.throws(()=>parse("design A { Text { text: 'a'; } }"),/непустой строкой/);
  assert.throws(()=>validateDesign(parse("component A { Text { key: 'x'; } }"),parse("design A { Button { key: 'x'; text: 'a'; } }")),/Тип дизайн-key/);
  assert.throws(()=>parse("component A { Text { key: 'x'; } Text { key: 'x'; } }"),/Повторный key/);
  const c=parse('component A { Text {} }');
  assert.throws(()=>validateDesign(c,parse("design A { Text { key: 'missing'; text: 'x'; } }")),/Неизвестный/);
  assert.throws(()=>parse("design A { Text { key: 'x'; text: 'a'; } Text { key: 'x'; text: 'b'; } }"),/Повторный/);
  assert.throws(()=>parse("design A { Text { key: 'x'; clicked -> actions.search(); } }"),/только свойства/);
});
