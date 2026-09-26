import {test} from 'node:test';
import assert from 'node:assert/strict';
import {parse} from '../src/language.js';
import {setDesignData,designReferencesInFiles} from '../src/design-data.js';
import {propertyFields} from '../src/property-inspector.js';
import {pickerHex} from '../src/color-values.js';
import {valueTextEdit,statementTextEdit} from '../src/property-edit.js';

const source=`component SearchWindow {
    Frame {
        Text {
            color: #e8edf7;
            background: '#98a4ba';
            font.size: design.titleSize;
            text: 'Библиотека знаний'; /* подпись */
        }
        TextInput {
            value <-> state.query;
            placeholder: 'Что найти?';
        }
        Button {
            clicked -> actions.search('docs');
            disabled: state.loading;
            opacity: 0.8;
        }
    }
}`;
const nodeOf=type=>{let found;const walk=nodes=>{for(const n of nodes){found??=n.type===type?n:null;walk(n.children);}};walk(parse(source).nodes);return found;};
const byKey=fields=>Object.fromEntries(fields.map(f=>[f.key,f]));

test('a color field is recognised in both quoted and bare form',()=>{
  const fields=byKey(propertyFields(nodeOf('Text'),source));
  assert.equal(fields.color.kind,'color');
  assert.equal(fields.color.value,'#e8edf7');
  assert.equal(fields.color.css,'#e8edf7');
  assert.equal(fields.background.kind,'color');
  assert.equal(fields.background.value,'#98a4ba');
  // Only the six-digit form is losslessly editable by the native picker.
  assert.equal(pickerHex('#e8edf7'),'#e8edf7');
  assert.equal(pickerHex('  #ABC  '),'#aabbcc');
  assert.equal(pickerHex('#00000080'),null);
  assert.equal(pickerHex('argb(255, 1, 2, 3)'),null);
});
test('a design token field keeps its reference text and commits it as markup',()=>{
  setDesignData({'design.titleSize':'26'});
  const field=byKey(propertyFields(nodeOf('Text'),source))['font.size'];
  assert.equal(field.kind,'token');
  assert.equal(field.ref,'design.titleSize');
  assert.equal(field.text,'design.titleSize');
  assert.equal(field.commit,'value');
});
test('expressions and literals are told apart, and the value text round-trips',()=>{
  const text=byKey(propertyFields(nodeOf('Text'),source));
  assert.equal(text.text.kind,'string');
  assert.equal(text.text.value,'Библиотека знаний');
  const button=byKey(propertyFields(nodeOf('Button'),source));
  assert.equal(button.disabled.kind,'expression');
  assert.equal(button.disabled.text,'state.loading');
  assert.equal(button.opacity.kind,'number');
  assert.equal(button.opacity.value,0.8);
});
test('bindings and handlers are fields of their own, not values',()=>{
  // A preview node carries its bound name back as a computed prop the definition has no
  // range for; the statement row is the one that writes it, so the frozen twin is dropped.
  const bound=nodeOf('TextInput');
  const input=propertyFields({...bound,props:{...bound.props,value:'Что найти?'}},source);
  assert.deepEqual(input.filter(field=>field.key==='value').map(f=>f.kind),['binding']);
  const view=byKey(input);
  assert.equal(view.value.op,'<->');
  assert.equal(view.value.text,'state.query');
  assert.equal(view.value.commit,'statement');
  const button=byKey(propertyFields(nodeOf('Button'),source));
  assert.equal(button.clicked.kind,'handler');
  assert.equal(button.clicked.text,"actions.search('docs')");
});
test('a token catalogue lists references from markup files only',()=>{
  assert.deepEqual(designReferencesInFiles({'a.ui':'x: design.STATUS; y: design.a() ; z: design.STATUS;','b.design.ui':'w: design.other;','src/design.rs':'design::IGNORED'}),
    ['design.STATUS','design.a()','design.other']);
  // The design attribute and the .design.ui suffix name a file, so they are not tokens.
  assert.deepEqual(designReferencesInFiles({'ui/SearchWindow.ui':"#[design('./SearchWindow.design.ui')]\ncomponent SearchWindow {\n    Text { text: design.STATUS; }\n}"}),['design.STATUS']);
  assert.deepEqual(designReferencesInFiles({'a.ui':'x: design.text.title; y: design.call(1);'}),[]);
});
test('editing a value text replaces only that value and keeps comments',()=>{
  const edit=valueTextEdit(source,nodeOf('Text').start,'text',"'Новое название'");
  const next=source.slice(0,edit.from)+edit.insert+source.slice(edit.to);
  assert.ok(next.includes('/* подпись */'));
  assert.ok(next.includes('color: #e8edf7;'));
  assert.equal(parse(next).nodes[0].children[0].props.text,'Новое название');
  assert.equal(parse(next).nodes[0].children[0].props.color.expr,'#e8edf7');
});
test('a value must be one complete value, not a snippet that reopens the node',()=>{
  assert.throws(()=>valueTextEdit(source,nodeOf('Text').start,'text',"'x' } Button { text: 'y"),/не разобрано/);
  assert.throws(()=>valueTextEdit(source,nodeOf('Text').start,'text',''),/Пустое значение/);
  assert.throws(()=>valueTextEdit(source,nodeOf('Text').start,'missing','1'),/изменилось/);
});
test('a binding or handler rewrite replaces the whole statement',()=>{
  const binding=statementTextEdit(source,nodeOf('TextInput').start,'value','state.filters.query');
  const next=source.slice(0,binding.from)+binding.insert+source.slice(binding.to);
  assert.ok(next.includes('value <-> state.filters.query;'));
  assert.ok(!next.includes('value <-> state.query;'));
  const handler=statementTextEdit(source,nodeOf('Button').start,'clicked',"actions.search('all')");
  assert.equal(handler.insert,"clicked -> actions.search('all');");
});
test('binding paths and handlers are validated before the source changes',()=>{
  assert.throws(()=>statementTextEdit(source,nodeOf('TextInput').start,'value','state'),/путь к полю/);
  assert.throws(()=>statementTextEdit(source,nodeOf('Button').start,'clicked','backend.search()'),/actions/);
  assert.throws(()=>statementTextEdit(source,nodeOf('Text').start,'color','state.accent'),/Оператор/);
});
