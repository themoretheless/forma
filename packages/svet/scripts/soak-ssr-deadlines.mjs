import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {createHash} from 'node:crypto';
import {resolve} from 'node:path';
import {pathToFileURL} from 'node:url';
import {createResumptionFixture} from './browser-fixture-endpoints.mjs';
const base=process.argv[4]?pathToFileURL(resolve(process.argv[4],'dist/package')+'/'):new URL('../dist/package/',import.meta.url);
const e=await import(new URL('index.js',base)),r=await import(new URL('server.js',base)),host=createResumptionFixture(e);
const durationMs=Number(process.argv[3]??60000),concurrency=16;assert(Number.isSafeInteger(durationMs)&&durationMs>=1000&&durationMs<=3600000);assert.equal(typeof global.gc,'function');
const neighborDelayMs=Number(process.env.SVET_DEADLINE_NEIGHBOR_DELAY_MS??0);assert(Number.isSafeInteger(neighborDelayMs)&&neighborDelayMs>=0&&neighborDelayMs<=1000);
const report={neighborDelayMs,startedAt:new Date().toISOString(),node:process.version,durationMs,concurrency,requests:0,collectedObjects:0,modeCounts:{complete:0,cancel:0,abort:0,timeout:0},earlyTimeoutsAtNeighborCheck:0,neighborChecks:[],samples:[],windows:Array.from({length:4},()=>({count:0,heapMin:Infinity,heapMax:0,rssMin:Infinity,rssMax:0})),scope:'Concurrent Node deadline churn: 16 live requests, both codecs, complete/cancel/caller abort/timeout. Keep neighbours alive until cancellation has been checked; retain terminal errors/handles during each GC barrier. Samples are observational, not HTTP/Edge/browser or multi-hour memory qualification.'};
const nativeSet=globalThis.setTimeout,nativeClear=globalThis.clearTimeout,timers=new Set();let peakTimers=0;
globalThis.setTimeout=(callback,delay,...args)=>{const timer=nativeSet(()=>{timers.delete(timer);callback(...args);},delay);timers.add(timer);peakTimers=Math.max(peakTimers,timers.size);return timer;};
globalThis.clearTimeout=timer=>{timers.delete(timer);return nativeClear(timer);};
let held=[],probes=[],sampleIndex=0,batch=0;
const track=(kind,value)=>probes.push({kind,ref:new WeakRef(value)});
const gate=()=>{let release;const promise=new Promise(resolve=>release=resolve);return {promise,release};};
async function runBatch(){
 const rows=[];
 for(let i=0;i<concurrency;i++){
  const id=batch*concurrency+i,mode=['complete','cancel','abort','timeout'][i%4],codec=(i>>2)%2?'svet-html-ref-v1':'identity',wait=gate(),abort=new AbortController(),marker={id};track('capture',marker);
  const listeners=new Set(),add=abort.signal.addEventListener.bind(abort.signal),remove=abort.signal.removeEventListener.bind(abort.signal);
  abort.signal.addEventListener=(type,callback,options)=>{if(type==='abort')listeners.add(callback);return add(type,callback,options);};
  abort.signal.removeEventListener=(type,callback,options)=>{if(type==='abort')listeners.delete(callback);return remove(type,callback,options);};
  const row={id,mode,codec,wait,abort,listeners,cleaned:0,core:null,startedAt:performance.now()};rows.push(row);
  const view=()=>{
   assert.equal(marker.id,id);row.core=e.getWasmCore();track('core',row.core);track('memory',row.core.memory);const source=e.signal(id);track('signal',source);e.onCleanup(source.dispose);e.onCleanup(()=>row.cleaned++);
   const node=e.h('main',null,e.h('b',null,String(id)),e.h(async()=>{await wait.promise;assert.equal(source(),id);return e.h('p',null,'request-'+id+'-'+('x'.repeat(1024)));},null));track('node',node);return {node};
  };track('view',view);
  row.stream=r.renderToReadableStreamAsync(view,{codec,signal:abort.signal,timeoutMs:mode==='timeout'?100:10000});row.reader=row.stream.getReader();row.closed=row.reader.closed.then(()=>({ok:true}),error=>({error}));
 }
 await Promise.all(rows.map(async row=>{const first=await row.reader.read();assert(!first.done);row.shell=JSON.parse(new TextDecoder().decode(first.value));assert(row.shell.payload.html.includes('<b>'+row.id+'</b>'));}));
 assert.equal(new Set(rows.map(row=>row.core)).size,concurrency);assert(rows.every(row=>row.core.svet_get_node_count()>0));
 await Promise.all(rows.filter(row=>row.mode==='cancel'||row.mode==='abort').map(async row=>{if(row.mode==='cancel')await row.reader.cancel();else{row.abort.abort('controlled disconnect');assert.equal((await row.closed).error,'controlled disconnect');}}));
 await new Promise(setImmediate);
 if(neighborDelayMs)await new Promise(resolve=>nativeSet(resolve,neighborDelayMs));
 report.neighborChecks=[];
 for(const row of rows){
  const alive=row.core.svet_get_node_count()>0;
  report.neighborChecks.push({id:row.id,mode:row.mode,alive,elapsedMs:performance.now()-row.startedAt});
  if(row.mode==='timeout'&&!alive){
   // Wall-clock deadlines may legitimately expire while the cancellation barrier
   // yields. Require this request's exact deadline outcome, not blanket liveness.
   const error=(await row.closed).error;assert(error instanceof r.SsrTimeoutError);
   assert.equal(error.timeoutMs,100);assert.equal(row.cleaned,1);assert.equal(row.listeners.size,0);
   report.earlyTimeoutsAtNeighborCheck++;
  }else assert.equal(alive,row.mode==='complete'||row.mode==='timeout','Cancellation affected neighbour '+row.id+' ('+row.mode+')');
 }
 await Promise.all(rows.map(async row=>{
  if(row.mode==='timeout'){const error=(await row.closed).error;assert(error instanceof r.SsrTimeoutError);assert.equal(error.timeoutMs,100);row.failure=error;}
  else if(row.mode==='complete'){
   row.wait.release();const decoder=row.codec==='identity'?null:e.createSsrStreamTransportDecoder();const chunks=[decoder?decoder.decode(row.shell):row.shell];
   while(true){const part=await row.reader.read();if(part.done)break;const frame=JSON.parse(new TextDecoder().decode(part.value));chunks.push(decoder?decoder.decode(frame):frame);}
   decoder?.dispose();assert.deepEqual(chunks.map(c=>c.sequence),[0,1,2]);assert(chunks[2].payload.html.includes('request-'+row.id+'-'));assert((await row.closed).ok);
  }
  row.wait.release();await new Promise(setImmediate);assert.equal(row.cleaned,1);assert.equal(row.core.svet_get_node_count(),0);assert.equal(row.listeners.size,0,'Caller abort listeners were not removed');report.modeCounts[row.mode]++;
 }));
 held=rows.map(({stream,reader,failure})=>({stream,reader,failure}));report.requests+=concurrency;
}
async function bounded(promise){let timer;try{return await Promise.race([promise,new Promise((_,reject)=>timer=setTimeout(()=>reject(Error('Concurrent readable barrier deadline')),10000))]);}finally{clearTimeout(timer);}}
try{
 const started=performance.now();
 do{
  await bounded(runBatch());assert.equal(timers.size,0,'Timeout handles survived stream teardown');batch++;await new Promise(setImmediate);
  let alive=[];
  for(let round=0;round<10;round++){global.gc();await new Promise(setImmediate);alive=probes.filter(p=>p.ref.deref());if(!alive.length)break;await new Promise(setImmediate);}
  assert.equal(alive.length,0,'Concurrent terminal handles retained request objects');report.collectedObjects+=probes.length;probes=[];
  const elapsedMs=performance.now()-started,memory=process.memoryUsage();report.samples[sampleIndex++%32]={elapsedMs,requests:report.requests,...memory};
  const window=report.windows[Math.min(3,Math.floor(elapsedMs*4/durationMs))];window.count++;window.heapMin=Math.min(window.heapMin,memory.heapUsed);window.heapMax=Math.max(window.heapMax,memory.heapUsed);window.rssMin=Math.min(window.rssMin,memory.rss);window.rssMax=Math.max(window.rssMax,memory.rss);
 }while(performance.now()-started<durationMs);
 report.elapsedMs=performance.now()-started;report.retainedTerminalHandles=held.length;report.activeTimers=timers.size;report.peakTimers=peakTimers;assert.equal(e.getGraphTelemetry().nodes,0);report.status='passed';
}catch(error){report.status='failed';report.error=String(error.stack??error);process.exitCode=1;}
finally{for(const timer of timers)nativeClear(timer);globalThis.setTimeout=nativeSet;globalThis.clearTimeout=nativeClear;held=[];host.close();report.completedAt=new Date().toISOString();report.sourceSha256=createHash('sha256').update(await readFile(new URL(import.meta.url))).digest('hex');report.serverRuntimeSha256=createHash('sha256').update(await readFile(new URL('ssr-runtime.js',base))).digest('hex');if(process.argv[2])await writeFile(process.argv[2],JSON.stringify(report,null,2)+'\n');console.log(JSON.stringify({status:report.status,requests:report.requests,collectedObjects:report.collectedObjects,error:report.error}));}
