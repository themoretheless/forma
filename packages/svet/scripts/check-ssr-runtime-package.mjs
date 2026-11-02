import assert from 'node:assert/strict';
import {AsyncLocalStorage} from 'node:async_hooks';
import {randomUUID,createHash} from 'node:crypto';
import {resolve} from 'node:path';
import {pathToFileURL} from 'node:url';
import {readFile,writeFile} from 'node:fs/promises';
import {createResumptionFixture} from './browser-fixture-endpoints.mjs';
const installed=resolve(process.argv[2]),metadata=JSON.parse(await readFile(resolve(installed,'package.json'),'utf8'));
const entry=resolve(installed,metadata.exports['./server-runtime'].import),bridge=resolve(installed,'../..','ssr-runtime-public-probe.mjs');
await writeFile(bridge,"export * from 'svet-engine/server-runtime'; import * as engine from 'svet-engine'; export {engine};\n");
const {createSsrRuntime,assertSsrAsyncContextCarrier,engine:e}=await import(pathToFileURL(bridge).href),host=createResumptionFixture(e);
const report={startedAt:new Date().toISOString(),node:process.version};
try{
 const code=await readFile(entry,'utf8');assert(!/from\s+['"]node:/.test(code),'Host-neutral implementation imported Node');
 await assertSsrAsyncContextCarrier(()=>new AsyncLocalStorage());report.carrierQualified=true;
 let current;const stack={getStore:()=>current,run(value,callback){const old=current;current=value;try{return callback();}finally{current=old;}},exit(callback){const old=current;current=undefined;try{return callback();}finally{current=old;}}};
 await assert.rejects(assertSsrAsyncContextCarrier(()=>stack),/native await/);report.stackRejected=true;
 const requestTokens=[],renderTokens=[],ownerTokens=[],coreRefs=[];let carrierIndex=0,ownerRunFailure;
 const runtime=createSsrRuntime({createAsyncContext:()=>{
  const index=carrierIndex++,storage=new AsyncLocalStorage();
  return {getStore:()=>storage.getStore(),run(value,callback){if(index===0)requestTokens.push(value);if(index===2)renderTokens.push(value);if(index===1){ownerTokens.push(value);if(ownerRunFailure)throw ownerRunFailure;}return storage.run(value,callback);},exit:callback=>storage.exit(callback)};
 },randomUUID});let cleaned=0;const cores=[];
 const results=await Promise.all(Array.from({length:8},(_,i)=>runtime.renderToStringAsync(()=>{cores.push(e.getWasmCore());coreRefs.push(new WeakRef(e.getWasmCore()));return {node:e.h(async()=>{await Promise.resolve();const source=e.signal(i);e.onCleanup(()=>{source.dispose();cleaned++;});await Promise.resolve();return e.h('p',null,String(source()));},null)};})));
 assert.deepEqual(results.map(value=>value.html),Array.from({length:8},(_,i)=>`<p>${i}</p>`));assert.equal(new Set(cores).size,8);assert.equal(cleaned,8);assert(cores.every(core=>core.svet_get_node_count()===0));report.concurrentRequests=8;
 const failure=Error('retired host request');await assert.rejects(runtime.runWithIsolatedContextAsync(()=>{coreRefs.push(new WeakRef(e.getWasmCore()));throw failure;}),error=>error===failure);
 const reader=runtime.renderToReadableStreamAsync(()=>{coreRefs.push(new WeakRef(e.getWasmCore()));return {node:e.h('p',null,'stream')};}).getReader();
 while(!(await reader.read()).done){}await reader.closed;
 await assert.rejects(runtime.renderToStringAsync(()=>{coreRefs.push(new WeakRef(e.getWasmCore()));return new Promise(()=>{});},{timeoutMs:5}),/timed out/);
 const timedView=runtime.renderToReadableStreamAsync(()=>{coreRefs.push(new WeakRef(e.getWasmCore()));return new Promise(()=>{});},{timeoutMs:5}).getReader();
 const closedFailure=timedView.closed.catch(error=>error);await assert.rejects(timedView.read(),/timed out/);assert.match(String(await closedFailure),/timed out/);

 for(const mode of ['success','abort','timeout','rejection'])await runtime.runWithIsolatedContextAsync(async()=>{
  coreRefs.push(new WeakRef(e.getWasmCore()));const source=e.signal(1);e.onCleanup(source.dispose);let observed=0;
  const abort=new AbortController(),reason='child '+mode;
  const pending=runtime.runWithOwnerScopeAsync([],()=>{
   if(mode==='success'){e.effect(()=>{observed=source();});return 7;}
   if(mode==='rejection')throw reason;
   return new Promise(()=>{});
  },{signal:abort.signal,timeoutMs:mode==='timeout'?5:undefined});
  if(mode==='success')assert.equal(await pending,7);
  else {const rejected=pending.catch(error=>error);if(mode==='abort')abort.abort(reason);const error=await rejected;if(mode==='timeout')assert.match(String(error),/timed out/);else assert.equal(error,reason);}
  source.set(2);assert.equal(source(),2);if(mode==='success')assert.equal(observed,2);
 });
 const setupFailure=Error('owner carrier setup');ownerRunFailure=setupFailure;let calls=0;
 await runtime.runWithIsolatedContextAsync(async()=>{coreRefs.push(new WeakRef(e.getWasmCore()));await assert.rejects(runtime.runWithOwnerScopeAsync([],()=>calls++),error=>error===setupFailure);assert.equal(e.h('b',null,'parent').textContent,'parent');});ownerRunFailure=undefined;assert.equal(calls,0);
 assert.equal(ownerTokens.length,13);assert(ownerTokens.every(token=>token.closed&&token.core===undefined&&token.owner===undefined));
 assert.equal(requestTokens.length,17);assert(requestTokens.every(token=>token.closed&&token.core===undefined));
 assert.equal(renderTokens.length,11);assert(renderTokens.every(token=>token.closed&&token.stream===undefined&&token.pending.size===0&&token.failure===undefined));
 cores.length=0;assert.equal(typeof global.gc,'function');await new Promise(setImmediate);
 for(let i=0;i<10;i++){global.gc();await new Promise(setImmediate);}
 assert.equal(coreRefs.filter(ref=>ref.deref()).length,0,'Retained host tokens kept retired request cores');
 report.retainedHostTokens={requests:requestTokens.length,renders:renderTokens.length,owners:ownerTokens.length,coresCollected:coreRefs.length};

 assert.throws(()=>createSsrRuntime({createAsyncContext:()=>new AsyncLocalStorage(),randomUUID}),/already installed/);report.status='passed';
}catch(error){report.status='failed';report.error=String(error.stack??error);process.exitCode=1;}
finally{host.close();report.completedAt=new Date().toISOString();report.runtimeSha256=createHash('sha256').update(await readFile(entry)).digest('hex');report.scope='Installed public server-runtime export, native Node ALS injected through host contract, stack-carrier rejection, 8 concurrent async renders and retained request/render/owner tokens across completion, cancellation, timeout, rejection and setup failure. Core collection under retained host tokens; no actual Edge/browser carrier qualification.';if(process.argv[3])await writeFile(process.argv[3],JSON.stringify(report,null,2)+'\n');console.log(JSON.stringify(report,null,2));}
