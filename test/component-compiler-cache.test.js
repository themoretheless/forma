import {test} from 'node:test';
import assert from 'node:assert/strict';
import {compileComponents,createComponentCompiler} from '../src/components.js';
import {createCacheBudget} from '../src/cache-budget.js';

const fixture=()=>({
  'Demo.ui':"component Demo { Frame { Fancy { text:state.label; } Button { text:'Base'; } Fancy { text:'Last'; } } }",
  'components/Button.ui':"component Button { Rectangle { Border { key:'outline'; width:1; background:#111111; } Text { text:props.text; } } }",
  'components/Fancy.ui':"component Fancy : Button { override outline from '../styles/Outline.ui'; }",
  'styles/Outline.ui':"Border { width:2; background:#abcdef; }",
});

test('linked dependency graph reuses definitions and invalidates transitive overrides',()=>{
  const files=fixture(),compile=createComponentCompiler();
  const check=()=>assert.deepEqual(compile(files,'Demo.ui',{label:'x'}),compileComponents(files,'Demo.ui',{label:'x'}));
  check();const cold=compile.cacheStats();check();
  assert.ok(compile.cacheStats().linkedHits>cold.linkedHits);
  files['unrelated.ui']='broken';const misses=compile.cacheStats().linkedMisses;check();
  assert.equal(compile.cacheStats().linkedMisses,misses);
  files['styles/Outline.ui']=files['styles/Outline.ui'].replace('width:2','width:7');check();
  assert.ok(compile.cacheStats().linkedMisses>misses);
  files['components/Button.ui']='component Button : Fancy {}';
  assert.throws(()=>compile(files,'Demo.ui',{label:'x'}),/Цикл наследования/);
});

test('eviction and disabled caches preserve outputs and do not retain projects forever',()=>{
  for(const maxBytes of [0,4096,1024*1024]){
    const storage=createCacheBudget({maxBytes,maxEntries:3}),compile=createComponentCompiler({cache:storage}),files=fixture();
    for(let i=0;i<3;i++){
      assert.deepEqual(compile(files,'Demo.ui',{label:String(i)}),compileComponents(files,'Demo.ui',{label:String(i)}));
      assert.ok(compile.cacheStats().estimatedBytes<=maxBytes);
      assert.ok(compile.cacheStats().entries<=3);
    }
  }
});

test('cached inheritance cannot bypass the depth limit',()=>{
  const files={'Demo.ui':'component Demo { Frame { C31 {} } }','components/C0.ui':'component C0 { Rectangle {} }'};
  for(let i=1;i<=32;i++)files[`components/C${i}.ui`]=`component C${i} : C${i-1} {}`;
  const compile=createComponentCompiler();compile(files,'Demo.ui');
  files['Demo.ui']='component Demo { Frame { C32 {} } }';
  assert.throws(()=>compile(files,'Demo.ui'),/Глубина наследования/);
  assert.throws(()=>compileComponents(files,'Demo.ui'),/Глубина наследования/);
});

