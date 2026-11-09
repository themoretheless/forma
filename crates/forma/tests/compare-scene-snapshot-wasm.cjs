const assert=require('node:assert/strict');
const {resolve}=require('node:path');
const {readFileSync,writeFileSync}=require('node:fs');
const {createHash}=require('node:crypto');
const {isDeepStrictEqual}=require('node:util');
const [before,after,output]=process.argv.slice(2);assert(before&&after&&output);const compactRoot=process.argv.includes('--compact-root');
const Old=require(resolve(before,'forma.js')).Runtime,New=require(resolve(after,'forma.js')).Runtime;
const source="component App { state items: List = []; Column { width:260; height:16384; let visible = state.items; for item in visible key item.id { TextInput { width:200; height:32; value <-> item.title; } } } }";
const files=JSON.stringify({'App.ui':source}),report={node:process.version,cases:[],wasmSha256:{before:createHash('sha256').update(readFileSync(resolve(before,'forma_bg.wasm'))).digest('hex'),after:createHash('sha256').update(readFileSync(resolve(after,'forma_bg.wasm'))).digest('hex')}};
for(const count of [8,32,96,256]){
 const state=JSON.stringify({items:Array.from({length:count},(_,i)=>({id:'id'+i,title:'title'+i}))});
 const oldJson=Old.compile_ui_scene_artifact(files,'App.ui',state),newJson=New.compile_ui_scene_artifact(files,'App.ui',state);
 const original=JSON.parse(oldJson),current=JSON.parse(newJson);assert.equal(original.source,current.source);assert.equal(original.templates,current.templates);
 let removed=0,removedRootReplayCopies=0;
 for(const control of original.controls){
  for(const frame of control.state.__binding_frames.value){const data=frame.value,locals=data.__locals.value,lexicals=data.__lexicals.value;for(const name of Object.keys(lexicals)){if(Object.hasOwn(locals,name)){assert.deepEqual(locals[name],lexicals[name]);delete locals[name];removed++;}}}
  const state=control.state;
  if(compactRoot&&state.__binding_lexicals&&state.__binding_let_steps&&state.__binding_frames.value.some(frame=>isDeepStrictEqual(frame.value.__lexicals,state.__binding_lexicals)&&isDeepStrictEqual(frame.value.__let_steps,state.__binding_let_steps))){delete state.__binding_lexicals;delete state.__binding_let_steps;removedRootReplayCopies++;}
 }
 assert.deepEqual(original,current,'Only proven redundant internal replay fields may differ');
 const oldReader=new Old();try{oldReader.load_scene_artifact(newJson);assert.equal(oldReader.control_count(),count);}finally{oldReader.free();}
 report.cases.push({controls:count,status:'passed',removedDuplicateLocals:removed,removedRootReplayCopies,beforeBytes:Buffer.byteLength(oldJson),afterBytes:Buffer.byteLength(newJson),sourceTemplatesEqual:true,allOtherFieldsEqual:true,olderReaderAccepts:true});
}
report.status='passed';writeFileSync(output,JSON.stringify(report,null,2)+'\n');console.log(JSON.stringify(report,null,2));
