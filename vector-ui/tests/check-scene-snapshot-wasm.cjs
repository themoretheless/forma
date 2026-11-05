const assert=require('node:assert/strict');
const {resolve}=require('node:path');
const {readFileSync,writeFileSync}=require('node:fs');
const {createHash}=require('node:crypto');
assert(process.argv[2]&&process.argv[3]);const dir=resolve(process.argv[2]);
const {Runtime,text_metrics,shaped_text_metrics}=require(resolve(dir,'forma.js'));
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
for(const chained of [false,true])check(`Warmed idle and reload reorder preserve ${chained?'chained':'single'} alias writes`,()=>{
 const files={'App.ui':"component App { state items: List = []; Column { width:360; height:200; let visible = state.items; for item in visible key item.id { TextInput { width:200; height:32; value <-> item.title; } } } }".replace('for item in visible',chained?'let rows = visible; for item in rows':'for item in visible')};
 const runtime=new Runtime();try{
  runtime.load_ui_files_json(JSON.stringify(files),'App.ui',JSON.stringify({items:[{id:'a',title:'Alpha'},{id:'b',title:'Beta'}]}));
  assert.equal(runtime.sync_ui_bindings(),false);const revision=runtime.layout_revision();
  for(let n=0;n<8;n++)assert.equal(runtime.sync_ui_bindings(),false);assert.equal(runtime.layout_revision(),revision);
  assert(runtime.focus_control(0));runtime.text_key('End',false,false);assert(runtime.text_insert('—WASM-A'));assert.equal(runtime.sync_ui_bindings(),true);
  const edited=JSON.parse(runtime.ui_state_json()).items[0].title;assert(edited.endsWith('—WASM-A'));
  for(let n=0;n<8;n++)assert.equal(runtime.sync_ui_bindings(),false);
  const reordered=JSON.parse(runtime.ui_state_json());reordered.items.reverse();
  runtime.load_ui_files_json(JSON.stringify(files),'App.ui',JSON.stringify(reordered));assert.equal(runtime.text_value(0),'Beta');assert.equal(runtime.text_value(1),edited);
  assert.equal(runtime.sync_ui_bindings(),false);assert(runtime.focus_control(0));runtime.text_key('End',false,false);assert(runtime.text_insert('—WASM-B'));assert.equal(runtime.sync_ui_bindings(),true);
  const final=JSON.parse(runtime.ui_state_json());assert.deepEqual(final.items.map(item=>item.id),['b','a']);assert.equal(final.items[0].title,'Beta—WASM-B');assert.equal(final.items[1].title,edited);
  for(let n=0;n<8;n++)assert.equal(runtime.sync_ui_bindings(),false);
 }finally{runtime.free();}
});
check('Project reload errors preserve live document, focus and cache versions',()=>{
 const project=value=>JSON.stringify({'App.ui':`component App { state query: String = '${value}'; Column { width:240; height:100; TextInput { key:'editor'; width:200; height:32; value <-> state.query; } } }`});
 const runtime=new Runtime();try{
  runtime.load_ui_files_json(project('old'),'App.ui','{}');assert(runtime.focus_control(0));runtime.text_key('End',false,false);runtime.text_insert('—before');runtime.sync_ui_bindings();
  const snapshot=()=>({state:runtime.ui_state_json(),text:runtime.text_value(0),focus:runtime.focused_index(),geometry:runtime.geometry_revision(),layout:runtime.layout_revision(),visual:runtime.visual_revision()});
  const before=snapshot();
  for(const [files,entry,state] of [['{','App.ui','{}'],['{}','missing.ui','{}'],[JSON.stringify({'App.ui':'component App { Missing {} }'}),'App.ui','{}'],[project('next'),'App.ui','{']]){
   assert.throws(()=>runtime.load_ui_files_json(files,entry,state));assert.deepEqual(snapshot(),before);
  }
  runtime.text_insert('—after');assert(runtime.sync_ui_bindings());assert(JSON.parse(runtime.ui_state_json()).query.endsWith('—before—after'));
  const previous=snapshot();runtime.load_ui_files_json(project('next'),'App.ui','{}');assert.equal(runtime.text_value(0),'next');assert.equal(runtime.geometry_revision(),(previous.geometry+1)>>>0);assert.equal(runtime.layout_revision(),(previous.layout+1)>>>0);
 }finally{runtime.free();}
});
check('Live disabled TextInput binding locks and unlocks without reload',()=>{
 const files={'App.ui':"component App { state locked: Bool = false; state query: String = 'seed'; Column { width:260; height:160; TextInput { key:'editor'; disabled:state.locked; width:200; height:32; value <-> state.query; } Toggle { key:'lock'; checked <-> state.locked; } } }",'Toggle.component.ui':"component Toggle { prop checked: Bool = false; width:100; height:24; Rectangle { PointerArea { clicked -> events.clicked(); } } }"};
 const view=new Runtime();try{
  view.load_ui_files_json(JSON.stringify(files),'App.ui','{}');assert(view.focus_control(0));view.text_key('End',false,false);view.text_insert('-kept');view.text_preedit('未確定');
  view.activate_control(1);assert(view.sync_ui_bindings());assert(view.control_disabled(0));assert(!view.text_editing());assert(!view.focus_control(0));assert(!view.text_insert('-blocked'));
  assert.equal(view.text_value(0),'seed-kept');assert.deepEqual(JSON.parse(view.ui_state_json()),{locked:true,query:'seed-kept'});
  const locked=JSON.parse(view.accessibility_tree_json()).nodes.find(n=>n.key==='editor');assert(locked.disabled);assert.deepEqual(locked.actions,[]);
  view.activate_control(1);assert(view.sync_ui_bindings());assert(!view.control_disabled(0));assert(view.focus_control(0));view.text_key('End',false,false);view.text_insert('-back');view.sync_ui_bindings();assert.deepEqual(JSON.parse(view.ui_state_json()),{locked:false,query:'seed-kept-back'});
 }finally{view.free();}
});
check('Nested layout keeps control snapshots and live writes with unrelated payload',()=>{
 let body="TextInput { key:'editor'; width:100; height:32; value <-> state.query; }";for(let i=0;i<24;i++)body=`Column { width:200; height:100; ${body} }`;
 const files=JSON.stringify({'App.ui':`component App { state query: String = 'seed'; ${body} }`});const state=JSON.stringify({payload:Array.from({length:256},(_,i)=>({data:`payload-${i}`}))});
 const artifact=JSON.parse(Runtime.compile_ui_scene_artifact(files,'App.ui',state));assert.equal(artifact.controls.length,1);assert.equal(artifact.controls[0].state.payload.value.length,256);
 const view=new Runtime();try{
  view.load_ui_files_json(files,'App.ui',state);assert(view.focus_control(0));view.text_key('End',false,false);view.text_insert('-nested');assert(view.sync_ui_bindings());const model=JSON.parse(view.ui_state_json());assert.equal(model.query,'seed-nested');assert.equal(model.payload.length,256);assert.equal(model.payload[255].data,'payload-255');assert.equal(view.sync_ui_bindings(),false);
 }finally{view.free();}
});
check('Large resolved List property retains contract type validation',()=>{
 const files={'App.ui':"component App { state payload: List = []; Column { width:200; height:100; Card { payload:state.payload; } } }",'Card.ui':"component Card { prop payload: List = []; width:100; height:32; Rectangle { background:#123456; PointerArea { clicked -> events.clicked(); } } }"};
 const state=JSON.stringify({payload:Array.from({length:256},(_,i)=>({data:`payload-${i}`}))});
 const artifact=JSON.parse(Runtime.compile_ui_scene_artifact(JSON.stringify(files),'App.ui',state));assert.equal(artifact.controls.length,1);assert.equal(artifact.controls[0].state.payload.value.length,256);
 const view=new Runtime();try{
  view.load_ui_files_json(JSON.stringify(files),'App.ui',state);assert.equal(view.control_count(),1);const before=view.ui_state_json(),revision=view.layout_revision();
  const invalid={...files,'App.ui':files['App.ui'].replace('payload:state.payload',"payload:'wrong-type'")};
  let refused=false;try{view.load_ui_files_json(JSON.stringify(invalid),'App.ui',state);}catch(error){refused=String(error).includes('expects List');}assert(refused);assert.equal(view.ui_state_json(),before);assert.equal(view.layout_revision(),revision);
 }finally{view.free();}
});
check('Source wrappers throw atomically and recover after invalid markup/template',()=>{
 const view=new Runtime();try{
  const before={count:view.control_count(),layout:view.layout_revision(),geometry:view.geometry_revision()};
  for(const action of [()=>view.load('invalid markup'),()=>view.load_component('component Demo { Frame { width:200; height:100; Button { width:100; height:32; } } }','component Button {'),()=>Runtime.compile_ui_scene_artifact('{','App.ui','{}')]){
   let refused=false;try{action();}catch{refused=true;}assert(refused);assert.equal(view.control_count(),before.count);assert.equal(view.layout_revision(),before.layout);assert.equal(view.geometry_revision(),before.geometry);
  }
  view.load_ui_files_json(JSON.stringify({'App.ui':"component App { Column { width:200; height:100; TextInput { key:'editor'; value:'recovered'; width:180; height:32; } } }"}),'App.ui','{}');assert.equal(view.text_value(0),'recovered');
 }finally{view.free();}
});
check('Modal blocks base scroll without cancelling its pressed control',()=>{
 const view=new Runtime();try{
  view.load("component Demo { Frame { width:200; height:60; padding:10; clip:true; Scroll { Button { key:'base'; width:80; height:200; } } Modal { key:'ask'; x:20; y:40; width:160; height:80; Button { key:'yes'; width:60; height:24; } } } }");
  view.close_modal(0);view.scroll(0,1000);const offset=Array.from(view.scroll_offset()),base=Array.from(view.control_bounds(0));assert(offset[1]>0);view.open_modal(0);view.pointer(30,50,1);view.scroll(0,-20);assert.deepEqual(Array.from(view.scroll_offset()),offset);assert.deepEqual(Array.from(view.control_bounds(0)),base);view.pointer(30,50,2);assert.equal(view.control_clicks(1),1);view.close_modal(0);view.scroll(0,-20);assert(view.scroll_offset()[1]<offset[1]);
 }finally{view.free();}
});
check('Portable CJK and emoji use real pixels and matching simple/shaped advances',()=>{
 for(const value of ['你','🙂','你好 мир🙂']){
  const simple=Array.from(text_metrics(value,28)),shaped=Array.from(shaped_text_metrics(value,28));assert(simple[0]>0);assert(Math.abs(simple[0]-shaped[0])<0.001);
  const view=new Runtime(),question=new Runtime();try{
   const files=text=>JSON.stringify({'App.ui':`component App { Column { width:240; height:80; TextInput { width:220; height:40; fontSize:28; value:'${text}'; } } }`});
   view.load_ui_files_json(files(value),'App.ui','{}');question.load_ui_files_json(files('?'),'App.ui','{}');assert.equal(view.text_value(0),value);assert.notDeepEqual(Array.from(view.pixels(240,80,1)),Array.from(question.pixels(240,80,1)));
   view.focus_control(0);view.text_key('Home',false,false);const start=view.text_caret_bounds()[0];view.text_key('End',false,false);const end=view.text_caret_bounds()[0];assert(Math.abs(end-start-simple[0])<0.01);
  }finally{view.free();question.free();}
 }
});
for(const cluster of ['e\u0301','👍🏽','👨‍👩‍👧‍👦','🇬🇪','1\ufe0f\u20e3','각','क्‍ष'])check(`Grapheme editing preserves ${cluster} atomically`,()=>{
 const view=new Runtime();try{
  const files=JSON.stringify({'App.ui':"component App { state query: String = ''; Column { width:240; height:100; TextInput { width:220; height:40; value <-> state.query; } } }"});
  const original=`L${cluster}R`,payload=[{id:'keep',value:'unchanged'}];
  view.load_ui_files_json(files,'App.ui',JSON.stringify({query:original,payload}));assert(view.focus_control(0));
  view.text_key('Home',false,false);view.text_key('ArrowRight',false,false);view.text_key('ArrowRight',true,false);
  assert.equal(view.text_selected(),cluster);view.text_key('Backspace',false,false);assert(view.sync_ui_bindings());
  assert.equal(view.text_value(0),'LR');assert.deepEqual(JSON.parse(view.ui_state_json()),{query:'LR',payload});
  view.text_key('z',false,true);assert(view.sync_ui_bindings());assert.equal(view.text_value(0),original);
  view.text_key('End',false,false);view.text_key('ArrowLeft',false,false);view.text_key('Backspace',false,false);
  assert(view.sync_ui_bindings());assert.equal(view.text_value(0),'LR');assert(view.pixels(240,100,1.25).length>0);
 }finally{view.free();}
});
check('Normalized word navigation/deletion retains Unicode clusters and binding payload',()=>{
 const view=new Runtime();try{
  const files=JSON.stringify({'App.ui':"component App { state query: String = ''; Column { width:300; height:100; TextInput { width:280; height:40; value <-> state.query; } } }"});
  const payload=[{id:'keep'}],family='👨‍👩‍👧‍👦';
  view.load_ui_files_json(files,'App.ui',JSON.stringify({query:`alpha é ${family} omega`,payload}));view.focus_control(0);
  view.text_key('End',false,false);assert(view.text_word_key('ArrowLeft',true));assert.equal(view.text_selected(),'omega');
  view.text_word_key('Backspace',false);assert(view.sync_ui_bindings());assert.equal(view.text_value(0),`alpha é ${family} `);
  view.text_word_key('Backspace',false);assert(view.sync_ui_bindings());assert.equal(view.text_value(0),'alpha é ');
  view.text_key('z',false,true);assert(view.sync_ui_bindings());assert.deepEqual(JSON.parse(view.ui_state_json()),{query:`alpha é ${family} `,payload});
 }finally{view.free();}
});
check('Word-forward deletion is additive to ordinary grapheme keys',()=>{
 const view=new Runtime();try{
  view.load_ui_files_json(JSON.stringify({'App.ui':"component App { Column { TextInput { width:200; height:32; value:'alpha beta'; } } }"}),'App.ui','{}');view.focus_control(0);
  view.text_key('Home',false,false);view.text_word_key('ArrowRight',false);view.text_word_key('Delete',false);assert.equal(view.text_value(0),'alpha');
  view.text_key('Home',false,false);view.text_key('ArrowRight',true,false);assert.equal(view.text_selected(),'a');assert.equal(view.text_word_key('a',false),false);
  view.focus(false);assert.equal(view.text_word_key('Backspace',false),false);
 }finally{view.free();}
});
check('Explicit editor commands remove marked pixels without changing committed history',()=>{
 const source=JSON.stringify({'App.ui':"component App { Column { width:240; height:100; TextInput { width:200; height:32; value:'alpha beta'; } } }"});
 for(const action of ['plain','word','undo']){
  const views=[new Runtime(),new Runtime()];try{
   for(const view of views){view.load_ui_files_json(source,'App.ui','{}');view.set_reduced_motion(true);view.focus_control(0);view.text_key('End',false,false);view.text_insert('!');}
   const clean=views[0].pixels(240,100,1).slice();views[1].text_preedit('未確定');assert.notDeepEqual(views[1].pixels(240,100,1),clean,'marked text must affect visible pixels');
   for(const view of views){if(action==='plain')view.text_key('ArrowLeft',false,false);else if(action==='word')view.text_word_key('Backspace',false);else view.text_key('z',false,true);}
   assert.equal(views[1].text_value(0),views[0].text_value(0));assert.deepEqual(views[1].pixels(240,100,1),views[0].pixels(240,100,1),action+' stale marked pixels');
  }finally{for(const view of views)view.free();}
 }
});
check('CRLF End, pointer hit and vertical selection retain complete line breaks',()=>{
 const view=new Runtime();try{
  const value='first\r\nlast';
  view.load_ui_files_json(JSON.stringify({'App.ui':"component App { state query: String = ''; Column { width:240; height:120; TextInput { width:200; height:90; multiline:true; value <-> state.query; } } }"}),'App.ui',JSON.stringify({query:value}));assert(view.focus_control(0));
  view.text_key('Home',false,false);view.text_key('ArrowUp',false,false);view.text_key('Home',false,false);
  view.text_key('End',true,false);assert.equal(view.text_selected(),'first');view.text_key('Backspace',false,false);assert.equal(view.text_value(0),'\r\nlast');view.text_key('z',false,true);assert.equal(view.text_value(0),value);
  const [x,y,w]=view.control_bounds(0);view.pointer(x+w-2,y+1,1);view.pointer(x+w-2,y+1,2);view.text_key('Home',true,false);assert.equal(view.text_selected(),'first');
  view.text_key('End',false,false);view.text_key('ArrowDown',true,false);assert.equal(view.text_selected(),'\r\nlast');
  view.text_key('Backspace',false,false);assert.equal(view.text_value(0),'first');view.text_key('z',false,true);assert.equal(view.text_value(0),value);
 }finally{view.free();}
});
check('CRLF and LF render exactly equal pixels with selection and preserve bound bytes',()=>{
 const files=JSON.stringify({'App.ui':"component App { state query: String = ''; Column { width:240; height:120; TextInput { width:200; height:90; multiline:true; value <-> state.query; } } }"});
 const views=[new Runtime(),new Runtime()];try{
  for(let i=0;i<2;i++){views[i].load_ui_files_json(files,'App.ui',JSON.stringify({query:i?'first\r\nlast':'first\nlast'}));views[i].set_reduced_motion(true);views[i].focus_control(0);}
  for(const select of [false,true]){
   if(select)for(const view of views)view.text_key('a',false,true);
   assert.deepEqual(views[0].pixels(240,120,1),views[1].pixels(240,120,1),'selected='+select);
   assert.deepEqual(views[0].text_caret_bounds(),views[1].text_caret_bounds());
  }
  assert.equal(views[1].text_value(0),'first\r\nlast');assert.equal(JSON.parse(views[1].ui_state_json()).query,'first\r\nlast');
 }finally{for(const view of views)view.free();}
});
check('Vertical keys preserve desired column through short rows for LF and CRLF',()=>{
 for(const newline of ['\n','\r\n']){
  const view=new Runtime();try{
   const value=['abcdefgh','x','abcdefgh'].join(newline);
   view.load_ui_files_json(JSON.stringify({'App.ui':"component App { state query: String = ''; Column { width:320; height:150; TextInput { width:280; height:120; multiline:true; value <-> state.query; } } }"}),'App.ui',JSON.stringify({query:value}));view.focus_control(0);
   view.text_key('Home',false,false);view.text_key('ArrowUp',false,false);view.text_key('ArrowUp',false,false);view.text_key('Home',false,false);
   for(let i=0;i<7;i++)view.text_key('ArrowRight',false,false);
   const first=view.text_caret_bounds();view.text_key('ArrowDown',false,false);view.text_key('ArrowDown',false,false);const last=view.text_caret_bounds();assert.equal(last[0],first[0]);
   view.text_key('Shift',true,false);view.text_key('ArrowUp',true,false);view.text_key('Shift',true,false);view.text_key('ArrowUp',true,false);assert.equal(view.text_selected(),'h'+newline+'x'+newline+'abcdefg');
   view.text_key('Backspace',false,false);assert.equal(view.text_value(0),'abcdefgh');view.text_key('z',false,true);assert.equal(view.text_value(0),value);
  }finally{view.free();}
 }
});
check('Command line deletion keeps LF/CRLF and is one undoable replacement',()=>{
 for(const newline of ['\n','\r\n'])for(const forward of [false,true]){
  const view=new Runtime();try{
   const value='abcdefgh'+newline+'last';view.load_ui_files_json(JSON.stringify({'App.ui':"component App { state query: String = ''; Column { width:320; height:140; TextInput { width:280; height:100; multiline:true; value <-> state.query; } } }"}),'App.ui',JSON.stringify({query:value}));view.focus_control(0);view.text_key('Home',false,false);view.text_key('ArrowUp',false,false);view.text_key('Home',false,false);for(let i=0;i<7;i++)view.text_key('ArrowRight',false,false);
   assert(view.text_key(forward?'Delete':'Backspace',false,true));assert.equal(view.text_value(0),(forward?'abcdefg':'h')+newline+'last');
   view.text_key('z',false,true);assert.equal(view.text_value(0),value);view.text_key('z',true,true);assert.equal(view.text_value(0),(forward?'abcdefg':'h')+newline+'last');
  }finally{view.free();}
 }
});
check('Line deletion joining RI and ZWJ clusters aligns caret and preserves undo selection',()=>{
 for(const [value,cluster] of [['🇬X🇪','🇬🇪'],['👨‍X👩','👨‍👩']])for(const forward of [false,true]){
  const view=new Runtime();try{
   view.load_ui_files_json(JSON.stringify({'App.ui':"component App { state query: String = ''; Column { TextInput { width:280; height:40; value <-> state.query; } } }"}),'App.ui',JSON.stringify({query:value}));view.focus_control(0);view.text_key('Home',false,false);view.text_key('ArrowRight',false,false);view.text_key('ArrowRight',true,false);assert.equal(view.text_selected(),'X');
   view.text_key(forward?'Delete':'Backspace',false,true);assert.equal(view.text_value(0),cluster);
   view.text_key('ArrowLeft',true,false);assert.equal(view.text_selected(),cluster,'post-join caret must be after whole cluster');
   view.text_key('z',false,true);assert.equal(view.text_value(0),value);assert.equal(view.text_selected(),'X');
   view.text_key('z',true,true);assert.equal(view.text_value(0),cluster);view.text_key('ArrowLeft',true,false);assert.equal(view.text_selected(),cluster);
  }finally{view.free();}
 }
});
check('Multiline metadata exports editor semantics and rejects noneditor indices',()=>{
 const view=new Runtime();try{
  view.load_ui_files_json(JSON.stringify({'App.ui':"component App { Column { TextInput { multiline:false; } TextInput { multiline:true; } } }"}),'App.ui','{}');
  assert.equal(view.control_multiline(0),false);assert.equal(view.control_multiline(1),true);assert.equal(view.control_multiline(999),false);
 }finally{view.free();}
});
check('DOM UTF16 selection uses surrogate pairs and normalized CRLF offsets',()=>{
 const view=new Runtime();try{
  view.load_ui_files_json(JSON.stringify({'App.ui':"component App { state query: String = ''; Column { TextInput { multiline:true; value <-> state.query; } } }"}),'App.ui',JSON.stringify({query:'A😀\r\né'}));view.focus_control(0);view.text_key('End',false,false);assert.deepEqual(Array.from(view.text_selection_utf16()),[6,6]);view.text_key('ArrowLeft',true,false);assert.deepEqual(Array.from(view.text_selection_utf16()),[6,4]);assert.equal(view.text_selected(),'é');assert.equal(view.text_value(0),'A😀\r\né');
 }finally{view.free();}
});
check('DOM selection import maps reverse Unicode and partial surrogate offsets safely',()=>{
 const view=new Runtime();try{
  view.load_ui_files_json(JSON.stringify({'App.ui':"component App { state query: String = ''; Column { TextInput { multiline:true; value <-> state.query; } } }"}),'App.ui',JSON.stringify({query:'A😀\r\né'}));view.focus_control(0);
  assert(view.text_set_selection_utf16(6,1));assert.equal(view.text_selected(),'😀\r\né');assert.deepEqual(Array.from(view.text_selection_utf16()),[6,1]);assert.equal(view.text_set_selection_utf16(6,1),false);
  view.text_set_selection_utf16(2,3);assert.equal(view.text_selected(),'😀');view.text_set_selection_utf16(2,2);assert.deepEqual(Array.from(view.text_selection_utf16()),[3,3]);
 }finally{view.free();}
});
check('Explicit editor label is stable across bound edits and distinct from value',()=>{
 const view=new Runtime();try{
  view.load_ui_files_json(JSON.stringify({'App.ui':"component App { state query: String = 'before'; Column { TextInput { key:'search'; label:'Search'; placeholder:'Hint'; value <-> state.query; } } }"}),'App.ui','{}');view.focus_control(0);assert.equal(view.control_input_label(0),'Search');view.text_key('a',false,true);view.text_insert('after');view.sync_ui_bindings();assert.equal(view.control_input_label(0),'Search');const node=JSON.parse(view.accessibility_tree_json()).nodes.find(n=>n.key==='search');assert.equal(node.label,'Search');assert.equal(node.value,'after');
 }finally{view.free();}
});
check('Accessibility snapshot JSON roundtrips control characters under JavaScript JSON.parse',()=>{
 const view=new Runtime();try{
  const value=Array.from({length:32},(_,i)=>String.fromCharCode(i)).join('')+' quote" slash\\ 😀';const label='Label '+value;
  view.load_ui_files_json(JSON.stringify({'App.ui':"component App { state query: String = ''; state caption: String = ''; Column { TextInput { key:'editor'; label:state.caption; multiline:true; value <-> state.query; } } }"}),'App.ui',JSON.stringify({query:value,caption:label}));view.focus_control(0);
  const tree=JSON.parse(view.accessibility_tree_json()),node=tree.nodes.find(n=>n.key==='editor');assert.equal(node.value,value);assert.equal(node.label,label);assert.equal(tree.focusedId,node.id);
 }finally{view.free();}
});
check('Readonly editor exports state and refuses edits while retaining Unicode selection',()=>{
 const view=new Runtime();try{
  view.load_ui_files_json(JSON.stringify({'App.ui':"component App { Column { TextInput { key:'locked'; label:'Reference'; readOnly:true; value:'A😀é'; } } }"}),'App.ui','{}');assert(view.control_readonly(0));assert(view.focus_control(0));view.text_key('a',false,true);assert.equal(view.text_selected(),'A😀é');assert.equal(view.text_insert('blocked'),false);
  for(const key of ['Backspace','Delete','Enter'])for(const command of [false,true])view.text_key(key,false,command);view.text_word_key('Backspace',false);view.text_key('z',false,true);assert.equal(view.text_value(0),'A😀é');view.text_set_selection_utf16(5,1);assert.equal(view.text_selected(),'😀é');const node=JSON.parse(view.accessibility_tree_json()).nodes.find(n=>n.key==='locked');assert.equal(node.readOnly,true);assert.equal(node.actions.includes('set_value'),false);
 }finally{view.free();}
});
check('Native semantic selection retains original CRLF UTF16 units independently of DOM units',()=>{
 const view=new Runtime();try{
  view.load_ui_files_json(JSON.stringify({'App.ui':"component App { state query: String = ''; Column { TextInput { key:'editor'; multiline:true; readOnly:true; value <-> state.query; } } }"}),'App.ui',JSON.stringify({query:'A😀\r\né'}));view.focus_control(0);view.text_key('End',false,false);view.text_key('ArrowLeft',true,false);const node=JSON.parse(view.accessibility_tree_json()).nodes.find(n=>n.key==='editor');assert.equal(node.multiline,true);assert.equal(node.readOnly,true);assert.deepEqual(node.selectionUtf16,[7,5]);assert.deepEqual(Array.from(view.text_selection_utf16()),[6,4]);assert.equal(node.value,'A😀\r\né');
 }finally{view.free();}
});
check('Direct semantic serializer preserves fractional coordinate precision and field schema',()=>{
 const view=new Runtime();try{
  view.load_component("component App { Frame { width:320.25; height:180.5; Button { key:'fraction'; width:100.3; height:30.7; } } }","component Button { Rectangle { PointerArea { clicked -> events.clicked(); } } }");const tree=JSON.parse(view.accessibility_tree_json()),node=tree.nodes.find(n=>n.key==='fraction');assert.equal(node.bounds[2],Math.fround(100.3));assert.equal(node.bounds[3],Math.fround(30.7));assert.equal(node.selectionUtf16,null);assert.equal(node.readOnly,false);assert.equal(node.multiline,false);assert.equal('numericValue' in node,false);assert.equal(tree.rootId,tree.nodes[0].id);
 }finally{view.free();}
});
check('Semantic editor epoch changes on model/selection but not visual tick/preedit',()=>{
 const view=new Runtime();try{
  view.load_ui_files_json(JSON.stringify({'App.ui':"component App { Column { TextInput { value:'seed'; } } }"}),'App.ui','{}');view.focus_control(0);const initial=view.text_model_revision();view.tick(100);assert.equal(view.text_model_revision(),initial);view.text_preedit('marked');assert.equal(view.text_model_revision(),initial);view.text_key('ArrowLeft',true,false);assert.notEqual(view.text_model_revision(),initial);const selected=view.text_model_revision();view.text_insert('X');assert.notEqual(view.text_model_revision(),selected);
 }finally{view.free();}
});
check('Semantic selection action roundtrips original UTF16 through WASM including readonly',()=>{
 for(const readOnly of [false,true]){const view=new Runtime();try{
  view.load_ui_files_json(JSON.stringify({'App.ui':"component App { state query: String = ''; Column { TextInput { key:'editor'; multiline:true; readOnly:"+readOnly+"; value <-> state.query; } } }"}),'App.ui',JSON.stringify({query:'A😀\r\né'}));
  const node=JSON.parse(view.accessibility_tree_json()).nodes.find(n=>n.key==='editor');assert(node.actions.includes('set_text_selection'));
  assert(view.accessibility_set_text_selection(node.id,7,5));assert.equal(view.text_selected(),'é');assert.deepEqual(Array.from(view.text_selection_utf16()),[6,4]);
  const selected=JSON.parse(view.accessibility_tree_json()).nodes.find(n=>n.id===node.id);assert.deepEqual(selected.selectionUtf16,[7,5]);assert(selected.focused);
  assert(view.accessibility_set_text_selection(node.id,2,2));assert.deepEqual(Array.from(view.text_selection_utf16()),[3,3]);
  assert(view.accessibility_set_text_selection(node.id,0xffffffff,0xffffffff));assert.deepEqual(Array.from(view.text_selection_utf16()),[6,6]);assert.equal(view.text_value(0),'A😀\r\né');
 }finally{view.free();}}
});
check('Semantic selection action refuses unknown disabled and noneditor nodes across WASM',()=>{
 const view=new Runtime();try{
  view.load_ui_files_json(JSON.stringify({'App.ui':"component App { Column { TextInput { key:'locked'; disabled:true; value:'seed'; } Text { key:'other'; text:'Other'; } } }"}),'App.ui','{}');
  const tree=JSON.parse(view.accessibility_tree_json());const locked=tree.nodes.find(n=>n.key==='locked'),other=tree.nodes.find(n=>n.key==='other');
  const focus=view.focused_index();assert.equal(view.accessibility_set_text_selection(locked.id,0,4),false);assert.equal(view.accessibility_set_text_selection(other.id,0,1),false);assert.equal(view.accessibility_set_text_selection(0xffffffff,0,1),false);assert.equal(view.focused_index(),focus);assert.equal(view.text_value(0),'seed');
 }finally{view.free();}
});
report.status=report.cases.every(c=>c.status==='passed')?'passed':'failed';writeFileSync(process.argv[3],JSON.stringify(report,null,2)+'\n');console.log(JSON.stringify(report,null,2));if(report.status!=='passed')process.exitCode=1;
