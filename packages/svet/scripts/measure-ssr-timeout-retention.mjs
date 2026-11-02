import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {createHash} from 'node:crypto';
import {resolve} from 'node:path';
import {pathToFileURL} from 'node:url';
import {createResumptionFixture} from './browser-fixture-endpoints.mjs';
const base=process.argv[3]?pathToFileURL(resolve(process.argv[3],'dist/package')+'/'):new URL('../dist/package/',import.meta.url);
const e=await import(new URL('index.js',base)),r=await import(new URL('server.js',base)),host=createResumptionFixture(e);
assert.equal(typeof global.gc,'function');const held=[],probes=[];
function track(mode,kind,value){probes.push({mode,kind,ref:new WeakRef(value)});}
async function run(mode){
 const setup=()=>{const core=e.getWasmCore(),source=e.signal(1);e.onCleanup(source.dispose);track(mode,'core',core);track(mode,'memory',core.memory);track(mode,'signal',source);return core;};
 const view=()=>{setup();return {node:e.h(async()=>{await new Promise(()=>{});return e.h('p',null);},null)};};
 let failure,handle;
 try{
  if(mode==='child')await r.runWithIsolatedContextAsync(async()=>{setup();try{await r.runWithOwnerScopeAsync([],()=>new Promise(()=>{}),{timeoutMs:1});}catch(error){failure=error;}});
  else if(mode==='string')await r.renderToStringAsync(view,{timeoutMs:1});
  else if(mode==='chunks'){handle=r.renderToChunksAsync(view,{timeoutMs:1});await handle.next();await handle.next();}
  else{const stream=r.renderToReadableStreamAsync(view,{timeoutMs:1}),reader=stream.getReader();handle={stream,reader};await reader.read();await reader.closed;}
 }catch(error){failure=error;}
 assert(failure instanceof r.SsrTimeoutError);assert.equal(failure.timeoutMs,1);held.push({failure,handle});
}
async function collect(){await new Promise(setImmediate);for(let i=0;i<10;i++){global.gc();await new Promise(setImmediate);}const alive={};for(const p of probes)if(p.ref.deref())alive[p.mode+':'+p.kind]=(alive[p.mode+':'+p.kind]??0)+1;return alive;}
const report={startedAt:new Date().toISOString(),node:process.version,requests:32,concurrency:32,scope:'Retained engine timeout errors and terminal handles across string/chunk/readable/child operations. Native Node object collection; no RSS/Edge/long plateau claim.'};
try{
 await Promise.all(Array.from({length:8},()=>['string','chunks','readable','child'].map(run)).flat());
 report.withErrors=await collect();held.length=0;report.afterRelease=await collect();report.objects=probes.length;
 assert.deepEqual(report.afterRelease,{},'Controls must collect after releasing errors');
 report.status=Object.keys(report.withErrors).length?'retention-reproduced':'passed';
}catch(error){report.status='failed';report.error=String(error.stack??error);process.exitCode=1;}
finally{host.close();report.completedAt=new Date().toISOString();report.sourceSha256=createHash('sha256').update(await readFile(new URL(import.meta.url))).digest('hex');report.runtimeSha256=createHash('sha256').update(await readFile(new URL('ssr-runtime.js',base))).digest('hex');if(process.argv[2])await writeFile(process.argv[2],JSON.stringify(report,null,2)+'\n');console.log(JSON.stringify(report,null,2));}
if(report.status!=='passed')process.exitCode=1;
