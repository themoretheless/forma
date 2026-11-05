import assert from 'node:assert/strict';
import {mkdtemp,readFile,writeFile} from 'node:fs/promises';
import {resolve} from 'node:path';
import {tmpdir} from 'node:os';
import {pathToFileURL} from 'node:url';
import {createHash} from 'node:crypto';
import {JSDOM} from 'jsdom';
assert(global.gc,'Use --expose-gc');const [installation,output]=process.argv.slice(2);assert(installation&&output);
const entry=pathToFileURL(resolve(installation,'dist/package/index.js')).href;
const engine=await import(entry),runtime=await import(pathToFileURL(resolve(installation,'dist/package/server.js')).href);
const dom=new JSDOM('<!doctype html><body></body>');for(const key of ['window','document','Node','Element'])globalThis[key]=dom.window[key];
const directory=await mkdtemp(resolve(tmpdir(),'svet-aot-lifecycle-')),source="component AotCard { state count: Number = 1; Column { Text { text: state.count; } Button { text: 'increment'; clicked -> state.increment('count', 1); } } }";
const code=engine.compileFormaToJsModule(source,entry),report={cases:[],node:process.version,generatedSha256:createHash('sha256').update(code).digest('hex')};
async function check(name,body){try{await body();report.cases.push({name,status:'passed'});}catch(error){report.cases.push({name,status:'failed',error:String(error.stack??error)});}}
async function load(name,hot){const path=resolve(directory,name+'.mjs');await writeFile(path,(hot?"import.meta.hot=globalThis[Symbol.for('svet.test.aot.hot')];\n":'')+code);return import(pathToFileURL(path).href);}
await check('production factory releases never-mounted disposed instances',async()=>{
 const mod=await load('production',false),refs=[];let controls=Array.from({length:4},()=>({live:true}));const controlRefs=controls.map(x=>new WeakRef(x));
 for(let i=0;i<4;i++)await runtime.runWithIsolatedContextAsync(()=>{const core=engine.getWasmCore();refs.push(new WeakRef(core),new WeakRef(core.memory));const inst=mod.default();inst.dispose();assert.equal(engine.getGraphTelemetry().nodes,0);});
 async function collect(rows){for(let i=0;i<12;i++){await new Promise(setImmediate);global.gc();await new Promise(setImmediate);if(!rows.some(ref=>ref.deref()))return 0;}return rows.filter(ref=>ref.deref()).length;}
 report.retainedTargets=await collect(refs);assert.equal(controlRefs.filter(ref=>ref.deref()).length,4);controls=null;assert.equal(await collect(controlRefs),0);assert.equal(report.retainedTargets,0,'Factory module retains disposed cores/Memory');report.gc={targets:8,controlsHeld:4,controlsReleased:0};
});
await check('HMR tracks mounted instances and preserves registration until last unmount',async()=>{
 let accept;const hot={data:{},accept(callback){accept=callback;}};globalThis[Symbol.for('svet.test.aot.hot')]=hot;
 const mod=await load('hmr',true),inst=mod.default();let left,right;
 try{
  assert.equal(hot.data.instances.size,0,'Never-mounted instance was HMR-active');
  const a=document.createElement('div'),b=document.createElement('div');left=inst.mount(a);right=inst.mount(b);assert.equal(hot.data.instances.size,1);
  left.unmount();left.unmount();assert.equal(hot.data.instances.size,1,'First unmount retired another live view');
  inst.actions.increment('count',1);assert.match(b.textContent,/2/);accept({uiAst:engine.parseFormaSource(source.replace('text: state.count;', "text: 'Reloaded ${state.count}';"))});assert.match(b.textContent,/Reloaded 2/);assert.equal(inst.state.count(),2);
  right.unmount();assert.equal(hot.data.instances.size,0);inst.dispose();assert.equal(engine.getGraphTelemetry().nodes,0);
 }finally{left?.unmount();right?.unmount();inst.dispose();delete globalThis[Symbol.for('svet.test.aot.hot')];}
});
await check('HMR explicit disposal retires live views before handles unmount',async()=>{
 const hot={data:{},accept(){}};globalThis[Symbol.for('svet.test.aot.hot')]=hot;
 const mod=await load('disposed',true),inst=mod.default();let left,right;
 const a=document.createElement('div'),b=document.createElement('div'),c=document.createElement('div');
 try{
  left=inst.mount(a);right=inst.mount(b);c.append(inst.render());assert.equal(hot.data.instances.size,1);
  inst.dispose();inst.dispose();assert.equal(hot.data.instances.size,0);assert.equal(engine.getGraphTelemetry().nodes,0);
  assert.equal(a.childNodes.length+b.childNodes.length+c.childNodes.length,0);
  a.textContent='replacement';left.unmount();right.unmount();assert.equal(a.textContent,'replacement');
  assert.throws(()=>inst.mount(a),/disposed/);assert.throws(()=>inst.render(),/disposed/);assert.throws(()=>inst.hotReload(source),/disposed/);
 }finally{left?.unmount();right?.unmount();inst.dispose();delete globalThis[Symbol.for('svet.test.aot.hot')];}
});
await check('cached factory uses the newest AST even when no instance was mounted during update',async()=>{
 let accept;const hot={data:{},accept(callback){accept=callback;}};globalThis[Symbol.for('svet.test.aot.hot')]=hot;
 const mod=await load('future-factory',true);let inst,handle;const container=document.createElement('div');
 try{accept({uiAst:engine.parseFormaSource(source.replace('text: state.count;', "text: 'Updated ${state.count}';"))});
  inst=mod.default();handle=inst.mount(container);assert.match(container.textContent,/Updated 1/);
 }finally{handle?.unmount();inst?.dispose();delete globalThis[Symbol.for('svet.test.aot.hot')];}
 assert.equal(engine.getGraphTelemetry().nodes,0);
});
await check('HMR drains healthy instances after one failure and defers newly mounted instances',async()=>{
 let accept;const hot={data:{},accept(callback){accept=callback;}};globalThis[Symbol.for('svet.test.aot.hot')]=hot;
 const mod=await load('accept-failure',true),first=mod.default(),second=mod.default(),a=document.createElement('div'),b=document.createElement('div'),c=document.createElement('div');
 const left=first.mount(a),right=second.mount(b),failure=Error('one instance failed');let late,lateHandle,lateUpdates=0;
 try{first.hotReload=()=>{late=mod.default();lateHandle=late.mount(c);late.hotReload=()=>{lateUpdates++;};throw failure;};
  assert.throws(()=>accept({uiAst:engine.parseFormaSource(source.replace('text: state.count;', "text: 'Updated ${state.count}';"))}),error=>error===failure);
  assert.match(b.textContent,/Updated 1/);assert.match(c.textContent,/Updated 1/);assert.equal(lateUpdates,0);
 }finally{left.unmount();right.unmount();lateHandle?.unmount();first.dispose();second.dispose();late?.dispose();delete globalThis[Symbol.for('svet.test.aot.hot')];}
 assert.equal(engine.getGraphTelemetry().nodes,0);
});

