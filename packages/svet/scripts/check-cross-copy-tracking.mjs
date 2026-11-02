import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {pathToFileURL} from 'node:url';
import {resolve} from 'node:path';
import {createHash} from 'node:crypto';
const latest=JSON.parse(await readFile(process.argv[3]?resolve(process.argv[3]):new URL('../node_modules/.cache/svet-event-instances/latest.json',import.meta.url),'utf8'));
const frozenBuild=JSON.parse(await readFile(resolve(latest.output,'metadata.json'),'utf8'));
for(const [path,expected] of Object.entries(frozenBuild.artifactSha256)){assert.equal(createHash('sha256').update(await readFile(resolve(latest.output,path))).digest('hex'),expected,'Frozen artifact changed: '+path);}
const a=await import(pathToFileURL(resolve(latest.output,'first/index.js')).href),b=await import(pathToFileURL(resolve(latest.output,'second/index.js')).href);
const report={startedAt:new Date().toISOString(),node:process.version,snapshot:latest.snapshot,cases:[],scope:'Two independently compiled compatible current engines in one Node realm, separate WASM cores. Explicit expected cross-copy semantics; failures remain open requirements.'};
async function check(name,body){try{await body();report.cases.push({name,status:'passed'});}catch(error){report.cases.push({name,status:'failed',error:String(error.stack??error)});}finally{a.resetReactiveSystem();b.resetReactiveSystem();}}
await check('foreign untrack pauses an existing observer',()=>{
 const source=a.signal(0);let runs=0;const observer=a.effect(()=>{runs++;b.untrack(()=>source());});source.set(1);assert.equal(runs,1);observer.dispose();source.dispose();
});
await check('foreign untrack permits newly-created observers to track',()=>{
 const source=a.signal(0);let runs=0,observer;b.untrack(()=>{observer=a.effect(()=>{source();runs++;});});source.set(1);assert.equal(runs,2);observer.dispose();source.dispose();
});
await check('foreign untrack restores collection after throw',()=>{
 const skipped=a.signal(0),tracked=a.signal(0);let runs=0;const reason=Error('scope');const observer=a.effect(()=>{runs++;assert.throws(()=>b.untrack(()=>{skipped();throw reason;}),error=>error===reason);tracked();});skipped.set(1);assert.equal(runs,1);tracked.set(1);assert.equal(runs,2);observer.dispose();skipped.dispose();tracked.dispose();
});
await check('foreign untrack does not restore retired generation frames',()=>{
 const old=a.signal(0);let fresh,inner,seen;
 const observer=a.effect(()=>{old();b.untrack(()=>{a.resetReactiveSystem();fresh=a.signal(9);inner=a.effect(()=>{seen=fresh();});});});
 fresh.set(10);assert.equal(seen,10);observer.dispose();inner.dispose();old.dispose();fresh.dispose();assert.equal(a.getGraphTelemetry().nodes,0);
});
await check('foreign untrack preserves invalidation of a dependency already read',()=>{
 const source=a.signal(0);let runs=0;const observer=a.effect(()=>{const value=source();runs++;if(value===0)b.untrack(()=>source.set(1));});assert.equal(runs,2);assert.equal(source(),1);observer.dispose();source.dispose();
});
await check('nested observers in both copies suppress incidental reads',()=>{
 const first=a.signal(0),second=b.signal(0),neededA=a.signal(0),neededB=b.signal(0);let runsA=0,runsB=0,inner;
 const outer=a.effect(()=>{runsA++;if(!inner)inner=b.effect(()=>{runsB++;a.untrack(()=>{first();second();});neededB();});neededA();});
 first.set(1);second.set(1);assert.deepEqual([runsA,runsB],[1,1]);neededA.set(1);neededB.set(1);assert.deepEqual([runsA,runsB],[2,2]);outer.dispose();inner.dispose();for(const value of [first,second,neededA,neededB])value.dispose();
});
await check('shared-core copies use native computed dependencies and one batch',()=>{
 const core=a.createWasmCoreInstance(),left=a.withWasmCore(core,()=>a.signal(1)),right=b.withWasmCore(core,()=>b.signal(2));
 const total=b.withWasmCore(core,()=>b.computed(()=>left()*2+right())),seen=[];
 const observer=a.withWasmCore(core,()=>a.effect(()=>seen.push(total())));
 a.withWasmCore(core,()=>a.batch(()=>{left.set(3);right.set(5);}));assert.deepEqual(seen,[4,11]);
 observer.dispose();total.dispose();left.dispose();right.dispose();assert.equal(a.withWasmCore(core,()=>a.getGraphTelemetry().nodes),0);
});
await check('reset in a shared-core copy invalidates foreign stale handles',()=>{
 const core=a.createWasmCoreInstance(),old=b.withWasmCore(core,()=>b.signal(1));a.withWasmCore(core,()=>a.resetReactiveSystem());
 const fresh=a.withWasmCore(core,()=>a.signal(9));old.set(99);assert.equal(old(),1);old.dispose();assert.equal(a.withWasmCore(core,()=>a.getGraphTelemetry().nodes),1);assert.equal(fresh(),9);fresh.dispose();
});
await check('shared-core owner teardown cleans foreign computeds',()=>{
 const core=a.createWasmCoreInstance();let stop;
 a.withWasmCore(core,()=>a.createRoot(dispose=>{stop=dispose;b.withWasmCore(core,()=>b.computed(()=>1));}));
 stop();assert.equal(a.withWasmCore(core,()=>a.getGraphTelemetry().nodes),0);
});
await check('shared-core reset settles a foreign resource wait',async()=>{
 const core=a.createWasmCoreInstance(),resource=b.withWasmCore(core,()=>b.createResource(()=>new Promise(()=>{})));
 const job=resource.refetch();a.withWasmCore(core,()=>a.resetReactiveSystem());let timer;
 try{assert.equal(await Promise.race([job,new Promise((_,reject)=>timer=setTimeout(()=>reject(Error('Foreign resource wait survived shared reset')),2000))]),undefined);}finally{clearTimeout(timer);}
 const fresh=a.withWasmCore(core,()=>a.signal(7));resource.dispose();assert.equal(fresh(),7);assert.equal(a.withWasmCore(core,()=>a.getGraphTelemetry().nodes),1);fresh.dispose();
});
await check('foreign signal invalidates an observer in another copy',()=>{
 const source=a.signal(0);let runs=0;const observer=b.effect(()=>{source();runs++;});source.set(1);assert.equal(runs,2);observer.dispose();source.dispose();
});
await check('nested foreign reads belong to the inner observer, not an outer source-core observer',()=>{
 const source=a.signal(0),outerNeeded=a.signal(0);let outerRuns=0,innerRuns=0,inner;
 const outer=a.effect(()=>{outerRuns++;if(!inner)inner=b.effect(()=>{source();innerRuns++;});outerNeeded();});
 source.set(1);assert.deepEqual([outerRuns,innerRuns],[1,2]);
 outerNeeded.set(1);assert.deepEqual([outerRuns,innerRuns],[2,2]);
 outer.dispose();inner.dispose();source.dispose();outerNeeded.dispose();
});
await check('independent copies coalesce mixed-core nested batch writes',()=>{
 const left=a.signal(0),right=a.signal(0),local=b.signal(0),seen=[];
 const observer=b.effect(()=>{seen.push(left()+right()+local());});
 try {a.batch(()=>{local.set(4);b.batch(()=>{left.set(1);right.set(2);});assert.deepEqual(seen,[0]);});assert.deepEqual(seen,[0,7]);}
 finally{observer.dispose();left.dispose();right.dispose();local.dispose();}
});
await check('independent-copy computed chains read current data inside a source batch',()=>{
 const source=a.signal(1),inner=b.computed(()=>source()*2),outer=b.computed(()=>inner()+1),seen=[];
 const observer=b.effect(()=>{seen.push(outer());});
 try{a.batch(()=>{source.set(2);assert.equal(outer.peek(),5);assert.deepEqual(seen,[3]);});assert.deepEqual(seen,[3,5]);}
 finally{observer.dispose();outer.dispose();inner.dispose();source.dispose();}
});
for(const batched of [false,true])for(const reverse of [false,true])await check(`three-core diamond batch=${batched} reverse=${reverse}`,()=>{
 const source=a.signal(1),left=a.computed(()=>source()*2),middle=b.createWasmCoreInstance(),target=b.createWasmCoreInstance();
 const right=b.withWasmCore(middle,()=>b.computed(()=>source()*3)),seen=[];
 const observer=b.withWasmCore(target,()=>b.effect(()=>{seen.push(reverse?right()+left():left()+right());}));
 try{if(batched)a.batch(()=>source.set(2));else source.set(2);assert.deepEqual(seen,[5,10]);}
 finally{observer.dispose();right.dispose();left.dispose();source.dispose();}
 for(const core of [middle,target])assert.equal(b.withWasmCore(core,()=>b.getGraphTelemetry().nodes),0);
});
await check('initial effect writes deliver one coherent cross-core diamond snapshot',()=>{
 const source=a.signal(1),left=a.computed(()=>source()*2),middle=b.createWasmCoreInstance(),target=b.createWasmCoreInstance();
 const right=b.withWasmCore(middle,()=>b.computed(()=>source()*3)),seen=[];
 const observer=b.withWasmCore(target,()=>b.effect(()=>{seen.push(right()+left());}));
 let writer;
 try{writer=a.effect(()=>{source.set(2);});assert.deepEqual(seen,[5,10]);}
 finally{writer?.dispose();observer.dispose();right.dispose();left.dispose();source.dispose();}
 for(const core of [middle,target])assert.equal(b.withWasmCore(core,()=>b.getGraphTelemetry().nodes),0);
});
for(const initialFailure of [false,true])await check(`foreign computed failure/recovery initial=${initialFailure}`,()=>{
 const source=a.signal(initialFailure?1:0),value=a.computed(()=>{if(source()%2)throw Error('foreign failure');return 7;});
 const seen=[],observer=b.effect(()=>{try{seen.push(value());}catch{seen.push('failed');}});
 try{if(!initialFailure){source.set(1);assert.deepEqual(seen,[7,'failed']);}source.set(2);assert.deepEqual(seen,initialFailure?['failed',7]:[7,'failed',7]);}
 finally{observer.dispose();value.dispose();source.dispose();}
});
await check('separate cores retain graph isolation after teardown',()=>{
 const first=a.signal(1),second=b.signal(2);assert.notEqual(a.getWasmCore(),b.getWasmCore());first.dispose();assert.equal(a.getGraphTelemetry().nodes,0);assert.equal(second(),2);second.dispose();assert.equal(b.getGraphTelemetry().nodes,0);
});
assert.equal(globalThis[Symbol.for('svet.reactivity.domains.v1')]?.frames.length??0,0,'Domain execution frames survived teardown');
assert.equal((globalThis[Symbol.for('svet.reactivity.observation-copies.v2')]??[]).reduce((count,ref)=>count+(ref.deref()?.length??0),0),0,'live-frame registry retained a retired context');
report.status=report.cases.every(row=>row.status==='passed')?'passed':'requirements-open';report.completedAt=new Date().toISOString();report.build=JSON.parse(await readFile(resolve(latest.output,'metadata.json'),'utf8'));report.probeSha256=createHash('sha256').update(await readFile(new URL(import.meta.url))).digest('hex');
if(process.argv[2])await writeFile(process.argv[2],JSON.stringify(report,null,2)+'\n');console.log(JSON.stringify({status:report.status,cases:report.cases},null,2));if(report.status!=='passed')process.exitCode=1;
