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

await check('invalid semantic HMR preserves live state and factory AST until a valid update',async()=>{
 let accept;const hot={data:{},accept(callback){accept=callback;}};globalThis[Symbol.for('svet.test.aot.hot')]=hot;
 const mod=await load('semantic-rejection',true),inst=mod.default(),container=document.createElement('div');let handle,fresh,freshHandle;
 try{handle=inst.mount(container);inst.state.count.set(9);const before=container.textContent,nodes=engine.getGraphTelemetry().nodes;
  const invalid=source.replace('state count: Number = 1;', '#[range(100, 10)] prop limit: Number = 5; state count: Number = 1;').replace('text: state.count;', "text: 'Invalid ${state.count}';");
  const validation=engine.validateFormaSource(invalid);assert.equal(validation.valid,false);
  assert.throws(()=>accept({uiAst:engine.parseFormaSource(invalid),uiValidation:validation}),/Invalid Forma HMR/);
  assert.equal(container.textContent,before);assert.equal(inst.state.count(),9);assert.equal(engine.getGraphTelemetry().nodes,nodes);
  fresh=mod.default();freshHandle=fresh.mount(document.createElement('div'));assert.equal(fresh.isValid(),true);freshHandle.unmount();fresh.dispose();
  const valid=source.replace('text: state.count;', "text: 'Recovered ${state.count}';");
  accept({uiAst:engine.parseFormaSource(valid),uiValidation:engine.validateFormaSource(valid)});assert.match(container.textContent,/Recovered 9/);
 }finally{handle?.unmount();freshHandle?.unmount();fresh?.dispose();inst.dispose();delete globalThis[Symbol.for('svet.test.aot.hot')];}
 assert.equal(engine.getGraphTelemetry().nodes,0);
});

report.status=report.cases.every(row=>row.status==='passed')?'passed':'failed';report.probeSha256=createHash('sha256').update(await readFile(new URL(import.meta.url))).digest('hex');report.compilerSha256=createHash('sha256').update(await readFile(resolve(installation,'dist/package/forma.js'))).digest('hex');await writeFile(output,JSON.stringify(report,null,2)+'\n');console.log(JSON.stringify({status:report.status,retainedTargets:report.retainedTargets,cases:report.cases}));dom.window.close();if(report.status!=='passed')process.exitCode=1;