await check('installed parser and validator refuse unsupported contract placement',async()=>{
 for(const body of [
  '#[ragne(0,10)] prop x: Number = 2; Column {}',
  '#[range(0,10)] #[range(0,5)] prop x: Number = 2; Column {}',
  '#[range(0,10)] state count: Number = 1; prop x: Number = 2; Column {}',
  'Column { #[range(0,10)] prop x: Number = 2; }',
  'Column {} #[range(0,10)] prop x: Number = 2;',
 ]){
  const invalid=`component App { ${body} }`;
  assert.throws(()=>engine.parseFormaSource(invalid),/contract/i);
  assert.equal(engine.validateFormaSource(invalid).valid,false);
 }
 const valid="component App { #[range(0,10)] prop x: Number = 2; Column { Text { text:'#[range(2,1)]'; } } }";
 assert.equal(engine.validateFormaSource(valid).valid,true);
 assert.equal(engine.parseFormaSource(valid).propConstraints[0].range[1],10);
 assert.equal(engine.getGraphTelemetry().nodes,0);
});

await check('installed AST contracts refuse malformed data before allocation and mutation',async()=>{
 const invalid=engine.parseFormaSource(source);
 invalid.propConstraints.push({name:'limit',type:'Number',required:false,defaultValue:5,range:[10,0]});
 assert.throws(()=>engine.compileFormaAst(invalid),/AST contract/);
 const cyclic=engine.parseFormaSource(source);cyclic.rootAstNodes[0].children.push(cyclic.rootAstNodes[0]);
 assert.throws(()=>engine.compileFormaAst(cyclic),/AST tree/);
 const missing=engine.parseFormaSource(source);delete missing.rootAstNodes[0].tag;
 assert.throws(()=>engine.compileFormaAst(missing),/AST tree/);
 const metadata=engine.parseFormaSource(source);metadata.styles={main:{color:12}};
 assert.throws(()=>engine.compileFormaAst(metadata),/AST document/);
 assert.equal(engine.getGraphTelemetry().nodes,0);
 const duplicate=engine.parseFormaSource(source);duplicate.letExpressions=[{name:'x',expr:'1'},{name:'x',expr:'2'}];
 assert.throws(()=>engine.compileFormaAst(duplicate),/duplicate let/);
 const reserved=engine.parseFormaSource(source);reserved.letExpressions=[{name:'__proto__',expr:'1'}];
 assert.throws(()=>engine.compileFormaAst(reserved),/AST document/);
 const inst=engine.compileFormaAst(engine.parseFormaSource(source)),container=document.createElement('div');
 const handle=inst.mount(container),before=container.textContent,nodes=engine.getGraphTelemetry().nodes;
 try{
  assert.throws(()=>inst.hotReload(invalid),/AST contract/);
  assert.throws(()=>inst.hotReload(cyclic),/AST tree/);
  assert.throws(()=>inst.hotReload(missing),/AST tree/);
  assert.throws(()=>inst.hotReload(metadata),/AST document/);
  assert.equal(container.textContent,before);assert.equal(engine.getGraphTelemetry().nodes,nodes);
  inst.state.count.set(4);assert.match(container.textContent,/4/);
 }finally{handle.unmount();inst.dispose();}
 assert.equal(engine.getGraphTelemetry().nodes,0);
 const own=engine.compileFormaAst(engine.parseFormaSource(source));
 try{own.hotReload('component App { state toString: Number = 7; prop valueOf: Number = 2; let hasOwnProperty = 3; Column {} }');
  own.state.toString.set(9);own.actions.reset('toString');assert.equal(own.state.toString(),7);
  assert.equal(own.props.valueOf(),2);assert.equal(own.computedLets.hasOwnProperty(),3);
 }finally{own.dispose();}
 assert.equal(engine.getGraphTelemetry().nodes,0);
});

