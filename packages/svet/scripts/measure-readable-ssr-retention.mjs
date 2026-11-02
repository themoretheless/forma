import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFile,writeFile} from 'node:fs/promises';
import {resolve} from 'node:path';
import {pathToFileURL} from 'node:url';
import {createResumptionFixture} from './browser-fixture-endpoints.mjs';
const base=process.argv[3]?pathToFileURL(resolve(process.argv[3],'dist/package')+'/'):new URL('../dist/package/',import.meta.url);
const e=await import(new URL('index.js',base)),r=await import(new URL('server.js',base)),host=createResumptionFixture(e);
assert.equal(typeof global.gc,'function','Run node --expose-gc');
const retained=[],probes=[];const track=(kind,value)=>probes.push({kind,ref:new WeakRef(value)});
async function run(mode,codec){
 const marker={mode},reason=Error('background failure outside request');let release;const wait=new Promise(resolve=>release=resolve),abort=new AbortController();
 track('capture',marker);
 const view=()=>{assert.equal(marker.mode,mode);const core=e.getWasmCore();track('core',core);track('memory',core.memory);const source=e.signal(1);e.onCleanup(source.dispose);track('signal',source);const node=e.h('main',null,e.h(async()=>{await wait;if(mode==='error')throw reason;return e.h('p',null,'x'.repeat(512));},null));track('node',node);return {node};};track('view',view);
 const stream=r.renderToReadableStreamAsync(view,{codec,signal:abort.signal}),reader=stream.getReader();retained.push({stream,reader});
 await reader.read();
 if(mode==='cancel')await reader.cancel();
 else if(mode==='abort'){const observed=reader.closed.catch(error=>error);abort.abort('request stopped');assert.equal(await observed,'request stopped');}
 else if(mode==='error'){const observed=reader.closed.catch(error=>error);release();assert.equal(await observed,reason);}
 else{release();while(!(await reader.read()).done){}await reader.closed;}
 release();await new Promise(setImmediate);
}
function unusedStream(){
 const marker={kind:'unused'};
 const view=()=>{assert.equal(marker.kind,'unused');return {node:document.createElement('p')};};
 const stream=r.renderToReadableStreamAsync(view);
 track('unused-capture',marker);track('unused-view',view);track('unused-stream',stream);
}
const report={startedAt:new Date().toISOString(),node:process.version,requests:64,unusedStreams:16,scope:'64 Node isolated requests, both codecs, terminal stream+reader handles retained after complete/cancel/abort/background-error. WeakRef object collection only; errors originate outside request. No RSS plateau or arbitrary user error graph claim.'};
async function bounded(promise){let timer;try{return await Promise.race([promise,new Promise((_,reject)=>timer=setTimeout(()=>reject(Error('Readable retention request deadline')),5000))]);}finally{clearTimeout(timer);}}
try{
 for(let i=0;i<8;i++)for(const codec of ['identity','svet-html-ref-v1'])for(const mode of ['complete','cancel','abort','error'])await bounded(run(mode,codec));
 for(let i=0;i<16;i++)unusedStream();
 await new Promise(setImmediate);
 for(let round=0;round<10;round++){global.gc();await new Promise(setImmediate);}
 const alive={};for(const probe of probes)if(probe.ref.deref())alive[probe.kind]=(alive[probe.kind]??0)+1;
 report.objects=probes.length;report.aliveWithHandles=alive;retained.length=0;await new Promise(setImmediate);for(let round=0;round<10;round++){global.gc();await new Promise(setImmediate);}
 report.aliveAfterRelease=probes.filter(probe=>probe.ref.deref()).length;
 assert.deepEqual(alive,{},'Terminal or discarded unstarted streams retained request objects');assert.equal(report.aliveAfterRelease,0);report.status='passed';
}catch(error){report.status='failed';report.error=String(error.stack??error);process.exitCode=1;}
finally{host.close();report.completedAt=new Date().toISOString();report.sourceSha256=createHash('sha256').update(await readFile(new URL(import.meta.url))).digest('hex');report.serverRuntimeSha256=createHash('sha256').update(await readFile(new URL('server.js',base))).digest('hex');if(process.argv[2])await writeFile(process.argv[2],JSON.stringify(report,null,2)+'\n');console.log(JSON.stringify(report,null,2));}
