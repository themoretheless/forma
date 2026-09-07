import {test} from 'node:test';
import assert from 'node:assert/strict';
import {existsSync,readFileSync} from 'node:fs';
import {compileComponents} from '../src/components.js';
import {parse} from '../src/language.js';

const host=`component Host {
 width:160; height:60; text:'Caller'; color:#ff0000;
 Rectangle { ContentPresenter { key:'content'; Frame {} } }
}`;
const label=`component Label {
 text:'Label default'; color:#00ff00; fontSize:12;
 Text { text:props.text; color:props.color; fontSize:props.fontSize; }
}`;
function compile(body,components={},hostSource=host){
 const files={'ui/Demo.ui':`component Demo { Frame { width:240; height:180; Host { ${body} } } }`,'components/Host.ui':hostSource,'components/Label.ui':label,...components};
 return compileComponents(files,'ui/Demo.ui',{}, {measureText:()=>[70,16]});
}

test('nested visual components own their defaults and receive only explicit arguments',()=>{
 const out=compile(`Frame { columns:[80,80];
 Label { key:'first'; cell:1 1; }
 IconLabel { key:'second'; cell:1 2; text:'Passed'; color:#0000ff; }
 }`,{'components/IconLabel.ui':`component IconLabel {
 text:'Wrapper'; color:#ffffff;
 Frame { Label { text:props.text; } }
 }`});
 const labels=out.templateTree.nodes.filter(n=>n.type==='Text');
 assert.deepEqual(labels.map(n=>n.props.text),['Label default','Passed']);
 assert.deepEqual(labels.map(n=>n.props.color.expr),['#00ff00','#00ff00']);
 const first=labels[0],second=out.templateTree.nodes.find(n=>n.key==='second');
 assert.deepEqual(first.props.cell,[1,1]);assert.deepEqual(second.props.cell,[1,2]);
 assert.equal(first.source.file,'components/Label.ui');
 assert.equal(first.propertySources.cell.file,'ui/Demo.ui');
 assert.equal(first.propertySources.text.file,'components/Label.ui');
 assert.match(out.template,/ContentText \{ x: 0;/);
 assert.match(out.template,/ContentText \{ x: 80;/);
 assert.throws(()=>compile('Bare {}',{'components/Bare.ui':`component Bare { Text { text:props.text; } }`}),/Неизвестное свойство props.text/);
 assert.throws(()=>compile('Label { missing:42; }'),/Неизвестное свойство Label.missing/);
});

test('caller-owned slot children and element arguments retain the caller props scope',()=>{
 const components={'components/Slot.ui':`component Slot {
 text:'Inner'; content:Label { text:props.text; };
 Frame { ContentPresenter { key:'content'; content:props.content; } }
 }`};
 const children=compile(`Slot { text:'Child value'; Label { text:props.text; } }`,components);
 const argument=compile(`Slot { text:'Child value'; content:Label { text:props.text; }; }`,components);
 const defaults=compile('Slot {}',components);
 assert.deepEqual(children.templateTree.nodes.filter(n=>n.type==='Text').map(n=>n.props.text),['Caller']);
 assert.deepEqual(argument.templateTree.nodes.filter(n=>n.type==='Text').map(n=>n.props.text),['Caller']);
 assert.deepEqual(defaults.templateTree.nodes.filter(n=>n.type==='Text').map(n=>n.props.text),['Inner']);
});

test('visual inheritance, ContentPresenter and base.content preserve definition origins',()=>{
 const out=compile('Derived { width:90; height:30; row:1; column:1; }',{
  'components/Base.ui':`component Base { caption:'Base caption'; compact:false;
   Frame { cell:1 1; ContentPresenter { key:'content'; Label { text:props.caption; } } }
  }`,
  'components/Derived.ui':`component Derived : Base { caption:'Derived caption';
   override content: match props.compact { true=>Frame {}; false=>base.content; };
  }`,
 });
 const frame=out.templateTree.nodes.find(n=>n.source?.file==='components/Base.ui'&&n.type==='Frame');
 assert.equal(frame.props.width,90);assert.equal(frame.props.height,30);
  assert.equal(frame.props.row,1);assert.equal(frame.props.column,1);
 assert.equal(frame.props.cell,undefined,'caller row/column replace the definition cell');
 assert.equal(frame.propertySources.width.file,'ui/Demo.ui');
 const text=out.templateTree.nodes.find(n=>n.type==='Text');
 assert.equal(text.props.text,'Derived caption');assert.equal(text.source.file,'components/Label.ui');
});

test('composition cycles and excessive nesting are bounded diagnostics',()=>{
 assert.throws(()=>compile('A {}',{'components/A.ui':'component A { Frame { A {} } }'}),/Цикл композиции: Host → A → A/);
 assert.throws(()=>compile('A {}',{'components/A.ui':'component A { Frame { B {} } }','components/B.ui':'component B { Frame { A {} } }'}),/Цикл композиции/);
 assert.throws(()=>compile('A {}',{'components/A.ui':'component A : B {}','components/B.ui':'component B : A {}'}),/Цикл наследования/);
 const deep={};for(let i=0;i<34;i++)deep[`components/V${i}.ui`]=`component V${i} { Frame { ${i<33?`V${i+1} {}`:''} } }`;
 assert.throws(()=>compile('V0 {}',deep),/Глубина композиции/);
 assert.throws(()=>compile('Frame { '.repeat(70)+'}'.repeat(70)),/предел глубины/);
});

test('nested components cannot smuggle pointer areas, events or unknown primitives',()=>{
 for(const source of [
  'component Action { Rectangle { PointerArea { clicked -> events.clicked(); } } }',
  'component Action { Text { text:"A"; clicked -> events.clicked(); } }',
  'component Action { Frame { Border { background:#fff; } } }',
 ])assert.throws(()=>compile('Action {}',{'components/Action.ui':source}),/интерактивных|визуальный примитив/);
 assert.throws(()=>compile('Label { clicked -> actions.test(); }'),/интерактивных/);
 assert.throws(()=>compile('Label { text <-> state.text; }'),/двусторонние привязки/);
 assert.throws(()=>compile('Label { x:10; }'),/Неизвестное свойство Label.x/);
});

test('nested Rectangle lowers to shared static clips and polygons and rejects brush states',()=>{
 const out=compile('Rectangle { width:20; height:20; radius:4; background:#12345680; Frame { Text { text:"inside"; } } }');
 assert.match(out.template,/ContentClip \{ x: 70; y: 20; width: 20; height: 20; radius: 4;/);
 assert.match(out.template,/ContentShape \{ points: '70 20 90 20 90 40 70 40'; color: #12345680;/);
 assert.match(out.template,/ContentText[^]*ContentClipEnd/);
 for(const props of ['background:Brush { color:#fff; hover:#000; };','background:"transparent";','borderWidth:1;','radius:-1;'])assert.throws(()=>compile(`Rectangle { ${props} }`),/hex|поддерживается|ожидается размер/);
});

test('explicit instance origins and rounded visual composition render through actual WASM',{
 skip:!existsSync(new URL('../public/vector-pkg/forma.js',import.meta.url)),
},async()=>{
 const wasm=await import('../public/vector-pkg/forma.js');
 await wasm.default({module_or_path:readFileSync(new URL('../public/vector-pkg/forma_bg.wasm',import.meta.url))});
 const linked=compile('x:8; y:6;',{},`component Host { width:40; height:40;
 Rectangle { Rectangle { width:20; height:20; radius:6; background:#ff0000;
 Rectangle { width:8; height:8; background:#00ff00; }
 } }
 }`);
 assert.match(linked.source,/x: 8;/);assert.match(linked.source,/y: 6;/);
 const r=new wasm.Runtime();
 try{
  r.load_component(linked.source,linked.template);
  assert.deepEqual(Array.from(r.control_bounds(0)),[8,6,40,40]);
  const image=r.content_pixels(240,180,1),pixel=(x,y)=>Array.from(image.slice((y*240+x)*4,(y*240+x)*4+4));
  assert.deepEqual(pixel(18,16),[0,0,0,0],'rounded corner remains transparent');
  assert.deepEqual(pixel(19,26),[255,0,0,255]);
  assert.deepEqual(pixel(28,26),[0,255,0,255]);
  assert.ok(r.gpu_commands(1,false).length>0,'the same template has native GPU commands');
 }finally{r.free();}
});

function multilineFixture(content,measureText=(value)=>[Array.from(value).length*7,17]) {
 return compileComponents({
  'ui/Multiline.ui':'component Multiline { Frame { width:120; height:120; Host {} } }',
  'components/Host.ui':`component Host { width:120; height:120; Rectangle { ${content} } }`,
 },'ui/Multiline.ui',{}, {measureText});
}

test('explicit newlines preserve leading, internal and trailing empty line positions',()=>{
 const linked=multilineFixture(String.raw`Text { width:100; height:90; fontSize:10; text:'\nА\n\nБ\n'; }`);
 const primitives=parse(linked.template).nodes[0].children;
 const lines=primitives.filter(node=>node.type==='ContentText').map(node=>node.props);
 assert.deepEqual(lines.map(line=>line.text),['','А','','Б','']);
 assert.deepEqual(lines.map(line=>line.y),[15,30,45,60,75]);
 assert.deepEqual(lines.map(line=>line.width),[0,7,0,7,0]);
 assert.ok(lines.every(line=>line.x===10&&line.height===15));
 assert.equal(primitives[0].type,'ContentClip');
 assert.deepEqual(primitives[0].props,{x:10,y:15,width:100,height:90,radius:0});
 assert.equal(primitives.at(-1).type,'ContentClipEnd');
});

test('content-sized grid uses the longest line width and counts blank lines in height',()=>{
 const linked=multilineFixture(String.raw`Frame {
  padding:4; gap:3 7; columns:[-,*]; rows:[-,*];
  Text { cell:1 1; text:'A\nBBB\n'; fontSize:10; }
  Text { cell:1 2; text:'right'; fontSize:10; }
  Text { cell:2 1; text:'v'; fontSize:10; }
 }`);
 const primitives=parse(linked.template).nodes[0].children;
 const clip=primitives.find(node=>node.type==='ContentClip');
 assert.deepEqual(clip.props,{x:4,y:4,width:21,height:45,radius:0});
 const right=primitives.find(node=>node.type==='ContentText'&&node.props.text==='right');
 const below=primitives.find(node=>node.type==='ContentText'&&node.props.text==='v');
 assert.deepEqual([right.props.x,right.props.y,right.props.width,right.props.height],[32,4,84,45]);
 assert.deepEqual([below.props.x,below.props.y,below.props.width,below.props.height],[4,52,21,64]);
});

test('actual WASM preserves a blank line and clips multiline ink to a short text box',{
 skip:!existsSync(new URL('../public/vector-pkg/forma.js',import.meta.url)),
},async()=>{
 const wasm=await import('../public/vector-pkg/forma.js');
 await wasm.default({module_or_path:readFileSync(new URL('../public/vector-pkg/forma_bg.wasm',import.meta.url))});
 function render(height) {
  const linked=multilineFixture(`Frame { rows:[${height},*]; Text { cell:1 1; width:60; fontSize:12; text:'M\\n\\nM'; } }`,wasm.text_metrics);
  const model=new wasm.Runtime();
  try {
   model.load_component(linked.source,linked.template);
   assert.ok(model.gpu_commands(1.25,false).length>0);
   return model.content_pixels(120,120,1);
  }finally{model.free();}
 }
 const full=render(72),short=render(20);
 const ink=(pixels,from,to)=>{
  let sum=0;for(let y=from;y<to;y++)for(let x=0;x<120;x++)sum+=pixels[(y*120+x)*4+3];return sum;
 };
 assert.ok(ink(full,0,18)>0);assert.equal(ink(full,18,36),0,'the explicit empty line keeps its own blank band');
 assert.ok(ink(full,36,54)>0,'the third line is present in the unclipped-height reference');
 assert.ok(ink(short,0,20)>0);assert.equal(ink(short,20,120),0,'overflowing lines cannot paint below the text box');
 for(let y=0;y<120;y++)for(let x=0;x<120;x++)if(x<30||x>=90)assert.equal(short[(y*120+x)*4+3],0);
});