await check('installed AST snapshot isolates caller edits across construction and HMR',async()=>{
 for(const phase of ['construction','HMR']){
  const ast=engine.parseFormaSource(source),inst=engine.compileFormaAst(phase==='construction'?ast:engine.parseFormaSource(source));
  if(phase==='HMR')inst.hotReload(ast);
  ast.rootAstNodes.length=0;ast.initialStateValues.count=100;
  const container=document.createElement('div'),handle=inst.mount(container);
  try{inst.state.count.set(4);assert.match(container.textContent,/4/);inst.actions.reset('count');assert.equal(inst.state.count(),1);}
  finally{handle.unmount();inst.dispose();}
  assert.equal(engine.getGraphTelemetry().nodes,0);
 }
});

await check('unknown AST extensions collect while the admitted instance stays mounted',async()=>{
 function construct(){
  const ast=engine.parseFormaSource(source);
  ast.propConstraints.push({name:'limit',type:'Number',required:false,defaultValue:5});
  ast.letExpressions.push({name:'doubled',expr:'state.count * 2'});
  ast.shortcuts.push({combo:'Mod+X',action:"state.increment('count',1)"});
  const arm={pattern:'_',propertyAssignments:{},childNodes:[]};ast.rootAstNodes[0].matchArms=[arm];
  const refs=[];
  for(const object of [ast,ast.rootAstNodes[0],ast.propConstraints[0],ast.letExpressions[0],ast.shortcuts[0],arm]){
   const payload={bytes:new Uint8Array(4096)};object.unusedExtension=payload;refs.push(new WeakRef(payload));
  }
  const inst=engine.compileFormaAst(ast),container=document.createElement('div'),handle=inst.mount(container);
  return {inst,container,handle,refs};
 }
 async function collect(refs){for(let n=0;n<16;n++){await new Promise(setImmediate);global.gc();await new Promise(setImmediate);if(!refs.some(ref=>ref.deref()))return 0;}return refs.filter(ref=>ref.deref()).length;}
 let controls=[{held:true},{held:true}];const controlRefs=controls.map(value=>new WeakRef(value));
 const live=construct();
 try{assert.equal(await collect(live.refs),0,'Unknown extension payload retained by mounted snapshot');
  assert.equal(controlRefs.filter(ref=>ref.deref()).length,2);controls=null;assert.equal(await collect(controlRefs),0);
  live.inst.state.count.set(4);assert.match(live.container.textContent,/4/);
  report.extensionGc={targets:6,retained:0,controlsHeld:2,controlsReleased:0};
 }finally{live.handle.unmount();live.inst.dispose();}
 assert.equal(engine.getGraphTelemetry().nodes,0);
});