test('cached hosts still expand changed nested components and reread SVG and metrics',()=>{
  const files={
    'Demo.ui':'component Demo { Frame { Host {} } }',
    'components/Host.ui':'component Host { Rectangle { Label {} Image { source:\'icon.svg\'; width:10; height:10; } } }',
    'components/Label.ui':"component Label { Text { text:'A'; } }",
    'icon.svg':'<svg viewBox="0 0 10 10"><rect width="10" height="10"/></svg>',
  };
  const compile=createComponentCompiler();
  const check=()=>assert.deepEqual(compile(files,'Demo.ui',{}, {measureText:()=>[20,10]}),compileComponents(files,'Demo.ui',{}, {measureText:()=>[20,10]}));
  check();files['components/Label.ui']=files['components/Label.ui'].replace("'A'","'B'");check();
  files['icon.svg']=files['icon.svg'].replace('width="10"','width="5"');check();
  delete files['components/Label.ui'];assert.throws(()=>compile(files,'Demo.ui'),/не найден/);
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

test('lowering keeps preview controls, template keys and immutable snapshots through scene transforms',()=>{
  const files={
    'Demo.ui':"component Demo { Frame { Scroll { Fancy { key:'first'; text:'Initial'; } } } }",
    'components/Fancy.ui':"component Fancy { Rectangle { key:'surface'; Border { key:'border'; width:1; background:#fff; } Frame { key:'layout'; Text { key:'caption'; text:props.text; } } } }",
  };
  const compile=createComponentCompiler();
  const transformScene=scene=>{
    const controls=scene[0].children[0].children;
    controls.push(structuredClone(controls[0]));
    controls[1].props.key='second';controls[1].props.text='Second';
  };
  const first=compile(files,'Demo.ui',{}, {transformScene});
  const preview=first.previewNodes[0].children[0].children;
  assert.deepEqual(preview.map(node=>[node.type,node.props.text]),[['Fancy','Initial'],['Fancy','Second']]);
  assert.deepEqual(first.templateTree.nodes.filter(n=>n.type==='Text').map(n=>[n.key,n.props.text]),[['caption','Initial'],['caption','Second']]);
  assert.doesNotMatch(first.template,/key:/);
  assert.match(first.source,/Scroll \{[^]*Button \{[^]*Button \{/);
  preview[0].props.text='External mutation';
  assert.equal(first.instanceTree.nodes.find(n=>n.key==='first').props.text,'Initial');
  assert.equal(first.templateTree.nodes.find(n=>n.key==='caption').props.text,'Initial');
  const second=compile(files,'Demo.ui');
  assert.equal(second.previewNodes[0].children[0].children.length,1);
  assert.equal(second.previewNodes[0].children[0].children[0].props.text,'Initial');
});

test('text layout measures each text and size once per compile and refreshes font metrics next time',()=>{
  const files={
    'Demo.ui':"component Demo { Frame { Label {} Label {} } }",
    'components/Label.ui':String.raw`component Label { Rectangle { Frame { columns:[-,-]; rows:[-]; Text { cell:1 1; text:'Same\nOther\nSame'; fontSize:10; } Text { cell:1 2; text:'Same'; fontSize:20; } } } }`,
  };
  const compile=createComponentCompiler(),calls=[],shared=new Float32Array(2);
  const metrics={scale:1,measureText(text,size){calls.push([text,size]);shared[0]=text.length*size*this.scale;shared[1]=size*1.5;return shared;}};
  const first=compile(files,'Demo.ui',{},metrics);
  assert.deepEqual(calls,[['Same',10],['Other',10],['Same',20]]);
  assert.match(first.template,/width: 40; height: 15; text: 'Same'/);
  assert.match(first.template,/width: 50; height: 15; text: 'Other'/);
  calls.length=0;metrics.scale=2;
  const second=compile(files,'Demo.ui',{},metrics);
  assert.deepEqual(calls,[['Same',10],['Other',10],['Same',20]]);
  assert.match(second.template,/width: 80; height: 15; text: 'Same'/);
});

test('scene and instance callbacks keep the caller metrics object as their receiver',()=>{
  const files={
    'Demo.ui':"component Demo { Frame { Button { text:'Before'; } } }",
    'components/Button.ui':"component Button { Rectangle { Text { text:props.text; } } }",
  };
  for(const compile of [compileComponents,createComponentCompiler()]){
    const metrics={
      width:123,label:'Before',transforms:0,patches:0,
      transformScene(scene){assert.equal(this,metrics);this.transforms++;this.label='After';scene[0].props.width=this.width;},
      instanceProps(node){assert.equal(this,metrics);this.patches++;return node.type==='Button'?{text:this.label}:null;},
    };
    const out=compile(files,'Demo.ui',{},metrics);
    assert.equal(metrics.transforms,1);
    assert.equal(metrics.patches,2);
    assert.equal(out.previewNodes[0].props.width,123);
    assert.equal(out.previewNodes[0].children[0].props.text,'After');
    assert.match(out.template,/text: 'After'/);
  }
});

test('metrics hooks inherited from a prototype retain private instance state',()=>{
  const files={
    'Demo.ui':"component Demo { Frame { Button { text:'Before'; } } }",
    'components/Button.ui':"component Button { Rectangle { Text { text:props.text; } } }",
  };
  class Metrics {
    #label='Prototype';
    transforms=0;
    patches=0;
    transformScene(scene){this.transforms++;scene[0].props.width=234;}
    instanceProps(node){this.patches++;return node.type==='Button'?{text:this.#label}:null;}
  }
  for(const compile of [compileComponents,createComponentCompiler()]){
    const metrics=new Metrics(),out=compile(files,'Demo.ui',{},metrics);
    assert.equal(metrics.transforms,1);
    assert.equal(metrics.patches,2);
    assert.equal(out.previewNodes[0].props.width,234);
    assert.equal(out.previewNodes[0].children[0].props.text,'Prototype');
    assert.match(out.template,/text: 'Prototype'/);
  }
});

test('repeated SVG lowering respects geometry, tint, source revisions and empty artwork',()=>{
  const files={
    'Demo.ui':"component Demo { Frame { Icon {} Icon {} Icon { size:10; color:#00ff00; } } }",
    'components/Icon.ui':"component Icon { width:60; height:40; size:20; color:#ff0000; Rectangle { Image { source:'icon.svg'; width:props.size; height:props.size; color:props.color; } } }",
    'icon.svg':'<svg viewBox="0 0 10 10"><rect width="10" height="10" fill="currentColor"/></svg>',
  };
  const compile=createComponentCompiler();
  const shapes=out=>[...out.template.matchAll(/ContentShape \{ points: '([^']*)'; color: ([^;]*); \}/g)].map(m=>[m[1],m[2]]);
  assert.deepEqual(shapes(compile(files,'Demo.ui')),[["20 10 40 10 40 30 20 30",'#ff0000'],["20 10 40 10 40 30 20 30",'#ff0000'],["25 15 35 15 35 25 25 25",'#00ff00']]);
  files['icon.svg']=files['icon.svg'].replace('width="10"','width="5"');
  assert.equal(shapes(compile(files,'Demo.ui'))[0][0],'20 10 30 10 30 30 20 30');
  files['icon.svg']='<svg viewBox="0 0 10 10"></svg>';
  const empty=compile(files,'Demo.ui');
  assert.deepEqual(shapes(empty),[]);
  const withoutImages={...files,'components/Icon.ui':files['components/Icon.ui'].replace(/Image \{[^}]*\}/,'')};
  assert.equal(empty.template,compile(withoutImages,'Demo.ui').template,'empty SVG adds no extra serialized whitespace');
  files['icon.svg']='<svg viewBox="0 0 10 10"><script/></svg>';
  assert.throws(()=>compile(files,'Demo.ui'),/SVG script/);
});
