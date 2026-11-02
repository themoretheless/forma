import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFile, writeFile} from 'node:fs/promises';
import {cpus} from 'node:os';
import {writeHeapSnapshot} from 'node:v8';
import {createWasmCoreInstance, withWasmCore, createRoot, signal, effect, onCleanup,
  createResource, captureReactiveContext, getGraphTelemetry, getCurrentOwner} from '../dist/package/index.js';
assert.equal(typeof global.gc, 'function', 'Use node --expose-gc');
const output=process.argv[2], batches=Number(process.argv[3]??1000);
assert(Number.isSafeInteger(batches)&&batches>=100&&batches<=100000&&batches%25===0);
const cores=[createWasmCoreInstance(),createWasmCoreInstance()];
const live=[], probes=[], samples=[];
let serial=0, cleanups=0, aborts=0, checked=0, maxGcRounds=0, sampleCount=0, baseline;
const windows=Array.from({length:4},()=>({count:0,memory:{}}));
const inspect=()=>cores.map(core=>withWasmCore(core,()=>({...getGraphTelemetry(),arenaFloats:core.svet_gpu_get_arena_len()})));
const report={startedAt:new Date().toISOString(),node:process.version,execArgv:process.execArgv,platform:process.platform,arch:process.arch,cpu:cpus()[0]?.model,batches,liveRoots:64,replacedPerBatch:16,samples,windows,recording:{ringSamples:32,baselineSamples:1,maxPendingWeakRefs:1600},
  scope:'Node isolated process, two cores, 64 simultaneously live roots; rolling replacement, no reset. Post-forced-GC observations, not DOM/GPU memory or a general RSS bound.'};
function mount(){
 const id=serial++,core=cores[id%2];let stop,owner,input,resource,context;
 withWasmCore(core,()=>createRoot(dispose=>{
  stop=dispose;owner=getCurrentOwner();input=signal(id);onCleanup(()=>input.dispose());
  effect(()=>{assert.equal(input(),id);});
  resource=createResource(({signal})=>{signal.addEventListener('abort',()=>aborts++,{once:true});return new Promise(()=>{});});
  context=captureReactiveContext();onCleanup(()=>cleanups++);
 }));
 return {id,core,stop,owner,input,resource,context};
}
function retire(entry){
 for(const kind of ['owner','input','resource','context'])probes.push({id:entry.id,kind,ref:new WeakRef(entry[kind])});
 withWasmCore(entry.core,entry.stop);assert.equal(entry.context.active,false);
}
function rotate(){
 for(let i=0;i<16;i++){const fresh=mount();retire(live.shift());live.push(fresh);}
}
async function sample(batch){
 await new Promise(setImmediate);
 let remaining;
 for(let round=1;round<=10;round++){
  global.gc();await new Promise(setImmediate);
  remaining=probes.reduce((n,probe)=>n+(probe.ref.deref()!==undefined),0);maxGcRounds=Math.max(maxGcRounds,round);
  if(!remaining)break;
  await new Promise(setImmediate);
 }
 const current={batch,elapsedMs:performance.now()-started,...process.memoryUsage(),cores:inspect(),pendingWeakRefs:probes.length,remaining};
 if(batch===0){baseline=current;report.baseline=baseline;}
 samples[sampleCount++%32]=current;
 if(batch>0&&batch<=batches){
  const window=windows[Math.min(3,Math.floor((batch-1)*4/batches))];window.count++;
  for(const key of ['rss','heapUsed','external','arrayBuffers']){
   const stat=window.memory[key]??={min:Infinity,max:-Infinity,mean:0,first:current[key],last:0};
   stat.min=Math.min(stat.min,current[key]);stat.max=Math.max(stat.max,current[key]);
   stat.mean+=(current[key]-stat.mean)/window.count;stat.last=current[key];
  }
 }
 if(remaining){report.retained=probes.filter(p=>p.ref.deref()!==undefined).map(({id,kind})=>({id,kind}));if(process.env.SVET_LIVE_ROOTS_HEAP){
   writeHeapSnapshot(process.env.SVET_LIVE_ROOTS_HEAP);
   await new Promise(setImmediate);global.gc();await new Promise(setImmediate);
   report.afterSnapshotRemaining=probes.reduce((n,p)=>n+(p.ref.deref()!==undefined),0);
  }}
 assert.equal(remaining,0,'Retired root objects retained after ten GC turns');
 checked+=probes.length;probes.length=0;
 if(batch>0&&batch!==batches+1){
  current.cores.forEach((state,i)=>{
   assert.equal(state.nodes,baseline.cores[i].nodes);assert.equal(state.edges,baseline.cores[i].edges);
   assert.equal(state.capacity,baseline.cores[i].capacity);assert.equal(state.wasmMemoryBytes,baseline.cores[i].wasmMemoryBytes);
   assert.equal(state.pendingEffectCount,0);
  });
 }
}
const started=performance.now();
try{
 for(let i=0;i<64;i++)live.push(mount());
 // Reach the overlapping old/new allocation high-water mark before baseline.
 rotate();
 await sample(0);
 for(let batch=1;batch<=batches;batch++){
  rotate();
  if(batch%25===0)await sample(batch);
 }
 while(live.length)retire(live.pop());
 await sample(batches+1);
 for(const state of inspect()){assert.equal(state.nodes,0);assert.equal(state.edges,0);assert.equal(state.pendingEffectCount,0);}
 assert.equal(cleanups,serial);assert.equal(aborts,serial);assert.equal(checked,serial*4);
 report.status='passed';
}catch(error){report.status='failed';report.error=String(error.stack??error);process.exitCode=1;}
finally{
 samples.sort((a,b)=>a.batch-b.batch);
 report.completedAt=new Date().toISOString();Object.assign(report,{sampleCount,retainedSamples:samples.length});Object.assign(report,{createdRoots:serial,cleanups,aborts,checkedObjects:checked,maxGcRounds});
 report.sourceSha256=createHash('sha256').update(await readFile(new URL(import.meta.url))).digest('hex');
 report.runtimeSha256=createHash('sha256').update(await readFile(new URL('../dist/package/reactivity.js',import.meta.url))).digest('hex');
 if(output)await writeFile(output,JSON.stringify(report,null,2)+'\n');
 console.log(JSON.stringify({status:report.status,error:report.error,createdRoots:serial,checkedObjects:checked,samples:samples.length,maxGcRounds}));
}
