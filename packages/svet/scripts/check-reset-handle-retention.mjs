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
const baseKinds=['signal','signal-peek','computed','computed-peek'];
const kinds=[...baseKinds,...baseKinds.map(kind=>'peer-'+kind)],held=[],refs=[],graph=[];
let controls=Array.from({length:4},()=>({live:true}));const controlRefs=controls.map(value=>new WeakRef(value));
for(const kind of kinds)for(let n=0;n<4;n++)await runtime.runWithIsolatedContextAsync(()=>{
 const core=engine.getWasmCore();refs.push({kind,core:new WeakRef(core),memory:new WeakRef(core.memory)});
 const handleKind=kind.replace(/^peer-/, '');
 const source=engine.signal(3),derived=engine.createRoot(()=>engine.computed(()=>source()*2));assert.equal(derived(),6);
 held.push({kind,value:handleKind==='signal'?source:handleKind==='signal-peek'?source.peek:handleKind==='computed'?derived:derived.peek});
 if(kind.startsWith('peer-'))peer.withWasmCore(core,()=>peer.resetReactiveSystem());else engine.resetReactiveSystem();const fresh=engine.signal(9);source.set(100);assert.equal(fresh(),9);fresh.dispose();
 graph.push(engine.getGraphTelemetry().nodes);assert.equal(graph.at(-1),0);
});
async function collect(){for(let n=0;n<16;n++){await new Promise(setImmediate);global.gc();await new Promise(setImmediate);}}
await collect();const retained=Object.fromEntries(kinds.map(kind=>[kind,{cores:refs.filter(row=>row.kind===kind&&row.core.deref()).length,memories:refs.filter(row=>row.kind===kind&&row.memory.deref()).length}]));
assert.equal(controlRefs.filter(ref=>ref.deref()).length,4);controls=null;await collect();assert.equal(controlRefs.filter(ref=>ref.deref()).length,0);
for(const row of held)assert.equal(row.value(),row.kind.replace(/^peer-/, '').startsWith('signal')?3:6);
const artifactSha256={};for(const name of (await readdir(directory)).sort())if(/\.(js|wasm)$/.test(name))artifactSha256[name]=createHash('sha256').update(await readFile(resolve(directory,name))).digest('hex');
const report={status:Object.values(retained).every(row=>row.cores===0&&row.memories===0)?'passed':'retained',retained,graph,heldReferences:held.length,controlsHeld:4,controlsReleased:0,artifactSha256,probeSha256:createHash('sha256').update(await readFile(new URL(import.meta.url))).digest('hex'),node:process.version,scope:'Installed Node isolated contexts; retained Signal/Computed getters and peek after graph reset without explicit disposal; forced GC.'};await writeFile(output,JSON.stringify(report,null,2)+'\n');console.log(JSON.stringify({status:report.status,retained}));await rm(peerRoot,{recursive:true,force:true});if(report.status!=='passed')process.exitCode=1;