await check('HMR admission reentrancy and throwing final cleanup leave the instance recoverable',async()=>{
 const text='component App { state count: Number = 1; let doubled = state.count * 2; let tripled = state.count * 3; Text { text:doubled; } }';
 const inst=engine.compileFormaAst(engine.parseFormaSource(text)),container=document.createElement('div'),handle=inst.mount(container);
 try{
  const ast=engine.parseFormaSource(text);let entered=false;
  Object.defineProperty(ast,'componentName',{get(){if(!entered){entered=true;inst.hotReload(text);}return 'App';}});
  assert.throws(()=>inst.hotReload(ast),/already in progress/);assert.equal(container.textContent,'2');
  const old=inst.computedLets.doubled,dispose=old.dispose.bind(old);old.dispose=()=>{dispose();throw Error('retired computed cleanup');};
  assert.throws(()=>inst.hotReload(text),/retired computed cleanup/);assert.doesNotThrow(()=>inst.hotReload(text));
  inst.state.count.set(4);assert.equal(container.textContent,'8');
 }finally{handle.unmount();inst.dispose();}
 assert.equal(engine.getGraphTelemetry().nodes,0);
 const closed=engine.compileFormaAst(engine.parseFormaSource(text)),ast=engine.parseFormaSource(text);
 Object.defineProperty(ast,'componentName',{get(){closed.dispose();return 'App';}});
 assert.throws(()=>closed.hotReload(ast),/Forma instance is disposed/);assert.equal(engine.getGraphTelemetry().nodes,0);
});

