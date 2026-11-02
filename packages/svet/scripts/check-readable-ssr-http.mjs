import assert from 'node:assert/strict';
import {createServer} from 'node:http';
import {Readable} from 'node:stream';
import {pipeline} from 'node:stream/promises';
import {readFile,writeFile} from 'node:fs/promises';
import {resolve} from 'node:path';
import {pathToFileURL} from 'node:url';
import {createHash} from 'node:crypto';
import {createResumptionFixture} from './browser-fixture-endpoints.mjs';
const base=process.argv[3]?pathToFileURL(resolve(process.argv[3],'dist/package')+'/'):new URL('../dist/package/',import.meta.url);
const e=await import(new URL('index.js',base)),r=await import(new URL('server.js',base)),host=createResumptionFixture(e),states=new Map();
const report={startedAt:new Date().toISOString(),node:process.version,checks:[]};
const server=createServer((req,res)=>{
 const url=new URL(req.url,'http://fixture'),id=url.searchParams.get('id');
 if(url.pathname==='/stats'){const row=states.get(id);res.setHeader('content-type','application/json');res.end(JSON.stringify(row?{settled:row.settled,nodes:row.core?.svet_get_node_count(),cleaned:row.cleaned}:{}));return;}
 if(url.pathname==='/release'){states.get(id)?.release();res.end('released');return;}
 if(url.pathname!=='/open'||states.has(id)||states.size>=8){res.writeHead(400);res.end();return;}
 const codec=e.negotiateSsrStreamCodec(req.headers[e.SSR_STREAM_CODEC_HEADER]??null);if(!codec){res.writeHead(406);res.end();return;}
 let release;const wait=new Promise(resolve=>release=resolve),row={release,settled:false,cleaned:0,core:null};states.set(id,row);
 const stream=r.renderToReadableStreamAsync(()=>{row.core=e.getWasmCore();return {node:e.h('main',null,e.h('b',null,'early'),e.h(async()=>{e.onCleanup(()=>row.cleaned++);const pending=e.signal(0);e.onCleanup(pending.dispose);await wait;return e.h('p',null,'x'.repeat(32768));},null))};},{codec});
 const response=new Response(stream,{headers:{'content-type':e.SSR_STREAM_CONTENT_TYPE,[e.SSR_STREAM_VERSION_HEADER]:'1',[e.SSR_STREAM_CODEC_HEADER]:codec}});res.writeHead(response.status,Object.fromEntries(response.headers));
 void pipeline(Readable.fromWeb(response.body),res).catch(()=>{}).finally(()=>{row.settled=true;});
});
try{
 await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));const root=`http://127.0.0.1:${server.address().port}`;
 const control=async(op,id)=>{const res=await fetch(`${root}/${op}?id=${id}`,{signal:AbortSignal.timeout(5000)});assert(res.ok);return op==='stats'?res.json():res.text();};
 for(const codec of ['identity',e.SSR_STREAM_HTML_REFERENCE_CODEC])for(const cancel of [false,true]){
  const id=`${codec}-${cancel}`,response=await fetch(`${root}/open?id=${id}`,{headers:{[e.SSR_STREAM_CODEC_HEADER]:codec},signal:AbortSignal.timeout(10000)});e.assertSsrStreamResponse(response,{codec});const reader=response.body.getReader();let text=new TextDecoder().decode((await reader.read()).value);assert.equal(JSON.parse(text.trim()).payload.type,'shell');const before=await control('stats',id);assert(before.nodes>0&&!before.settled);
  if(cancel)await reader.cancel();else{await control('release',id);const decoder=new TextDecoder();for(let part=await reader.read();!part.done;part=await reader.read())text+=decoder.decode(part.value,{stream:true});text+=decoder.decode();const transport=codec==='identity'?null:e.createSsrStreamTransportDecoder();const chunks=text.trim().split('\n').map(line=>{const frame=JSON.parse(line);return transport?transport.decode(frame):frame;});transport?.dispose();assert.deepEqual(chunks.map(c=>c.sequence),[0,1,2]);assert.equal(chunks[2].payload.html,'<main><b>early</b><p>'+'x'.repeat(32768)+'</p></main>');}
  let after;for(let i=0;i<1000;i++){after=await control('stats',id);if(after.settled&&after.nodes===0)break;await new Promise(resolve=>setTimeout(resolve,1));}assert(after.settled);assert.equal(after.nodes,0);assert.equal(after.cleaned,1);await control('release',id);report.checks.push({codec,cancel,before,after});
 }
 assert.equal(e.getGraphTelemetry().nodes,0);report.status='passed';
}catch(error){report.status='failed';report.error=String(error.stack??error);process.exitCode=1;}
finally{for(const row of states.values())row.release();server.closeAllConnections();await new Promise(resolve=>server.close(resolve));host.close();report.completedAt=new Date().toISOString();report.sourceSha256=createHash('sha256').update(await readFile(new URL(import.meta.url))).digest('hex');report.serverRuntimeSha256=createHash('sha256').update(await readFile(new URL('server.js',base))).digest('hex');if(process.argv[2])await writeFile(process.argv[2],JSON.stringify(report,null,2)+'\n');console.log(JSON.stringify({status:report.status,checks:report.checks.length,error:report.error}));}
