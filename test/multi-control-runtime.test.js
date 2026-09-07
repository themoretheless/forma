import {test} from 'node:test';
import assert from 'node:assert/strict';
import {existsSync,readFileSync} from 'node:fs';
import {compileComponents} from '../src/components.js';
const files={
 'ui/Demo.ui':"component Demo { Frame { width:200; height:180; padding:10; gap:12; Button { key:'one'; width:150; height:60; text:'Первый'; clicked -> actions.one(); } Green { key:'two'; width:150; height:60; text:'Второй'; clicked -> actions.two(); } } }",
 'components/Button.ui':"component Button { background:#ff0000; Rectangle { background: Brush { color:props.background; hover:#0000ff; transition:0ms; }; Text { text:props.text; fontSize:16; color:#ffffff; } PointerArea { clicked -> events.clicked(); } } }",
 'components/Green.ui':"component Green : Button { background:#00ff00; }",
};
test('all linked instances reach WASM with distinct templates, origins, state and events',{skip:!existsSync(new URL('../public/vector-pkg/forma.js',import.meta.url))},async()=>{
 const runtime=await import('../public/vector-pkg/forma.js');
 await runtime.default({module_or_path:readFileSync(new URL('../public/vector-pkg/forma_bg.wasm',import.meta.url))});
 const linked=compileComponents(files,'ui/Demo.ui');
 assert.equal(linked.templateTree.roots.length,2);
 assert.deepEqual(linked.instanceTree.nodes.slice(1).map(n=>n.type),['Button','Green']);
 const r=new runtime.Runtime();
 try{
  r.load_component(linked.source,linked.template);
  assert.equal(r.control_count(),2);assert.equal(r.control_label(1),'Второй');
  assert.deepEqual(Array.from(r.control_bounds(1)),[10,82,150,60]);
  assert.deepEqual(Array.from(r.gpu_paints()).slice(0,4),[1,0,0,1]);
  assert.deepEqual(Array.from(r.gpu_paints()).slice(8,12),[0,1,0,1]);
  const geometry=r.gpu_commands(1,false);
  r.pointer(30,100,1);r.pointer(30,100,2);
  assert.equal(r.control_clicks(0),0);assert.equal(r.control_clicks(1),1);
  assert.equal(r.event_index(),1);assert.equal(r.key(),'two');assert.equal(r.action(),'actions.two');
  assert.equal(r.control_focused(1),true);assert.equal(r.control_focused(0),false);
  assert.deepEqual(r.gpu_commands(1,false),geometry);
  r.focus_next(true);r.key_event(1,true,false);r.key_event(1,false,false);
  assert.equal(r.event_index(),0);assert.equal(r.action(),'actions.one');assert.equal(r.control_clicks(0),1);
  assert.throws(()=>r.load_component(linked.source,linked.template.slice(0,-1)));
  assert.equal(r.control_clicks(0),1,'invalid reload must keep the last valid scene');
 }finally{r.free();}
});
test('the second instance is validated rather than silently ignored',()=>{
 assert.throws(()=>compileComponents({...files,'components/Green.ui':'component Green : Missing {}'},'ui/Demo.ui'),/Компонент не найден/);
 assert.throws(()=>compileComponents({...files,'ui/Demo.ui':files['ui/Demo.ui'].replace("key:'two';","key:'two'; unexpected:42;")},'ui/Demo.ui'),/Неизвестное свойство/);
});