await check('rollback drains throwing candidate computed cleanup and preserves the old document',async()=>{
 let fail=false;const primary=Error('prepare refused');
 const text='component App { state count: Number = 1; let doubled = state.count * 2; let tripled = state.count * 3; Column { Text { text:doubled; } Child {} } }';
 const inst=engine.compileFormaAst(engine.parseFormaSource(text),{components:{Child:()=>{const child=engine.compileFormaAst(engine.parseFormaSource('component Child { Column {} }'));if(fail){const current=inst.computedLets.doubled,dispose=current.dispose.bind(current);current.dispose=()=>{dispose();throw Error('secondary cleanup');};child.render=()=>{throw primary;};}return child;}}});
 const container=document.createElement('div'),handle=inst.mount(container),old=inst.computedLets.doubled,nodes=engine.getGraphTelemetry().nodes;
 try{fail=true;assert.throws(()=>inst.hotReload(text),error=>error===primary);assert.equal(inst.computedLets.doubled,old);assert.equal(engine.getGraphTelemetry().nodes,nodes);inst.state.count.set(2);assert.equal(container.textContent,'4');fail=false;inst.hotReload(text);assert.equal(container.textContent,'4');}
 finally{fail=false;handle.unmount();inst.dispose();}assert.equal(engine.getGraphTelemetry().nodes,0);
});
await check('instance teardown drains failures and preserves construction-time ViewModel ownership',async()=>{
 const text='component App { state count: Number = 1; let doubled = state.count * 2; let tripled = state.count * 3; Text { text:doubled; } }';
 const inst=engine.compileFormaAst(engine.parseFormaSource(text)),container=document.createElement('div'),handle=inst.mount(container),old=inst.computedLets.doubled,dispose=old.dispose.bind(old);
 old.dispose=()=>{dispose();throw Error('computed dispose refused');};assert.throws(()=>inst.dispose(),/computed dispose refused/);assert.equal(engine.getGraphTelemetry().nodes,0);assert.equal(container.childNodes.length,0);assert.doesNotThrow(()=>inst.dispose());handle.unmount();
 const external=engine.signal(7),options={viewModel:{external}},owned=engine.compileFormaAst(engine.parseFormaSource('component App { state external: Number = 1; Column {} }'),options);options.viewModel={};
 try{owned.dispose();assert.equal(external(),7);assert.equal(engine.getGraphTelemetry().nodes,1);}finally{external.dispose();}
 const skipped=engine.compileFormaAst(engine.parseFormaSource('component App { state owned: Number = 1; Column {} }'),{viewModel:{owned:{}}});skipped.dispose();assert.equal(engine.getGraphTelemetry().nodes,0);
 const aliases=engine.compileFormaAst(engine.parseFormaSource('component App { state a: Number = 1; state b: Number = 2; Column {} }'));
 const a=aliases.state.a,b=aliases.state.b,da=a.dispose.bind(a),db=b.dispose.bind(b);let aCalls=0,bCalls=0;
 a.dispose=()=>{aCalls++;da();};b.dispose=()=>{bCalls++;db();};aliases.state.b=a;aliases.state.extra=a;
 aliases.dispose();assert.equal(aCalls,1);assert.equal(bCalls,1);assert.equal(engine.getGraphTelemetry().nodes,0);
 const foreign=engine.signal(9),replacement=engine.compileFormaAst(engine.parseFormaSource('component App { state owned: Number = 1; Column {} }'));
 replacement.state.owned=foreign;replacement.state.alias=foreign;
 try{replacement.dispose();assert.equal(foreign(),9);assert.equal(engine.getGraphTelemetry().nodes,1);}finally{foreign.dispose();}
 assert.equal(engine.getGraphTelemetry().nodes,0);

});

await check('prop/computed ownership survives public aliases and foreign HMR replacements',async()=>{
 for(const kind of ['props','computedLets']){
  const text='component App { prop a: Number = 1; prop b: Number = 2; let a = 1; let b = 2; Column {} }',inst=engine.compileFormaAst(engine.parseFormaSource(text)),map=inst[kind],a=map.a,b=map.b,da=a.dispose.bind(a),db=b.dispose.bind(b);let ac=0,bc=0;
  a.dispose=()=>{ac++;da();};b.dispose=()=>{bc++;db();};map.b=a;map.extra=a;inst.dispose();assert.equal(ac,1);assert.equal(bc,1);assert.equal(engine.getGraphTelemetry().nodes,0);
 }
 const external=engine.signal(7),foreign=engine.computed(()=>external()),text='component App { let a = 1; let b = 2; Column {} }',inst=engine.compileFormaAst(engine.parseFormaSource(text)),old=inst.computedLets.a,stop=old.dispose.bind(old),fs=foreign.dispose.bind(foreign);let calls=0,foreignCalls=0;
 old.dispose=()=>{calls++;stop();};foreign.dispose=()=>{foreignCalls++;fs();};inst.computedLets.a=foreign;
 try{inst.hotReload(text);assert.equal(calls,1);assert.equal(foreignCalls,0);assert.equal(foreign(),7);inst.dispose();assert.equal(foreignCalls,0);}
 finally{inst.dispose();foreign.dispose();external.dispose();}assert.equal(engine.getGraphTelemetry().nodes,0);
});

