// Exact public artifact/model/CPU frame oracle for shared evaluation scopes.
const assert=require('node:assert/strict');
const {resolve}=require('node:path');
const {readFileSync,writeFileSync}=require('node:fs');
const {createHash}=require('node:crypto');
const [baselineDir,candidateDir,output]=process.argv.slice(2);
assert(baselineDir&&candidateDir&&output);
const modules=[baselineDir,candidateDir].map(dir=>require(resolve(dir,'forma.js')));
const rows=[];
const caption="component Caption { width:180; height:32; Rectangle { background:#27334a; Text { text:'${props.__state.item.title} / ${props.__state.payload.length}'; color:#ffffff; } } }";
for(const count of [8,32,96,256]) for(const mode of ['direct','lexical','nested','shadow','snapshot']) {
 const input="TextInput { width:180; height:32; value <-> item.title; }";
 const body=mode==='nested'?`for group in state.groups key group.id { let visible = group.items; for item in visible key item.id { ${input} } }`:
  mode==='shadow'?`for item in state.items key item.id { let payload = [1,2]; ${input} }`:
  mode==='snapshot'?"for item in state.items key item.id { Caption {} }":
  `${mode==='lexical'?'let cargo = state.payload; let visible = state.items;':''} for item in ${mode==='lexical'?'visible':'state.items'} key item.id { ${input} }`;
 const source=`component App { ${mode==='snapshot'?"use 'Caption.ui';":''} state items: List = []; state groups: List = []; state payload: List = []; Column { width:240; height:120; Scroll { width:220; height:100; Column { width:200; ${body} } } } }`;
 const items=Array.from({length:count},(_,i)=>({id:`row-${i}`,title:`row-${i} — 你好 мир🙂`}));
 const initial={items:mode==='nested'?[]:items,groups:mode==='nested'?Array.from({length:count/2},(_,i)=>({id:`group-${i}`,items:items.slice(i*2,i*2+2)})):[],payload:Array.from({length:12},(_,i)=>`payload-${i}-${'unrelated'.repeat(12)}`)};
 const files=JSON.stringify({'App.ui':source,...(mode==='snapshot'?{'Caption.ui':caption}:{})});
 const artifacts=modules.map(api=>api.Runtime.compile_ui_scene_artifact(files,'App.ui',JSON.stringify(initial)));
 assert.equal(artifacts[1],artifacts[0],`artifact JSON ${mode}/${count}`);
 const views=modules.map(api=>{const view=new api.Runtime();view.load_ui_files_json(files,'App.ui',JSON.stringify(initial));assert.equal(view.control_count(),count);return view;});
 const frozen=modules.map((api,i)=>{const view=new api.Runtime();view.load_scene_artifact(artifacts[i]);return view;});
 const frames=[];const initialFrames=[];
 const compare=(stage,saveInitial=false)=>{
  assert.equal(views[1].ui_state_json(),views[0].ui_state_json());
  for(const scale of [1,1.25,2]) {
   const w=Math.ceil(240*scale),h=Math.ceil(120*scale);
   const snapshots=views.map(view=>({focus:view.focused_index(),scroll:Array.from(view.scroll_offset()),caret:Array.from(view.text_caret_bounds()),pixels:Buffer.from(view.pixels(w,h,scale))}));
   assert.deepEqual(snapshots[1],snapshots[0],`${mode}/${count}/${stage}/${scale}`);
   if(saveInitial)initialFrames.push(snapshots[0].pixels);
   frames.push({stage,scale,sha256:createHash('sha256').update(snapshots[0].pixels).digest('hex')});
  }
 };
 try {
  compare('initial',true);
  if(mode!=='snapshot') {
   for(const [index,suffix,stage] of [[0,'—first🙂','first-edit'],[count-1,'—last🙂','last-edit']]) {
    for(const view of views){assert(view.focus_control(index));view.text_key('End',false,false);assert(view.text_insert(suffix));assert.equal(view.sync_ui_bindings(),true);if(index===count-1)view.scroll(0,100000);}
    compare(stage);
   }
  }
  const state=JSON.parse(views[0].ui_state_json());
  assert.equal(state.payload.length,12);
  if(mode==='nested'){assert(state.groups[0].items[0].title.endsWith('—first🙂'));assert(state.groups.at(-1).items.at(-1).title.endsWith('—last🙂'));state.groups.reverse();for(const group of state.groups)group.items.reverse();}
  else {if(mode!=='snapshot'){assert(state.items[0].title.endsWith('—first🙂'));assert(state.items.at(-1).title.endsWith('—last🙂'));}state.items.reverse();}
  for(const view of views)view.load_ui_files_json(files,'App.ui',JSON.stringify(state));
  compare('reorder');
  for(const [i,scale] of [1,1.25,2].entries()){
   const w=Math.ceil(240*scale),h=Math.ceil(120*scale);
   const old=frozen.map(view=>Buffer.from(view.pixels(w,h,scale)));
   assert.deepEqual(old[0],initialFrames[i],`frozen artifact ${mode}/${count}/${scale}`);assert.deepEqual(old[1],old[0]);
   frames.push({stage:'frozen-after-replacement',scale,sha256:createHash('sha256').update(old[0]).digest('hex')});
  }
  rows.push({mode,count,artifactBytes:Buffer.byteLength(artifacts[0]),artifactSha256:createHash('sha256').update(artifacts[0]).digest('hex'),frames});
 }finally{views.forEach(view=>view.free());frozen.forEach(view=>view.free());}
}
const hash=dir=>createHash('sha256').update(readFileSync(resolve(dir,'forma_bg.wasm'))).digest('hex');
writeFileSync(output,JSON.stringify({status:'passed',scope:'Exact public artifact JSON, model/first+last binding edits, alias/nested scopes, props.__state custom template, reorder and frozen artifact CPU frames; no OS/GC/RSS claim',baselineSha256:hash(baselineDir),candidateSha256:hash(candidateDir),rows},null,2)+'\n');
console.log(`${rows.length} scope scenarios, ${rows.reduce((n,row)=>n+row.frames.length,0)} exact frames and frozen snapshots passed`);
