import {test} from 'node:test';
import assert from 'node:assert/strict';
import {parse} from '../src/language.js';
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
