import {test} from 'node:test';
import assert from 'node:assert/strict';
import {parse,resolve} from '../src/language.js';
import {designReferences,setDesignData} from '../src/design-data.js';
test('static constants and methods use cached design values',()=>{
  const d=parse("design A { Text { key: 'a'; text: design.STATUS; } TextInput { key: 'b'; value: design.example_query(); } }");
  assert.deepEqual(designReferences(d),['design.STATUS','design.example_query()']);
  assert.throws(()=>resolve(d.overrides.a.text,{}),/Нет дизайн-данных/);
  setDesignData({'design.STATUS':'Ready','design.example_query()':'Hello'});
  assert.equal(resolve(d.overrides.a.text,{}),'Ready');
  assert.equal(resolve(d.overrides.b.value,{}),'Hello');
});
