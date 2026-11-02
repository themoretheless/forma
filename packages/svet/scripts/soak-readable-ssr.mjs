import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {createHash} from 'node:crypto';
import {resolve} from 'node:path';
import {pathToFileURL} from 'node:url';
import {createResumptionFixture} from './browser-fixture-endpoints.mjs';
const base=process.argv[4]?pathToFileURL(resolve(process.argv[4],'dist/package')+'/'):new URL('../dist/package/',import.meta.url);
const e=await import(new URL('index.js',base)),r=await import(new URL('server.js',base)),host=createResumptionFixture(e);
const durationMs=Number(process.argv[3]??60000),concurrency=16;assert(Number.isSafeInteger(durationMs)&&durationMs>=1000&&durationMs<=3600000);assert.equal(typeof global.gc,'function');
const report={startedAt:new Date().toISOString(),node:process.version,durationMs,concurrency,requests:0,collectedObjects:0,modeCounts:{complete:0,cancel:0,abort:0,error:0},samples:[],windows:Array.from({length:4},()=>({count:0,heapMin:Infinity,heapMax:0,rssMin:Infinity,rssMax:0})),scope:'Concurrent Node Web ReadableStream requests in 16-request barriers, both codecs, four terminal modes; retain one batch of terminal stream/reader handles while GC-checking request objects. No HTTP/browser/Edge, universal throughput/RSS bound or arbitrary error graph qualification.'};
let held=[],probes=[],sampleIndex=0,batch=0;
const track=(kind,value)=>probes.push({kind,ref:new WeakRef(value)});
const gate=()=>{let release;const promise=new Promise(resolve=>release=resolve);return {promise,release};};
async function runBatch(){
 const reason=Error('controlled concurrent component failure'),rows=[];
 for(let i=0;i<concurrency;i++){
  const id=batch*concurrency+i,mode=['complete','cancel','abort','error'][i%4],codec=(i>>2)%2?'svet-html-ref-v1':'identity',wait=gate(),abort=new AbortController(),marker={id};track('capture',marker);
  const row={id,mode,codec,wait,abort,cleaned:0,core:null};rows.push(row);
  const view=()=>{
   assert.equal(marker.id,id);row.core=e.getWasmCore();track('core',row.core);track('memory',row.core.memory);const source=e.signal(id);track('signal',source);e.onCleanup(source.dispose);e.onCleanup(()=>row.cleaned++);
   const node=e.h('main',null,e.h('b',null,String(id)),e.h(async()=>{await wait.promise;if(mode==='error')throw reason;assert.equal(source(),id);return e.h('p',null,'request-'+id+'-'+('x'.repeat(1024)));},null));track('node',node);return {node};
  };track('view',view);
  row.stream=r.renderToReadableStreamAsync(view,{codec,signal:abort.signal});row.reader=row.stream.getReader();row.closed=row.reader.closed.then(()=>({ok:true}),error=>({error}));
 }
 await Promise.all(rows.map(async row=>{const first=await row.reader.read();assert(!first.done);row.shell=JSON.parse(new TextDecoder().decode(first.value));assert(row.shell.payload.html.includes('<b>'+row.id+'</b>'));}));
 assert.equal(new Set(rows.map(row=>row.core)).size,concurrency);assert(rows.every(row=>row.core.svet_get_node_count()>0));
 await Promise.all(rows.filter(row=>row.mode==='cancel'||row.mode==='abort').map(async row=>{if(row.mode==='cancel')await row.reader.cancel();else{row.abort.abort('controlled disconnect');assert.equal((await row.closed).error,'controlled disconnect');}}));
 await new Promise(setImmediate);
 for(const row of rows)assert.equal(row.core.svet_get_node_count()>0,row.mode==='complete'||row.mode==='error','Cancellation affected a neighbouring request');
 await Promise.all(rows.map(async row=>{
  if(row.mode==='error'){row.wait.release();assert.equal((await row.closed).error,reason);}
  else if(row.mode==='complete'){
   row.wait.release();const decoder=row.codec==='identity'?null:e.createSsrStreamTransportDecoder();const chunks=[decoder?decoder.decode(row.shell):row.shell];
   while(true){const part=await row.reader.read();if(part.done)break;const frame=JSON.parse(new TextDecoder().decode(part.value));chunks.push(decoder?decoder.decode(frame):frame);}
   decoder?.dispose();assert.deepEqual(chunks.map(c=>c.sequence),[0,1,2]);assert(chunks[2].payload.html.includes('request-'+row.id+'-'));assert((await row.closed).ok);
  }
  row.wait.release();await new Promise(setImmediate);assert.equal(row.cleaned,1);assert.equal(row.core.svet_get_node_count(),0);report.modeCounts[row.mode]++;
 }));
 held=rows.map(({stream,reader})=>({stream,reader}));report.requests+=concurrency;
}
async function bounded(promise){let timer;try{return await Promise.race([promise,new Promise((_,reject)=>timer=setTimeout(()=>reject(Error('Concurrent readable barrier deadline')),10000))]);}finally{clearTimeout(timer);}}
try{
 const started=performance.now();
 do{
  await bounded(runBatch());batch++;await new Promise(setImmediate);
  let alive=[];
  for(let round=0;round<10;round++){global.gc();await new Promise(setImmediate);alive=probes.filter(p=>p.ref.deref());if(!alive.length)break;await new Promise(setImmediate);}
  assert.equal(alive.length,0,'Concurrent terminal handles retained request objects');report.collectedObjects+=probes.length;probes=[];
  const elapsedMs=performance.now()-started,memory=process.memoryUsage();report.samples[sampleIndex++%32]={elapsedMs,requests:report.requests,...memory};
  const window=report.windows[Math.min(3,Math.floor(elapsedMs*4/durationMs))];window.count++;window.heapMin=Math.min(window.heapMin,memory.heapUsed);window.heapMax=Math.max(window.heapMax,memory.heapUsed);window.rssMin=Math.min(window.rssMin,memory.rss);window.rssMax=Math.max(window.rssMax,memory.rss);
 }while(performance.now()-started<durationMs);
 report.elapsedMs=performance.now()-started;report.retainedTerminalHandles=held.length;assert.equal(e.getGraphTelemetry().nodes,0);report.status='passed';
}catch(error){report.status='failed';report.error=String(error.stack??error);process.exitCode=1;}
finally{held=[];host.close();report.completedAt=new Date().toISOString();report.sourceSha256=createHash('sha256').update(await readFile(new URL(import.meta.url))).digest('hex');report.serverRuntimeSha256=createHash('sha256').update(await readFile(new URL('server.js',base))).digest('hex');if(process.argv[2])await writeFile(process.argv[2],JSON.stringify(report,null,2)+'\n');console.log(JSON.stringify({status:report.status,requests:report.requests,collectedObjects:report.collectedObjects,error:report.error}));}