await check('options getters cannot allocate into closed instances or rerun during metadata rollback',async()=>{
 for(const existing of [false,true]){
  let close=false,inst;const props={get value(){if(close)inst.dispose();return 2;}};
  const text='component App { '+(existing?'prop value: Number = 1; ':'')+'let doubled = 2; Column {} }';
  inst=engine.compileFormaAst(engine.parseFormaSource(text),{props});const container=document.createElement('div'),handle=inst.mount(container),old=inst.computedLets.doubled,stop=old.dispose.bind(old);let calls=0;old.dispose=()=>{calls++;stop();};
  try{close=true;assert.throws(()=>inst.hotReload(existing?text:text.replace('component App {','component App { prop value: Number = 1;')),/Forma instance is disposed/);assert.equal(calls,1);assert.equal(container.childNodes.length,0);assert.equal(engine.getGraphTelemetry().nodes,0);}
  finally{close=false;handle.unmount();inst.dispose();}
 }
 let reads=0,fail=false;const primary=Error('prepare refused'),props={get value(){reads++;return 2;}},text='component App { prop value: Number = 1; let doubled = 2; Column { Child {} } }';
 const inst=engine.compileFormaAst(engine.parseFormaSource(text),{props,components:{Child:()=>{const child=engine.compileFormaAst(engine.parseFormaSource('component Child { Column {} }'));if(fail)child.render=()=>{throw primary;};return child;}}}),container=document.createElement('div'),handle=inst.mount(container),old=inst.computedLets.doubled,nodes=engine.getGraphTelemetry().nodes;
 try{fail=true;assert.throws(()=>inst.hotReload(text),error=>error===primary);assert.equal(reads,2);assert.equal(inst.computedLets.doubled,old);assert.equal(engine.getGraphTelemetry().nodes,nodes);}
 finally{fail=false;handle.unmount();inst.dispose();}assert.equal(engine.getGraphTelemetry().nodes,0);
});

await check('invalid semantic HMR preserves live state and factory AST until a valid update',async()=>{
 let accept;const hot={data:{},accept(callback){accept=callback;}};globalThis[Symbol.for('svet.test.aot.hot')]=hot;
 const mod=await load('semantic-rejection',true),inst=mod.default(),container=document.createElement('div');let handle,fresh,freshHandle;
 try{handle=inst.mount(container);inst.state.count.set(9);const before=container.textContent,nodes=engine.getGraphTelemetry().nodes;
  const invalid=source.replace('state count: Number = 1;', '#[range(100, 10)] prop limit: Number = 5; state count: Number = 1;').replace('text: state.count;', "text: 'Invalid ${state.count}';");
  const validation=engine.validateFormaSource(invalid);assert.equal(validation.valid,false);
  assert.throws(()=>engine.parseFormaSource(invalid),/Forma contract/);
  const candidate=engine.parseFormaSource(source.replace('text: state.count;', "text: 'Invalid ${state.count}';"));
  assert.throws(()=>accept({uiAst:candidate,uiValidation:validation}),/Invalid Forma HMR/);
  assert.equal(container.textContent,before);assert.equal(inst.state.count(),9);assert.equal(engine.getGraphTelemetry().nodes,nodes);
  fresh=mod.default();freshHandle=fresh.mount(document.createElement('div'));assert.equal(fresh.isValid(),true);freshHandle.unmount();fresh.dispose();
  const valid=source.replace('text: state.count;', "text: 'Recovered ${state.count}';");
  accept({uiAst:engine.parseFormaSource(valid),uiValidation:engine.validateFormaSource(valid)});assert.match(container.textContent,/Recovered 9/);
 }finally{handle?.unmount();freshHandle?.unmount();fresh?.dispose();inst.dispose();delete globalThis[Symbol.for('svet.test.aot.hot')];}
 assert.equal(engine.getGraphTelemetry().nodes,0);
});

report.status=report.cases.every(row=>row.status==='passed')?'passed':'failed';report.probeSha256=createHash('sha256').update(await readFile(new URL(import.meta.url))).digest('hex');report.compilerSha256=createHash('sha256').update(await readFile(resolve(installation,'dist/package/forma.js'))).digest('hex');await writeFile(output,JSON.stringify(report,null,2)+'\n');console.log(JSON.stringify({status:report.status,retainedTargets:report.retainedTargets,cases:report.cases}));dom.window.close();if(report.status!=='passed')process.exitCode=1;
