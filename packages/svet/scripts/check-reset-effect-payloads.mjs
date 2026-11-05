import assert from 'node:assert/strict';
import {readFile,writeFile,readdir,mkdtemp,cp,rm,symlink} from 'node:fs/promises';
import {createHash} from 'node:crypto';
import {resolve} from 'node:path';
import {tmpdir} from 'node:os';
import {pathToFileURL,fileURLToPath} from 'node:url';
assert(global.gc,'Use --expose-gc');const [installation,output]=process.argv.slice(2);assert(installation&&output);
const directory=resolve(installation,'dist/package'),engine=await import(pathToFileURL(resolve(directory,'index.js'))),runtime=await import(pathToFileURL(resolve(directory,'server.js')));
const peerRoot=await mkdtemp(resolve(tmpdir(),'svet-reset-peer-'));await cp(directory,resolve(peerRoot,'runtime'),{recursive:true});await writeFile(resolve(peerRoot,'package.json'),'{"type":"module"}');
await symlink(fileURLToPath(new URL('../node_modules',import.meta.url)),resolve(peerRoot,'node_modules'),'dir');
const peer=await import(pathToFileURL(resolve(peerRoot,'runtime/index.js')));
const baseKinds=['effect-handle','effect-method'];
const kinds=[...baseKinds,...baseKinds.map(kind=>'peer-'+kind)],held=[],refs=[],graph=[];
const cleanupCalls=[];
function makeCleanup(payload,index){return ()=>{assert.equal(payload.marker,index);cleanupCalls.push(index);};}
function makeBody(payload,cleanup){return register=>{assert.equal(payload.marker,'body');register(cleanup);};}
let controls=Array.from({length:4},()=>({live:true}));const controlRefs=controls.map(value=>new WeakRef(value));
for(const kind of kinds)for(let n=0;n<4;n++)await runtime.runWithIsolatedContextAsync(()=>{
 const core=engine.getWasmCore();refs.push({kind,core:new WeakRef(core),memory:new WeakRef(core.memory)});
 const handleKind=kind.replace(/^peer-/, '');
 const index=refs.length-1,bodyPayload={marker:'body'},cleanupPayload={marker:index};
 refs.at(-1).body=new WeakRef(bodyPayload);refs.at(-1).cleanup=new WeakRef(cleanupPayload);
 const handle=engine.createRoot(()=>engine.effect(makeBody(bodyPayload,makeCleanup(cleanupPayload,index))));
 held.push({kind,value:handleKind==='effect-method'?handle.dispose:handle});
 if(kind.startsWith('peer-'))peer.withWasmCore(core,()=>peer.resetReactiveSystem());else engine.resetReactiveSystem();
 assert.equal(cleanupCalls.length,0);
 const fresh=engine.signal(9),values=[],observer=engine.effect(()=>values.push(fresh()));
 fresh.set(10);assert.deepEqual(values,[9,10]);observer.dispose();fresh.dispose();
 graph.push(engine.getGraphTelemetry().nodes);assert.equal(graph.at(-1),0);
});
async function collect(){for(let n=0;n<16;n++){await new Promise(setImmediate);global.gc();await new Promise(setImmediate);}}
await collect();const retained=Object.fromEntries(kinds.map(kind=>[kind,{cores:refs.filter(row=>row.kind===kind&&row.core.deref()).length,memories:refs.filter(row=>row.kind===kind&&row.memory.deref()).length,bodies:refs.filter(row=>row.kind===kind&&row.body.deref()).length,cleanups:refs.filter(row=>row.kind===kind&&row.cleanup.deref()).length}]));
assert.equal(controlRefs.filter(ref=>ref.deref()).length,4);controls=null;await collect();assert.equal(controlRefs.filter(ref=>ref.deref()).length,0);
for(const row of held){if(row.kind.endsWith('effect-handle'))row.value.dispose();else row.value();}
assert.equal(cleanupCalls.length,held.length);
await collect();const afterLateDisposal=Object.fromEntries(kinds.map(kind=>[kind,{cores:refs.filter(row=>row.kind===kind&&row.core.deref()).length,memories:refs.filter(row=>row.kind===kind&&row.memory.deref()).length,bodies:refs.filter(row=>row.kind===kind&&row.body.deref()).length,cleanups:refs.filter(row=>row.kind===kind&&row.cleanup.deref()).length}]));
const artifactSha256={};for(const name of (await readdir(directory)).sort())if(/\.(js|wasm)$/.test(name))artifactSha256[name]=createHash('sha256').update(await readFile(resolve(directory,name))).digest('hex');
const report={status:Object.values(retained).every(row=>row.bodies===0&&row.cleanups===4&&row.cores===4&&row.memories===4)&&Object.values(afterLateDisposal).every(row=>row.cores===0&&row.memories===0&&row.bodies===0&&row.cleanups===0)?'passed':'retained',retained,afterLateDisposal,graph,heldReferences:held.length,controlsHeld:4,controlsReleased:0,cleanupCalls:cleanupCalls.length,artifactSha256,probeSha256:createHash('sha256').update(await readFile(new URL(import.meta.url))).digest('hex'),node:process.version,scope:'Installed Node isolated contexts; retained reset-only effect handles/methods; obsolete body payload releases at reset, cleanup stays pending until late disposal; forced GC.'};await writeFile(output,JSON.stringify(report,null,2)+'\n');console.log(JSON.stringify({status:report.status,retained}));await rm(peerRoot,{recursive:true,force:true});if(report.status!=='passed')process.exitCode=1;
