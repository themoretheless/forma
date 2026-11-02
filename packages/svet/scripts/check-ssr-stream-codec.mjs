import assert from 'node:assert/strict';
import {writeFile,readFile} from 'node:fs/promises';
import {resolve} from 'node:path';
import {pathToFileURL} from 'node:url';
import {createHash} from 'node:crypto';
import {createResumptionFixture} from './browser-fixture-endpoints.mjs';
const runtime=process.argv[3]?pathToFileURL(resolve(process.argv[3],'dist/package')+'/'):new URL('../dist/package/',import.meta.url);
const e=await import(new URL('index.js',runtime)),r=await import(new URL('server.js',runtime)),host=createResumptionFixture(e);
const report={startedAt:new Date().toISOString(),node:process.version};
try{
 let release;const wait=new Promise(resolve=>release=resolve),signals={},chunks=[];
 const stream=r.renderToChunksAsync(()=>({node:e.h('main',null,e.h('b',null,'shell'),e.h(async()=>{await wait;signals.count=e.signal(7);e.onCleanup(signals.count.dispose);return e.h('section',null,e.h('input',{type:'number','data-svet-model':'count',model:signals.count}),e.h('p',null,'🦀<&'.repeat(20000)));},null)),signals}));
 chunks.push((await stream.next()).value);release();for await(const chunk of stream)chunks.push(chunk);
 const encoder=e.createSsrStreamTransportEncoder(),decoder=e.createSsrStreamTransportDecoder(),frames=chunks.map(chunk=>encoder.encode(chunk));
 const restored=frames.map(frame=>decoder.decode(JSON.parse(JSON.stringify(frame))));assert.deepEqual(restored,chunks);assert('codec' in frames[2]);
 for(const records of [chunks,restored]){
  const container=document.createElement('div'),receiver=e.createSsrStreamReceiver(container);receiver.apply(records[0]);receiver.apply(records[1]);const input=container.querySelector('input');const state=receiver.apply(records[2]);assert.equal(container.querySelector('input'),input);assert.equal(state.count(),7);input.value='12';input.dispatchEvent(new document.defaultView.Event('input'));assert.equal(state.count(),12);receiver.dispose();
 }
 report.plainChunkBytes=chunks.map(c=>Buffer.byteLength(JSON.stringify(c)));report.encodedChunkBytes=frames.map(c=>Buffer.byteLength(JSON.stringify(c)));report.plainBytes=report.plainChunkBytes.reduce((a,b)=>a+b,0);report.encodedBytes=report.encodedChunkBytes.reduce((a,b)=>a+b,0);assert(report.encodedBytes<report.plainBytes*0.6);
 const timings=[];for(let round=0;round<10;round++){const start=performance.now();for(let i=0;i<10;i++){const enc=e.createSsrStreamTransportEncoder(),dec=e.createSsrStreamTransportDecoder();for(const chunk of chunks)dec.decode(JSON.parse(JSON.stringify(enc.encode(chunk))));}timings.push((performance.now()-start)/10);}
 report.codecRoundtripMs=timings;report.memoryAfter=process.memoryUsage();assert.equal(e.getGraphTelemetry().nodes,0);report.status='passed';
}catch(error){report.status='failed';report.error=String(error.stack??error);process.exitCode=1;}
finally{host.close();report.completedAt=new Date().toISOString();report.scope='One Node async render, lossless JSON transport framing and two JSDOM resume/input identity checks; 100 codec roundtrips. Timings are unpaired diagnostics, not acceleration proof; memory is one process sample, not plateau.';report.sourceSha256=createHash('sha256').update(await readFile(new URL(import.meta.url))).digest('hex');report.runtimeSha256={};for(const file of ['stream-codec.js','server.js','stream.js'])report.runtimeSha256[file]=createHash('sha256').update(await readFile(new URL(file,runtime))).digest('hex');if(process.argv[2])await writeFile(process.argv[2],JSON.stringify(report,null,2)+'\n');console.log(JSON.stringify(report,null,2));}
