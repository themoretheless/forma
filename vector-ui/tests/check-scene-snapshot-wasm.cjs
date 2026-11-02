const assert=require('node:assert/strict');
const {resolve}=require('node:path');
const {readFileSync,writeFileSync}=require('node:fs');
const {createHash}=require('node:crypto');
assert(process.argv[2]&&process.argv[3]);const dir=resolve(process.argv[2]);
const {Runtime}=require(resolve(dir,'forma.js'));
const report={node:process.version,cases:[],wasmSha256:createHash('sha256').update(readFileSync(resolve(dir,'forma_bg.wasm'))).digest('hex')};
function check(name,run){try{run();report.cases.push({name,status:'passed'});}catch(error){report.cases.push({name,status:'failed',error:String(error.stack??error)});}}
check('Custom visual template reads props.__state through transferred snapshot',()=>{
 const files={'App.ui':"component App { use 'Caption.ui'; state caption: String = 'initial'; Column { Caption {} } }",'Caption.ui':"component Caption { width:100; height:30; Rectangle { Text { text: props.__state.caption; } } }"};
 const json=Runtime.compile_ui_scene_artifact(JSON.stringify(files),'App.ui',JSON.stringify({caption:'from-state'}));
 const artifact=JSON.parse(json);assert.equal(artifact.controls.length,1);assert(artifact.templates.includes('from-state'));
 assert.equal(artifact.controls[0].state.caption.value,'from-state');
 const runtime=new Runtime();try{runtime.load_scene_artifact(json);assert.equal(runtime.control_count(),1);}finally{runtime.free();}
});
for(const layout of ['Column','Frame'])check(`${layout} For snapshots preserve root collection and distinct owned item values`,()=>{
 const files={'App.ui':"component App { state items: List = []; Column { for item in state.items key item.id { TextInput { width:100; height:30; value <-> item.title; } } } }".replace('Column {',layout+' {')};
 const state={items:[{id:'a',title:'Alpha'},{id:'b',title:'Beta'}]};
 const json=Runtime.compile_ui_scene_artifact(JSON.stringify(files),'App.ui',JSON.stringify(state));const artifact=JSON.parse(json);
 assert.equal(artifact.controls.length,2);
 for(const [i,title] of ['Alpha','Beta'].entries()){
  assert.equal(artifact.controls[i].state.item.value.title.value,title);assert.equal(artifact.controls[i].state.items.value.length,2);
 }
 const runtime=new Runtime();try{runtime.load_scene_artifact(json);assert.equal(runtime.control_count(),2);assert.equal(runtime.text_value(0),'Alpha');assert.equal(runtime.text_value(1),'Beta');}finally{runtime.free();}
});
for(const chained of [false,true])check(`Live input commits through ${chained?'chained':'single'} retained Let collection alias`,()=>{
 const files={'App.ui':"component App { state items: List = []; Column { width:260; height:200; let visible = state.items; for item in visible key item.id { TextInput { width:200; height:32; value <-> item.title; } } } }".replace('for item in visible',chained?'let chosen = visible; for item in chosen':'for item in visible')};
 const runtime=new Runtime();
 try{runtime.load_ui_files_json(JSON.stringify(files),'App.ui',JSON.stringify({items:[{id:'a',title:'Alpha'},{id:'b',title:'Beta'}]}));
  assert.equal(runtime.control_count(),2);assert(runtime.focus_control(0));assert(runtime.text_insert('-edited'));
  const edited=runtime.text_value(0);assert.notEqual(edited,'Alpha');assert.equal(runtime.sync_ui_bindings(),true);
  const state=JSON.parse(runtime.ui_state_json());assert.equal(state.items[0].title,edited);assert.equal(state.items[1].title,'Beta');assert.equal(runtime.text_value(0),edited);assert.equal(runtime.sync_ui_bindings(),false);
 }finally{runtime.free();}
});
report.status=report.cases.every(c=>c.status==='passed')?'passed':'failed';writeFileSync(process.argv[3],JSON.stringify(report,null,2)+'\n');console.log(JSON.stringify(report,null,2));if(report.status!=='passed')process.exitCode=1;
