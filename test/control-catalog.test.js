import {test} from 'node:test';
import assert from 'node:assert/strict';
import {parse} from '../src/language.js';
import {insertElement} from '../src/element-edit.js';
import {insertableControls} from '../src/control-catalog.js';

const button="component Button {\n    text: 'Кнопка';\n    Rectangle { }\n}";
const files={
  'components/Button.ui':button,
  // An internal part is written like a control but has no page-level root shape.
  'components/Icon.ui':'component Icon {\n    Text { }\n}',
  'assets/search.svg':'<svg/>',
  'ui/Page.ui':'component Page { Frame { Button { } } }',
};
const page=markup=>parse(`component Palette { Frame { ${markup} } }`).nodes[0].children;

test('the palette lists exactly the types a page can place',()=>{
  const items=insertableControls(files);
  // Project components plus the built-in fallbacks; bare primitives have no definition.
  assert.deepEqual(items.map(item=>item.type),['Button','Text','TextInput']);
  for(const item of items){
    assert.equal(item.markup.includes('\n'),false,'a palette entry is one line');
    assert.equal(page(item.markup)[0]?.type,item.type,`${item.markup} is not a ${item.type}`);
  }
});
test('only an invisible default label is replaced with a start text',()=>{
  const [text]=insertableControls({}).filter(item=>item.type==='Text');
  assert.deepEqual(page(text.markup)[0].props,{text:'Текст'});
  // A control that already shows something keeps its own defaults untouched.
  assert.deepEqual(page(insertableControls(files).find(item=>item.type==='Button').markup)[0].props,{});
});
test('a project whose components do not link offers no palette',()=>{
  // The compiler resolves components/<Name>.ui as component Name, so one misnamed file
  // breaks linking for the whole project instead of only hiding that one entry.
  assert.deepEqual(insertableControls({'components/Misnamed.ui':button,'ui/Page.ui':'component Page { Frame {} }'}),[]);
  assert.deepEqual(insertableControls({'ui/Page.ui':'component Page { Frame {} }','components/Button.ui':'component {'}),[]);
});
test('the designer splices every palette entry into a live page',()=>{
  const source='component Page {\n  Row {\n    Button { }\n  }\n}';
  const row=parse(source).nodes[0],control=row.children[0];
  for(const item of insertableControls(files)){
    // The palette hands the primitive the same two slots the canvas add-button resolves:
    // next to the selected control, or inside the selected container.
    for(const [side,start]of [['after',control.start],['inside',row.start]]){
      const change=insertElement(source,start,item.markup,false,side);
      const next=source.slice(0,change.from)+change.insert+source.slice(change.to);
      assert.equal(parse(next).nodes[0].children.length,2,`${item.markup} (${side}) lands nowhere`);
      assert.equal(next.slice(change.start,change.start+item.type.length),item.type,`${item.markup} (${side}) reports the wrong offset`);
    }
  }
});
test('the splice primitive itself refuses a container',()=>{
  // The palette omits containers for exactly this reason, so the two rules cannot drift apart.
  const source='component Page {\n  Row {\n    Button { }\n  }\n}';
  assert.throws(()=>insertElement(source,parse(source).nodes[0].children[0].start,'Frame { }',false,'inside'),/один контрол Forma/);
});
