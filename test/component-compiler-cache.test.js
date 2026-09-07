import {test} from 'node:test';
import assert from 'node:assert/strict';
import {compileComponents,createComponentCompiler} from '../src/components.js';

const fixture=()=>({
  'Demo.ui':"component Demo { Frame { Fancy { text:state.label; } Button { text:'Base'; } Fancy { text:'Last'; } } }",
  'components/Button.ui':"component Button { Rectangle { Border { key:'outline'; width:1; background:#111111; } Text { text:props.text; } } }",
  'components/Fancy.ui':"component Fancy : Button { override outline from '../styles/Outline.ui'; }",
  'styles/Outline.ui':"Border { width:2; background:#abcdef; }",
});

test('session compiler invalidates every changed source and re-evaluates state and metrics',()=>{
  const files=fixture(),compile=createComponentCompiler();
  const check=(state={label:'First'},metrics={})=>assert.deepEqual(compile(files,'Demo.ui',state,metrics),compileComponents(files,'Demo.ui',state,metrics));
  check();check({label:'Changed'});
  check({label:'Preset'},{instanceProps:n=>n.type==='Fancy'?{text:'Override'}:null});
  files['styles/Outline.ui']="Border { width:3; background:#fedcba; }";check();
  files['components/Button.ui']=files['components/Button.ui'].replace('width:1','width:4');check();
  files['components/Fancy.ui']=files['components/Fancy.ui'].replace("Outline.ui';","Outline.ui' { width:5; };");check();
  files['Demo.ui']=files['Demo.ui'].replace("text:'Last'","text:'New'");check();
  files['styles/Outline.ui']='Border { invalid';assert.throws(()=>compile(files,'Demo.ui',{label:'x'}));
  files['styles/Outline.ui']="Border { width:6; background:#123456; }";check();
  delete files['styles/Outline.ui'];assert.throws(()=>compile(files,'Demo.ui',{label:'x'}),/не найден/);
});

test('shared linked templates isolate inheritance, instances and returned snapshots',()=>{
  const files=fixture(),compile=createComponentCompiler(),first=compile(files,'Demo.ui',{label:'First'});
  assert.deepEqual(first.templateTree.nodes.filter(n=>n.type==='Border').map(n=>n.props.width),[2,1,2]);
  assert.deepEqual(first.templateTree.nodes.filter(n=>n.type==='Text').map(n=>n.props.text),['First','Base','Last']);
  first.visualNodes[0].props.text='Caller mutation';
  first.visualNodes[0].source.file='Caller mutation';
  const second=compile(files,'Demo.ui',{label:'First'});
  assert.deepEqual(second,compileComponents(files,'Demo.ui',{label:'First'}));
  assert.equal(first.templateTree.nodes.find(n=>n.type==='Text').props.text,'First');
  assert.throws(()=>{second.templateTree.nodes[0].props.width=999;},TypeError);
});

test('nested instance layout overrides do not remove definition origins from later instances',()=>{
  const files={
    'Demo.ui':"component Demo { Frame { Host {} } }",
    'components/Host.ui':"component Host { Rectangle { Frame { columns:[80,80]; Label { row:1; column:1; } Label {} } } }",
    'components/Label.ui':"component Label { Text { cell:1 2; text:'Label'; } }",
  };
  const out=createComponentCompiler()(files,'Demo.ui',{}, {measureText:()=>[40,16]});
  const labels=out.templateTree.nodes.filter(n=>n.type==='Text');
  assert.equal(labels[0].propertySources.cell,undefined);
  assert.equal(labels[1].propertySources.cell.file,'components/Label.ui');
  assert.deepEqual(labels[1].props.cell,[1,2]);
});
