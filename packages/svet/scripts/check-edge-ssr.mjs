import {openWorkerdInspector} from './workerd-inspector.mjs';
import {execFileSync} from 'node:child_process';
import {get as httpGet} from 'node:http';
import {createRequire} from 'node:module';
import {resolve,dirname} from 'node:path';
import {fileURLToPath,pathToFileURL} from 'node:url';
import {mkdtemp,readFile,writeFile,copyFile} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import {createResumptionFixture} from './browser-fixture-endpoints.mjs';
import {createHash} from 'node:crypto';
if(!process.argv[2])throw Error('Usage: check-edge-ssr.mjs EDGE_DEPS_DIR [OUTPUT] [INSTALLED_PACKAGE_DIR]');
const root=resolve(fileURLToPath(new URL('..',import.meta.url))),deps=resolve(process.argv[2]),require=createRequire(resolve(deps,'package.json')),{build}=require('esbuild');
const {Miniflare,convertV4MiniflareOptions}=await import(pathToFileURL(resolve(deps,'node_modules/miniflare/dist/src/index.js')).href);
const packageRoot=process.argv[4]?resolve(process.argv[4]):root,metadata=JSON.parse(await readFile(resolve(packageRoot,'package.json'),'utf8'));
const temporary=await mkdtemp(resolve(tmpdir(),'svet-edge-probe-')),bundle=resolve(temporary,'worker.js');
const compatibilityDate=process.env.SVET_EDGE_COMPATIBILITY_DATE??'2026-07-30';
if(!/^\d{4}-\d{2}-\d{2}$/.test(compatibilityDate)||new Date(compatibilityDate+'T00:00:00Z').toISOString().slice(0,10)!==compatibilityDate)throw Error('Invalid SVET_EDGE_COMPATIBILITY_DATE');
const report={startedAt:new Date().toISOString(),node:process.version,compatibilityDate,compatibilityFlags:['nodejs_compat','enable_request_signal'],temporary,packageRoot};let mf,consumerDom,inspector;
if(process.env.MINIFLARE_WORKERD_PATH){report.workerdBinaryOverride=process.env.MINIFLARE_WORKERD_PATH;report.workerdBinaryVersion=execFileSync(process.env.MINIFLARE_WORKERD_PATH,['--version'],{encoding:'utf8'}).trim();}
function validateResponse(response){const matrix=response.result.domMatrix;if(response.httpStatus!==200||!matrix||matrix.cycles!==16||matrix.cleanedRoots!==16||matrix.cleanedRows!==64||matrix.eventTeardowns!==16||response.result.forms?.controls!==8||response.result.forms?.multiple!==true||response.result.forms?.lateOptions!==true||response.result.forms?.cleaned!==1||response.result.forms?.nodes!==0)throw Error('Edge DOM lifecycle matrix failed: '+JSON.stringify(response));}
try{
 const compiled=await build({metafile:true,entryPoints:[resolve(root,'tests/edge/worker.mjs')],outfile:bundle,bundle:true,format:'esm',platform:'browser',target:'es2022',external:['node:async_hooks'],plugins:[{name:'compiled-svet-wasm',setup(build){build.onResolve({filter:/^svet-engine-copy$/},()=>({path:resolve(packageRoot,metadata.exports['.'].import),namespace:'svet-copy'}));
 build.onResolve({filter:/^\./,namespace:'svet-copy'},args=>({path:resolve(dirname(args.importer),args.path),namespace:'svet-copy'}));
 build.onLoad({filter:/.*/,namespace:'svet-copy'},async args=>({contents:await readFile(args.path,'utf8'),loader:'js',resolveDir:dirname(args.path)}));
 build.onResolve({filter:/^svet-engine(?:\/.*)?$/},args=>{
 const key=args.path==='svet-engine'?'.':'.'+args.path.slice('svet-engine'.length),entry=metadata.exports[key];
 if(!entry)throw Error('Missing public export '+args.path);
 const path=resolve(packageRoot,entry.import??entry.default);
 return path.endsWith('.wasm')?{path:'./svet-core.wasm',external:true}:{path};
});}}],nodePaths:[resolve(deps,'node_modules')],logLevel:'silent'});
 const copyInputs=Object.keys(compiled.metafile.inputs).filter(path=>path.startsWith('svet-copy:'));report.independentEngineModules=copyInputs.length;if(!copyInputs.some(path=>path.endsWith('/reactivity.js')))throw Error('Second reactive engine module was not independently compiled');
 const asset=resolve(temporary,'svet-core.wasm');await copyFile(resolve(packageRoot,metadata.exports['./core.wasm'].default),asset);
 const hostOptions={modules:[{type:'ESModule',path:bundle},{type:'CompiledWasm',path:asset}],modulesRoot:temporary,compatibilityDate:report.compatibilityDate,compatibilityFlags:report.compatibilityFlags,unsafeDirectSockets:[{host:'127.0.0.1',port:0}],cf:false,...(process.env.SVET_EDGE_GC_FORCE==='1'?{inspectorPort:0}:{})};
 report.hostOptionsAdapter=convertV4MiniflareOptions?'official-v4-converter':'native-v4';
 mf=new Miniflare(convertV4MiniflareOptions?convertV4MiniflareOptions(hostOptions):hostOptions);
 const uiResponse=await mf.dispatchFetch('http://fixture.test/retained-ui');report.retainedUi=await uiResponse.json();if(uiResponse.status!==200||report.retainedUi.status!=='passed'||report.retainedUi.requests!==16||report.retainedUi.retainedReferences!==40||report.retainedUi.nodesAfter!==0)throw Error('Edge retained UI lifecycle failed: '+JSON.stringify(report.retainedUi));
 const uiGcState=async()=>{const response=await mf.dispatchFetch('http://fixture.test/retained-ui-gc');if(response.status!==200)throw Error('Retained UI inspection failed');return response.json();};
 report.retainedUiTargets=await uiGcState();if(report.retainedUiTargets.targets!==32)throw Error('Wrong retained UI target count');
 const boundaryResponse=await mf.dispatchFetch('http://fixture.test/copy-boundaries');report.crossCopyBoundaries=await boundaryResponse.json();if(boundaryResponse.status!==200||report.crossCopyBoundaries.status!=='passed'||report.crossCopyBoundaries.cases.length!==4||report.crossCopyBoundaries.nodesAfter!==0)throw Error('Edge cross-copy boundaries failed: '+JSON.stringify(report.crossCopyBoundaries));
 let copyTimer;try{report.crossCopyOwners=await Promise.race([Promise.all([0,100].map(async trace=>{const response=await mf.dispatchFetch('http://fixture.test/copy-owners?trace='+trace),row=await response.json();if(response.status!==200||row.status!=='passed'||row.independentCores!==true||row.requests!==4||row.ownerMatches!==4||row.contexts!==1||row.failedChildCleanups!==1||row.lateRefusals!==1||row.rootOverrides!==1||row.domBindings!==2||row.reactiveDomUpdates!==2||row.eventTeardowns!==2||row.retainedCallbacks!==1||row.nodesAfter!==0)throw Error('Edge cross-copy ownership failed: '+JSON.stringify(row));return row;})),new Promise((_,reject)=>copyTimer=setTimeout(()=>reject(Error('Edge cross-copy deadline')),15000))]);}finally{clearTimeout(copyTimer);}
 const copyGcState=async()=>{const response=await mf.dispatchFetch('http://fixture.test/copy-gc');if(response.status!==200)throw Error('Edge cross-copy GC inspection failed');return response.json();};
 report.crossCopyOwnerTargets=await copyGcState();if(report.crossCopyOwnerTargets.targets!==24)throw Error('Wrong cross-copy target count');
 let timer;try{report.responses=await Promise.race([Promise.all([0,100].map(async trace=>{const response=await mf.dispatchFetch('http://fixture.test/probe?trace='+trace);return {httpStatus:response.status,result:await response.json()};})),new Promise((_,reject)=>timer=setTimeout(()=>reject(Error('Edge SSR deadline')),15000))]);}finally{clearTimeout(timer);}
 for(const response of report.responses)validateResponse(response);
 report.gcCapabilities=report.responses[0].result.gcCapabilities;
 const churnMs=Number(process.env.SVET_EDGE_CHURN_MS??1000);
 if(!Number.isSafeInteger(churnMs)||churnMs<0||churnMs>3600000)throw Error('Invalid SVET_EDGE_CHURN_MS');
 if(churnMs){
  const churnURL=await mf.unsafeGetDirectURL();
  report.churn={requestedMs:churnMs,timeoutTransport:process.env.SVET_EDGE_TIMEOUT_TRANSPORT==='chunks'?'chunks':'readable',directSocket:process.env.SVET_EDGE_DIRECT_CHURN==='1',diagnosticKeepalive:process.env.SVET_EDGE_KEEPALIVE==='1',concurrency:8,events:0,domCycles:0,streams:0,stringRenders:0,timeouts:0};
  const started=performance.now();let batch=0,lastProgress=0;
  do{
   let timer;try{
    const results=await Promise.race([Promise.all(Array.from({length:8},async(_,i)=>{
     const path='/probe?trace='+(10000+batch*1000+i*100)+(process.env.SVET_EDGE_KEEPALIVE==='1'?'&keepalive=1':'')+'&timeoutTransport='+report.churn.timeoutTransport;
     const response=process.env.SVET_EDGE_DIRECT_CHURN==='1'?await fetch(new URL(path,churnURL),{signal:AbortSignal.timeout(30000)}):await mf.dispatchFetch('http://fixture.test'+path);
     const text=await response.text();let result;try{result=JSON.parse(text);}catch{const stats=await mf.dispatchFetch('http://fixture.test/probe-stats').then(r=>r.text());throw Error('Edge churn non-JSON response '+response.status+': '+text+'; progress='+stats);}
     const row={httpStatus:response.status,result};validateResponse(row);if(result.timeoutTransport!==report.churn.timeoutTransport)throw Error('Wrong timeout probe transport');return row.result;
    })),new Promise((_,reject)=>timer=setTimeout(()=>reject(Error('Edge churn batch deadline')),30000))]);
    report.churn.events+=results.length;report.churn.domCycles+=results.reduce((n,r)=>n+r.domMatrix.cycles,0);
    report.churn.streams+=results.reduce((n,r)=>n+r.streamMatrix.streams,0);report.churn.stringRenders+=results.length*8;report.churn.timeouts+=results.length;
   }finally{clearTimeout(timer);}
   batch++;const elapsedMs=performance.now()-started;
   if(elapsedMs-lastProgress>=5000){console.log(JSON.stringify({edgeChurn:{elapsedMs,events:report.churn.events}}));lastProgress=elapsedMs;}
  }while(performance.now()-started<churnMs);
  report.churn.elapsedMs=performance.now()-started;report.churn.status='passed';
 }
 if(process.env.SVET_EDGE_GC==='1'){
  const operation=async action=>{const response=await mf.dispatchFetch('http://fixture.test/gc/'+action);const result=await response.json();if(response.status!==200)throw Error('Edge GC '+action+': '+JSON.stringify(result));return result;};
  const gc=report.gc={scope:'Local workerd WeakRef collection, inspector GC requests or allocation pressure, 24 requests/6 modes/both codecs; retain terminal streams/readers/errors. Positive controls survive held phase and collect after release. GC acknowledgements are recorded separately; no RSS plateau or deployed Edge qualification.',observations:[]};
  gc.allocation=await operation('allocate');gc.initial=await operation('inspect');
  if(process.env.SVET_EDGE_GC_FORCE==='1'){inspector=await openWorkerdInspector(mf);gc.inspectorTarget=inspector.target;gc.inspectorEnable=await inspector.send('Runtime.enable');gc.mechanism='Inspector GC requested; collection verified by WeakRef controls, command acknowledgement recorded separately';gc.gcCommands=[];gc.heapBefore=await inspector.send('Runtime.getHeapUsage');}
  for(let attempt=0;attempt<3;attempt++){if(inspector){await inspector.send('Runtime.discardConsoleEntries');const command={status:'pending'};gc.gcCommands.push(command);void inspector.send('HeapProfiler.collectGarbage').then(()=>{command.status='acknowledged';},error=>{command.status='unacknowledged';command.error=String(error);});await new Promise(resolve=>setTimeout(resolve,100));gc.heapLast=await inspector.send('Runtime.getHeapUsage');}else await operation('pressure');const observation={...await operation('inspect'),copyOwners:await copyGcState(),retainedUi:await uiGcState()};gc.observations.push({phase:'held',attempt,...observation});if(Object.keys(observation.alive).every(key=>key==='liveControl')&&observation.copyOwners.alive===0&&observation.retainedUi.alive===0)break;}
  gc.withHandles=gc.observations.at(-1);await operation('release');
  for(let attempt=0;attempt<3;attempt++){if(inspector){await inspector.send('Runtime.discardConsoleEntries');const command={status:'pending'};gc.gcCommands.push(command);void inspector.send('HeapProfiler.collectGarbage').then(()=>{command.status='acknowledged';},error=>{command.status='unacknowledged';command.error=String(error);});await new Promise(resolve=>setTimeout(resolve,100));gc.heapLast=await inspector.send('Runtime.getHeapUsage');}else await operation('pressure');const observation={...await operation('inspect'),copyOwners:await copyGcState(),retainedUi:await uiGcState()};gc.observations.push({phase:'released',attempt,...observation});if(!Object.keys(observation.alive).length&&observation.copyOwners.alive===0&&observation.retainedUi.alive===0)break;}
  gc.afterRelease=gc.observations.at(-1);
  const targets=Object.entries(gc.withHandles.alive).filter(([kind])=>kind!=='liveControl').reduce((n,[,count])=>n+count,0);
  gc.status=Object.keys(gc.afterRelease.alive).length?'inconclusive':targets?'retention-reproduced':gc.withHandles.alive.liveControl===4?'passed':'invalid-controls';
  if(gc.status==='passed'&&(gc.withHandles.copyOwners.targets!==24||gc.withHandles.copyOwners.alive!==0||gc.afterRelease.copyOwners.alive!==0))gc.status='cross-copy-retention';
  if(gc.status==='passed'&&(gc.withHandles.retainedUi.targets!==32||gc.withHandles.retainedUi.alive!==0||gc.afterRelease.retainedUi.alive!==0))gc.status='retained-ui-retention';
  if(gc.status==='passed'&&(gc.allocation.ownerTokensHeld!==20||!gc.allocation.ownerTokensRetired||gc.afterRelease.ownerTokensHeld!==20))gc.status='invalid-owner-token-probe';
  if(inspector&&process.env.SVET_EDGE_GC_SNAPSHOT){
    const chunks=[],unsubscribe=inspector.subscribe(message=>{if(message.method==='HeapProfiler.addHeapSnapshotChunk')chunks.push(message.params.chunk);});
    const snapshot=inspector.send('HeapProfiler.takeHeapSnapshot',{reportProgress:false});
    await inspector.send('Runtime.getHeapUsage');await snapshot;unsubscribe();
    const data=chunks.join('');await writeFile(process.env.SVET_EDGE_GC_SNAPSHOT,data);gc.snapshot={path:process.env.SVET_EDGE_GC_SNAPSHOT,bytes:Buffer.byteLength(data),sha256:createHash('sha256').update(data).digest('hex')};
  }
  if(gc.status!=='passed')throw Error('Edge GC '+gc.status+': '+JSON.stringify(gc));
 }
 const engine=await import(pathToFileURL(resolve(packageRoot,metadata.exports['.'].import)).href),{assertSsrStreamResponse,createSsrStreamTransportDecoder}=engine;consumerDom=createResumptionFixture(engine);
 const directURL=await mf.unsafeGetDirectURL();report.responseTransport='direct-loopback-workerd';
 const directFetch=path=>fetch(new URL(path,directURL),{signal:AbortSignal.timeout(10000)});
 report.transports=[];
 async function bounded(promise){let timer;try{return await Promise.race([promise,new Promise((_,reject)=>timer=setTimeout(()=>reject(Error('Edge response transport deadline')),10000))]);}finally{clearTimeout(timer);}}
 for(const codec of ['identity','svet-html-ref-v1'])for(const mode of ['complete','error','cancel'])await bounded((async()=>{
  const id=codec+'-'+mode;let response,fetchFailure;if(mode!=='cancel'){
   try{response=await directFetch('/stream?id='+id+'&codec='+codec+'&mode='+mode);}
   catch(error){if(mode!=='error')throw error;fetchFailure=error;}
   if(response)assertSsrStreamResponse(response,{codec});
  }
  if(mode==='cancel'){
   const raw=await new Promise((resolve,reject)=>{const req=httpGet(new URL('/stream?id='+id+'&codec='+codec+'&mode='+mode,directURL),res=>resolve({req,res}));req.on('error',reject);});raw.res.on('error',()=>{});
   if(raw.res.statusCode!==200)throw Error('Raw Edge stream request failed');let text='';
   await new Promise((resolve,reject)=>{raw.res.on('data',part=>{text+=part.toString('utf8');if(text.includes('\n')){raw.res.pause();resolve();}});raw.res.once('end',()=>{if(!text.includes('\n'))reject(Error('No raw Edge shell'));});});
   if(JSON.parse(text.trim()).payload.type!=='shell')throw Error('Wrong raw Edge shell');
   const before=await directFetch('/stream-stats?id='+id).then(r=>r.json());if(before.phase!=='component-pending'||before.cleaned!==0||before.nodes<1)throw Error('External cancellation did not observe pending component');
   await new Promise(resolve=>{const socket=raw.res.socket;socket.once('close',resolve);socket.resetAndDestroy();});raw.req.destroy();
  }else if(mode==='error'){
   let failed=false,text='';if(response){try{text=await response.text();}catch{failed=true;}}
   const receiver=engine.createSsrStreamReceiver(document.createElement('div')),decoder=codec==='identity'?null:createSsrStreamTransportDecoder();
   try{for(const line of text.trim()?text.trim().split('\n'):[]){const frame=JSON.parse(line);receiver.apply(decoder?decoder.decode(frame):frame);}let rejected=false;try{receiver.finish();}catch(error){rejected=String(error).includes('before complete');}if(!rejected)throw Error('Truncated Edge error response accepted as completed');report.transports.push({codec,mode:'error-observation',transport:fetchFailure?'fetch-rejected-before-response':failed?'body-read-rejected':'clean-eof-without-complete',fetchFailure:fetchFailure?{name:fetchFailure.name,message:fetchFailure.message,causeCode:fetchFailure.cause?.code}:undefined,eofRejected:true});}finally{decoder?.dispose();receiver.dispose();}
  }else{
   const text=await response.text(),decoder=codec==='identity'?null:createSsrStreamTransportDecoder();const chunks=text.trim().split('\n').map(line=>{const frame=JSON.parse(line);return decoder?decoder.decode(frame):frame;});decoder?.dispose();
   if(chunks.map(c=>c.sequence).join(',')!=='0,1,2'||JSON.parse(chunks[2].payload.stateJson).signals.count!==7||!chunks[2].payload.html.includes('response|&lt;&amp;🦀'))throw Error('External Edge frames/state corrupted');
   const receiver=engine.createSsrStreamReceiver(document.createElement('div'));try{for(const chunk of chunks)receiver.apply(chunk);receiver.finish();}finally{receiver.dispose();}if(engine.getGraphTelemetry().nodes!==0)throw Error('Edge consumer resume graph leaked');
  }
  let state;for(let i=0;i<1000;i++){state=await directFetch('/stream-stats?id='+id).then(r=>r.json());if(state.cleaned===1&&state.nodes===0)break;await new Promise(resolve=>setTimeout(resolve,1));}
  if((mode==='error'&&!state.producerFailed)||state.cleaned!==1||state.nodes!==0||(mode==='cancel'&&state.retiredPhase!=='component-pending'))throw Error('External Edge transport retained request or cancelled too late: '+JSON.stringify(state));
  if(mode==='error'){
   const recoveryId=id+'-recovery',recovery=await directFetch('/stream?id='+recoveryId+'&codec='+codec+'&mode=complete');assertSsrStreamResponse(recovery,{codec});
   const decoder=codec==='identity'?null:createSsrStreamTransportDecoder(),receiver=engine.createSsrStreamReceiver(document.createElement('div'));
   try{for(const line of (await recovery.text()).trim().split('\n')){const frame=JSON.parse(line);receiver.apply(decoder?decoder.decode(frame):frame);}receiver.finish();}finally{decoder?.dispose();receiver.dispose();}
   const recovered=await directFetch('/stream-stats?id='+recoveryId).then(r=>r.json());
   if(recovered.cleaned!==1||recovered.nodes!==0||engine.getGraphTelemetry().nodes!==0)throw Error('Edge post-error recovery leaked');
   report.transports.push({codec,mode:'error-recovery',state:recovered});await directFetch('/stream-stats?id='+recoveryId+'&forget=1').then(r=>r.text());
  }
  report.transports.push({codec,mode,state});await directFetch('/stream-stats?id='+id+'&forget=1').then(r=>r.text());
 })());
 report.proxyAborts=[];
 for(const codec of ['identity','svet-html-ref-v1'])await bounded((async()=>{
  const id='proxy-abort-'+codec,abort=new AbortController();
  const response=await mf.dispatchFetch('http://fixture.test/stream?id='+id+'&codec='+codec+'&mode=cancel&deadline=500',{signal:abort.signal});assertSsrStreamResponse(response,{codec});
  const reader=response.body.getReader();await reader.read();
  const before=await directFetch('/stream-stats?id='+id).then(r=>r.json());if(before.phase!=='component-pending'||before.cleaned!==0)throw Error('Proxy abort did not observe pending request');
  abort.abort(Error('Explicit proxy request abort'));await reader.cancel().catch(()=>{});
  let state;for(let i=0;i<1000;i++){state=await directFetch('/stream-stats?id='+id).then(r=>r.json());if(state.cleaned===1&&state.nodes===0)break;await new Promise(resolve=>setTimeout(resolve,1));}
  report.proxyAborts.push({codec,before,state});
  if((!state.signalAborted&&!state.deadlineAborted)||state.cleaned!==1||state.nodes!==0||state.retiredPhase!=='component-pending')throw Error('Proxy deadline failed to retire pending request: '+JSON.stringify(state));
  await directFetch('/stream-stats?id='+id+'&forget=1').then(r=>r.text());
 })());
 report.status=report.responses.every(response=>response.httpStatus===200&&response.result.status==='passed')?'passed':'failed';if(report.status!=='passed')process.exitCode=1;
}catch(error){report.status='failed';report.error=String(error.stack??error);process.exitCode=1;}
finally{inspector?.close();consumerDom?.close();try{await mf?.dispose();}catch(error){report.disposeError=String(error);process.exitCode=1;}report.completedAt=new Date().toISOString();for(const [key,path] of [['miniflare','miniflare'],['workerd','workerd'],['dom','linkedom'],['bundler','esbuild']])report[key]=JSON.parse(await readFile(resolve(deps,'node_modules',path,'package.json'),'utf8')).version;report.wasmBridgeSha256=createHash('sha256').update(await readFile(resolve(packageRoot,'dist/package/wasm-bridge.js'))).digest('hex');report.reactivitySha256=createHash('sha256').update(await readFile(resolve(packageRoot,'dist/package/reactivity.js'))).digest('hex');report.runtimeSha256=createHash('sha256').update(await readFile(resolve(packageRoot,metadata.exports['./server-runtime'].import))).digest('hex');report.driverSourceSha256=createHash('sha256').update(await readFile(fileURLToPath(import.meta.url))).digest('hex');report.inspectorSourceSha256=createHash('sha256').update(await readFile(resolve(root,'scripts/workerd-inspector.mjs'))).digest('hex');report.retentionSourceSha256=createHash('sha256').update(await readFile(resolve(root,'tests/edge/retention.mjs'))).digest('hex');report.crossCopySourceSha256=createHash('sha256').update(await readFile(resolve(root,'tests/edge/cross-copy-owner.mjs'))).digest('hex');report.retainedUiSourceSha256=createHash('sha256').update(await readFile(resolve(root,'tests/edge/retained-ui.mjs'))).digest('hex');report.boundarySourceSha256=createHash('sha256').update(await readFile(resolve(root,'tests/edge/cross-copy-boundaries.mjs'))).digest('hex');report.workerSourceSha256=createHash('sha256').update(await readFile(resolve(root,'tests/edge/worker.mjs'))).digest('hex');report.wasmSha256=createHash('sha256').update(await readFile(resolve(packageRoot,metadata.exports['./core.wasm'].default))).digest('hex');try{report.bundleSha256=createHash('sha256').update(await readFile(bundle)).digest('hex');}catch{}if(process.argv[3])await writeFile(process.argv[3],JSON.stringify(report,null,2)+'\n');console.log(JSON.stringify(report,null,2));}
