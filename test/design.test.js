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

test('vector compiler applies #[design] base overrides, named states and design.* tokens',async()=>{
  const {compileComponents}=await import('../src/components.js');
  const {setDesignData}=await import('../src/design-data.js');
  const button=`component Button {
    width: 180; height: 40; text: 'Button'; disabled: false;
    Rectangle { background: #123456; Text { text: props.text; } PointerArea { clicked -> events.clicked(); } }
  }`;
  const files={
    'components/Button.ui':button,
    'ui/SearchWindow.ui':`#[design('./SearchWindow.design.ui')]
component SearchWindow {
  Frame {
    Text { key: 'status'; text: state.status; }
    Button { key: 'searchButton'; text: 'Найти'; disabled: state.loading; clicked -> actions.search(); }
  }
}`,
    'ui/SearchWindow.design.ui':`design SearchWindow {
  Text { key: 'status'; text: 'Базовый статус'; }
  Button { key: 'searchButton'; disabled: false; }
  state 'поиск' {
    Text { key: 'status'; text: 'Ищем…'; }
  }
  state 'нет результатов' {
    Button { key: 'searchButton'; disabled: true; }
    Text { key: 'status'; text: design.EMPTY_STATUS; }
  }
}`,
  };
  const metrics={measureText:()=>[80,20]};
  // Base design overrides (designState: null)
  const baseOut=compileComponents(files,'ui/SearchWindow.ui',{},{...metrics,designState:null});
  assert.match(baseOut.template,/Базовый статус/);
  assert.match(baseOut.source,/disabled: false;[^}]*key: 'searchButton';/);

  // Default state (first state: 'поиск')
  const firstOut=compileComponents(files,'ui/SearchWindow.ui',{},metrics);
  assert.match(firstOut.template,/Ищем…/);
  assert.match(firstOut.source,/disabled: false;[^}]*key: 'searchButton';/);

  // Second state ('нет результатов') with design.* token
  setDesignData({'design.EMPTY_STATUS':'Ничего не найдено'});
  try{
    const emptyOut=compileComponents(files,'ui/SearchWindow.ui',{},{...metrics,designState:'нет результатов'});
    assert.match(emptyOut.template,/Ничего не найдено/);
    assert.match(emptyOut.source,/disabled: true;[^}]*key: 'searchButton';/);
  }finally{
    setDesignData({});
  }

  // Interact mode (designMode: false) uses live runtime state instead of design overrides
  const liveOut=compileComponents(files,'ui/SearchWindow.ui',{status:'Живой статус',loading:true},{...metrics,designMode:false});
  assert.match(liveOut.template,/Живой статус/);
  assert.match(liveOut.source,/disabled: true;[^}]*key: 'searchButton';/);
});
