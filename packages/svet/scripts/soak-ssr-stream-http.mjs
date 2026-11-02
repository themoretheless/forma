import assert from 'node:assert/strict';
import {createServer} from 'node:http';
import {readFile,writeFile,rename,readdir} from 'node:fs/promises';
import {createHash} from 'node:crypto';
import {resolve} from 'node:path';
import {getHeapStatistics} from 'node:v8';
import {pathToFileURL} from 'node:url';
import {createResumptionFixture,createStreamingFixture} from './browser-fixture-endpoints.mjs';
const runtimeBase=process.argv[4]?pathToFileURL(resolve(process.argv[4],'dist/package')+'/'):new URL('../dist/package/',import.meta.url);
const runtimeFiles=(await readdir(runtimeBase)).filter(file=>file.endsWith('.js')||file.endsWith('.wasm')).sort();
const hashFile=async path=>createHash('sha256').update(await readFile(path)).digest('hex');
const runtimeHashes=async()=>Object.fromEntries(await Promise.all(runtimeFiles.map(async file=>[file,await hashFile(new URL(file,runtimeBase))])));
const engine=await import(new URL('index.js',runtimeBase)),serverEngine=await import(new URL('server.js',runtimeBase));
const durationMs=Number(process.argv[3]??60000),concurrency=8;
assert(Number.isSafeInteger(durationMs)&&durationMs>=1000&&durationMs<=86400000);
const gcMode=process.env.SVET_SOAK_GC??'forced';assert(['forced','natural'].includes(gcMode),'SVET_SOAK_GC must be forced or natural');
if(gcMode==='forced')assert.equal(typeof global.gc,'function','Run node --expose-gc');
const coreLifetimeMode=process.env.SVET_SOAK_CORE_LIFETIMES??'0';assert(['0','1'].includes(coreLifetimeMode),'SVET_SOAK_CORE_LIFETIMES must be0 or1');
const coreLifetimes={created:0,finalized:0,memoryCreated:0,memoryFinalized:0,bytesRegistered:0,bytesFinalized:0,maxPendingMemories:0};
const lifetimeRegistry=coreLifetimeMode==='1'?new FinalizationRegistry(record=>{
 if(record.kind==='core')coreLifetimes.finalized++;
 else{coreLifetimes.memoryFinalized++;coreLifetimes.bytesFinalized+=record.bytes;}
}):null;
const lifetimeDiagnostics=lifetimeRegistry?{onCore(core){
 coreLifetimes.created++;coreLifetimes.memoryCreated++;const bytes=core.memory.buffer.byteLength;
 coreLifetimes.bytesRegistered+=bytes;
 coreLifetimes.maxPendingMemories=Math.max(coreLifetimes.maxPendingMemories,coreLifetimes.memoryCreated-coreLifetimes.memoryFinalized);
 lifetimeRegistry.register(core,{kind:'core'});lifetimeRegistry.register(core.memory,{kind:'memory',bytes});
}}:null;
const host=createResumptionFixture(engine),streams=createStreamingFixture({...engine,renderToChunksAsync:serverEngine.renderToChunksAsync},null,lifetimeDiagnostics);
const injectedFailure=process.env.SVET_SOAK_INJECT_FAILURE;
assert(injectedFailure===undefined||injectedFailure==='stats-and-forget','Unknown failure injection');
const sockets=new Set();let socketsOpened=0,socketsClosed=0,activeResponses=0,peakActiveResponses=0;
let closedBytesRead=0,closedBytesWritten=0;
function diagnostics(){
 const heap=getHeapStatistics(),resources={};
 for(const kind of process.getActiveResourcesInfo())resources[kind]=(resources[kind]??0)+1;
 let readableBytes=0,writableBytes=0,bytesRead=closedBytesRead,bytesWritten=closedBytesWritten;
 for(const socket of sockets){readableBytes+=socket.readableLength;writableBytes+=socket.writableLength;bytesRead+=socket.bytesRead;bytesWritten+=socket.bytesWritten;}
 return {sockets:{active:sockets.size,opened:socketsOpened,closed:socketsClosed,readableBytes,writableBytes,bytesRead,bytesWritten},
  responses:{active:activeResponses,peak:peakActiveResponses},resources,
  v8:{externalMemory:heap.external_memory,mallocedMemory:heap.malloced_memory,peakMallocedMemory:heap.peak_malloced_memory,totalPhysicalSize:heap.total_physical_size},
  mainWasmBytes:engine.getWasmCore().memory.buffer.byteLength,
  ...(lifetimeRegistry?{requestCoreLifetimes:{...coreLifetimes,pending:coreLifetimes.created-coreLifetimes.finalized,
   pendingMemories:coreLifetimes.memoryCreated-coreLifetimes.memoryFinalized,pendingRegisteredBytes:coreLifetimes.bytesRegistered-coreLifetimes.bytesFinalized,
   scope:'Finalization callback accounting; pending includes live targets and callback lag, registered bytes exclude later memory growth.'}}:{})};
}
const server=createServer((req,res)=>{
 activeResponses++;peakActiveResponses=Math.max(peakActiveResponses,activeResponses);
 res.once('close',()=>{activeResponses--;});
 const url=new URL(req.url,'http://localhost');
 if(injectedFailure&&url.searchParams.get('id')==='soak-0-0'&&['/__svet_stream/stats','/__svet_stream/forget'].includes(url.pathname)){res.writeHead(503);res.end('controlled HTTP failure');return;}
 streams.handle(req,res,()=>{res.writeHead(404);res.end();});
});
server.on('connection',socket=>{
 sockets.add(socket);socketsOpened++;
 socket.once('close',()=>{sockets.delete(socket);socketsClosed++;closedBytesRead+=socket.bytesRead;closedBytesWritten+=socket.bytesWritten;});
});
const report={coreLifetimeMode,diagnosticsVersion:1,runtimePackageBase:runtimeBase.href,runtimeFileCount:runtimeFiles.length,startedAt:new Date().toISOString(),node:process.version,durationMs,concurrency,gcMode,injectedFailure,forcedGCs:0,cleanupFailures:[],requestFailures:[],requests:0,completed:0,disconnected:0,bytesRead:0,readerPauses:0,drains:0,maxNodesAfterFinish:0,samples:[],windows:Array.from({length:4},()=>({count:0,heapMin:Infinity,heapMax:0,rssMin:Infinity,rssMax:0})),scope:'Loopback Node HTTP/fetch streaming with delayed reader calls, concurrent clients and shell disconnects. Writer drains are observed; no proxy/browser, sustained kernel-buffer saturation or universal RSS bound claim.'};
let sampleIndex=0,root,lastCheckpoint=0,started;
async function checkpoint(){if(!process.argv[2])return;report.updatedAt=new Date().toISOString();const target=process.argv[2]+'.progress.json';await writeFile(target+'.tmp',JSON.stringify(report,null,2)+'\n');await rename(target+'.tmp',target);}
const pause=ms=>new Promise(resolve=>setTimeout(resolve,ms));
function failureDetails(error){return {name:error?.name??typeof error,message:String(error?.message??error),operation:error?.operation,requestId:error?.requestId,elapsedMs:error?.elapsedMs,cause:error?.cause?{name:error.cause.name,message:String(error.cause.message??error.cause)}:undefined};}
async function request(op,id){
 const started=performance.now();
 try{const res=await fetch(`${root}${op}?id=${id}`,{signal:AbortSignal.timeout(10000)});assert(res.ok,`${op} ${res.status}`);return await res.json();}
 catch(cause){throw Object.assign(new Error(`HTTP ${op} failed for ${id}`,{cause}),{operation:op,requestId:id,elapsedMs:performance.now()-started});}
}
async function run(id,cancel){
 let reader,failure;try{
 const response=await fetch(`${root}open?id=${id}`,{signal:AbortSignal.timeout(10000)});assert(response.ok);reader=response.body.getReader();const decoder=new TextDecoder();let text='';
  while(!text.includes('\n')){const part=await reader.read();assert(!part.done);report.bytesRead+=part.value.byteLength;text+=decoder.decode(part.value,{stream:true});}
  assert.equal(JSON.parse(text.trim()).payload.type,'shell');
  if(cancel){await reader.cancel();report.disconnected++;}
  else{
   await request('release',id);
   while(true){report.readerPauses++;await pause(5);const part=await reader.read();if(part.done)break;report.bytesRead+=part.value.byteLength;text+=decoder.decode(part.value,{stream:true});}
   text+=decoder.decode();const frames=text.trim().split('\n').map(JSON.parse);
   assert.deepEqual(frames.map(c=>c.sequence),[0,1,2]);assert.deepEqual(frames.map(c=>c.payload.type),['shell','patch','complete']);
   assert(frames.every(c=>c.version===engine.SSR_STREAM_VERSION&&c.streamId===frames[0].streamId));report.completed++;
  }
  let state;for(let tries=0;tries<1000;tries++){state=await request('stats',id);if(state.finished)break;await pause(1);}
  assert(state.finished,'Server failed to finish');assert.equal(state.nodes,0);assert.equal(state.cleaned,1);
  if(!cancel)assert(state.drains>=2);report.drains+=state.drains;report.maxNodesAfterFinish=Math.max(report.maxNodesAfterFinish,state.nodes);report.requests++;
 }catch(error){failure=error;throw error;}
 finally{
  await reader?.cancel().catch(()=>{});
  try{await request('forget',id);}catch(error){
   if(report.cleanupFailures.length<8)report.cleanupFailures.push({...failureDetails(error),primary:failure?failureDetails(failure):null});
   if(!failure)throw error;
  }
 }
}
try{
 await new Promise((resolve,reject)=>{server.once('error',reject);server.listen(0,'127.0.0.1',resolve);});root=`http://127.0.0.1:${server.address().port}/__svet_stream/`;
 report.sourceSha256=await hashFile(new URL(import.meta.url));report.fixtureSha256=await hashFile(new URL('./browser-fixture-endpoints.mjs',import.meta.url));report.runtimeSha256=await runtimeHashes();
 started=performance.now();let batch=0;await checkpoint();
 do{
  const outcomes=await Promise.allSettled(Array.from({length:concurrency},(_,i)=>run(`soak-${batch}-${i}`,(batch+i)%3===0)));
  const failures=outcomes.filter(row=>row.status==='rejected');
  for(const row of failures)if(report.requestFailures.length<8)report.requestFailures.push(failureDetails(row.reason));
  if(failures.length)throw failures[0].reason;
  batch++;
  await new Promise(setImmediate);if(gcMode==='forced'){global.gc();report.forcedGCs++;await new Promise(setImmediate);}
  const elapsedMs=performance.now()-started,memory=process.memoryUsage(),row={elapsedMs,requests:report.requests,...memory,diagnostics:diagnostics()};
  report.samples[sampleIndex++%32]=row;
  const window=report.windows[Math.min(3,Math.floor(elapsedMs*4/durationMs))];window.count++;window.heapMin=Math.min(window.heapMin,memory.heapUsed);window.heapMax=Math.max(window.heapMax,memory.heapUsed);window.rssMin=Math.min(window.rssMin,memory.rss);window.rssMax=Math.max(window.rssMax,memory.rss);
  for(const key of ['external','arrayBuffers']){window[key+'Min']=Math.min(window[key+'Min']??Infinity,memory[key]);window[key+'Max']=Math.max(window[key+'Max']??0,memory[key]);}
  if(!window.externalPeakObservation||memory.external>window.externalPeakObservation.external)window.externalPeakObservation=row;
  if(!report.externalPeakObservation||memory.external>report.externalPeakObservation.external)report.externalPeakObservation=row;
  if(elapsedMs-lastCheckpoint>=5000){report.elapsedMs=elapsedMs;report.status='running';await checkpoint();lastCheckpoint=elapsedMs;}
 }while(performance.now()-started<durationMs);
 report.elapsedMs=performance.now()-started;assert.equal(engine.getGraphTelemetry().nodes,0);report.status='passed';
}catch(error){if(started!==undefined)report.elapsedMs=performance.now()-started;report.status='failed';report.error=String(error.stack??error);report.errorDetails=failureDetails(error);process.exitCode=1;}
finally{
 report.beforeShutdown={memory:process.memoryUsage(),diagnostics:diagnostics()};
 const socketClosures=[...sockets].map(socket=>new Promise(resolve=>socket.once('close',resolve)));
 streams.close();server.closeAllConnections();await new Promise(resolve=>server.close(resolve));host.close();await Promise.all(socketClosures);await new Promise(setImmediate);report.afterShutdown={memory:process.memoryUsage(),diagnostics:diagnostics()};report.completedAt=new Date().toISOString();report.finalSourceSha256=await hashFile(new URL(import.meta.url));report.finalFixtureSha256=await hashFile(new URL('./browser-fixture-endpoints.mjs',import.meta.url));report.finalRuntimeSha256=await runtimeHashes();report.artifactsUnchanged=report.sourceSha256===report.finalSourceSha256&&report.fixtureSha256===report.finalFixtureSha256&&JSON.stringify(report.runtimeSha256)===JSON.stringify(report.finalRuntimeSha256);if(!report.artifactsUnchanged){report.status='failed';report.artifactError='Probe/runtime files changed during soak';report.error??=report.artifactError;process.exitCode=1;}
 await checkpoint();if(process.argv[2])await writeFile(process.argv[2],JSON.stringify(report,null,2)+'\n');console.log(JSON.stringify(report,null,2));
}
