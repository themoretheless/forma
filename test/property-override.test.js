import {test} from 'node:test';
import assert from 'node:assert/strict';
import {compileComponents,createComponentCompiler,linkComponentDefinitions} from '../src/components.js';
import {parser} from '../src/forma-parser.js';

const base=`component Button {
 width: 160; height: 40; text: 'Save'; color: #ffffff;
 Rectangle {
  ContentPresenter { key: 'content';
   Row { gap: 4; Text { key: 'caption'; text: props.text; color: props.color; fontSize: 14; } }
  }
  PointerArea { clicked -> events.clicked(); }
 }
}`;
const files=body=>({
 'components/Button.ui':base,
 'components/LargeButton.ui':`component LargeButton : Button { ${body} }`,
 'ui/Demo.ui':`component Demo { Frame { LargeButton {} Button {} } }`,
});

test('keyed property patches retain descendants, expressions, interaction and base instances',()=>{
 const project=files(`override content { gap: 12; }
 override 'caption' { fontSize: 18; color: state.ink; };`);
 const compiled=compileComponents(project,'ui/Demo.ui',{ink:'#ff0000'});
 const rows=compiled.templateTree.nodes.filter(n=>n.type==='Row');
 const labels=compiled.templateTree.nodes.filter(n=>n.type==='Text');
 assert.deepEqual(rows.map(n=>n.props.gap),[12,4]);
 assert.deepEqual(labels.map(n=>[n.props.text,n.props.fontSize,n.props.color]),[['Save',18,'#ff0000'],['Save',14,{expr:'#ffffff'}]]);
 assert.equal(compiled.templateTree.nodes.filter(n=>n.type==='PointerArea').length,2);
 const origin=labels[0].propertySources.fontSize;
 assert.equal(origin.file,'components/LargeButton.ui');
 assert.equal(project[origin.file].slice(origin.from,origin.to),'18');
});

test('property patches compose through inheritance and cached edits invalidate descendants',()=>{
 const project={...files(`override caption { fontSize: 18; }`),
  'components/FinalButton.ui':`component FinalButton : LargeButton { override caption { color: #00ff00; } }`,
  'ui/Demo.ui':`component Demo { Frame { FinalButton {} } }`};
 const compile=createComponentCompiler();
 const label=()=>compile(project,'ui/Demo.ui').templateTree.nodes.find(n=>n.type==='Text');
 assert.equal(label().props.fontSize,18);
 assert.deepEqual(label().props.color,{expr:'#00ff00'});
 project['components/LargeButton.ui']=project['components/LargeButton.ui'].replace('18','22');
 assert.equal(label().props.fontSize,22);
});

test('content patches modify the root of each inherited match branch',()=>{
 const project={...files(`override content { fontSize: 18; }`),
 'components/Button.ui':`component Button { width: 160; height: 40;
 Rectangle { ContentPresenter { key: 'content'; content: match state.busy {
 true => Text { text: 'Wait'; fontSize: 12; }; _ => Text { text: 'Save'; fontSize: 14; };
 }; } } }`};
 for(const busy of [true,false]){
  const label=compileComponents(project,'ui/Demo.ui',{busy}).templateTree.nodes.find(n=>n.type==='Text');
  assert.equal(label.props.fontSize,18);
  assert.equal(label.props.text,busy?'Wait':'Save');
 }
});

test('invalid patches fail without silently ignoring declarations',()=>{
 for(const [body,error]of [
  ['override missing { color: #ffffff; }',/найдено 0/],
  ["override caption { key: 'other'; }",/не меняет key/],
  ['override caption { Text {} }',/только свойства/],
  ['override caption { clicked -> events.clicked(); }',/только свойства/],
  ['override caption { text <-> state.text; }',/только свойства/],
  ['override caption { forward props { text }; }',/только свойства/],
  ['override caption { match true { true => { fontSize: 18; } } }',/только свойства/],
  ['override caption { fontSize: 18; } override caption {}',/Повторный override/],
 ])assert.throws(()=>linkComponentDefinitions(files(body)),error,body);
 assert.throws(()=>linkComponentDefinitions({'components/A.ui':`component A { override x {} Rectangle {} }`}),/базового компонента/);
 const duplicate=files('override caption {}');
 duplicate['components/Button.ui']=base.replace('fontSize: 14; }','fontSize: 14; } Text { key: \'caption\'; }');
 assert.throws(()=>linkComponentDefinitions(duplicate),/найдено 2/);
});

test('editor grammar accepts bare and quoted keys, with an optional semicolon',()=>{
 const errors=[];
 parser.parse(`component LargeButton : Button { override content { fontSize: 18; } override 'header-caption' { color: #ffffff; }; }`).iterate({enter:n=>{if(n.type.isError)errors.push(n.from);}});
 assert.deepEqual(errors,[]);
});
