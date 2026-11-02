import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {resolve} from 'node:path';
import {pathToFileURL} from 'node:url';
import {createHash} from 'node:crypto';
import {JSDOM} from 'jsdom';
assert(process.argv[2]&&process.argv[3],'Expected output and installed-copy manifest');
const manifest=JSON.parse(await readFile(resolve(process.argv[3]),'utf8')),build=JSON.parse(await readFile(resolve(manifest.output,'metadata.json'),'utf8'));
for(const [path,sha] of Object.entries(build.artifactSha256))assert.equal(createHash('sha256').update(await readFile(resolve(manifest.output,path))).digest('hex'),sha);
const dom=new JSDOM('<!doctype html><body></body>');
for(const key of ['document','Node','Element','HTMLElement','MutationObserver'])globalThis[key]=dom.window[key];
const a=await import(pathToFileURL(resolve(manifest.output,'first/index.js')).href),b=await import(pathToFileURL(resolve(manifest.output,'second/index.js')).href);
const report={cases:[],build,scope:'Compatible independent installed copies; shared explicit core; JSDOM boundary/resource lifecycle'};
async function check(name,body){try{await body();report.cases.push({name,status:'passed'});}catch(error){report.cases.push({name,status:'failed',error:String(error.stack??error)});}finally{a.resetReactiveSystem();b.resetReactiveSystem();document.body.replaceChildren();}}
for(const [label,host,foreign] of [['A→B',a,b],['B→A',b,a]]){
 await check(label+' Suspense sees pending, completion and unmount cancellation',async()=>{
  const core=host.getWasmCore();let resolvePending,resource,aborts=0;
  const pending=new Promise(yes=>resolvePending=yes);
  const app=host.mount(()=>host.Suspense({fallback:'waiting',children:()=>{resource=foreign.withWasmCore(core,()=>foreign.createResource(({signal})=>{signal.addEventListener('abort',()=>aborts++);return pending;}));return document.createTextNode('content');}}),document.body);
  try{
   const fallback=document.querySelector('[data-svet-suspense-fallback]');assert.equal(fallback.style.display,'contents','Foreign pending was invisible');
   resolvePending('ready');await pending;await Promise.resolve();assert.equal(resource(),'ready');assert.equal(fallback.style.display,'none');
   const job=resource.refetch();assert.equal(fallback.style.display,'contents');app.unmount();await job;assert.equal(host.getGraphTelemetry().nodes,0);assert.equal(aborts,1);
  }finally{app.unmount();}
 });
 await check(label+' ErrorBoundary sees foreign rejection and retry',async()=>{
  const core=host.getWasmCore();let rejectPending,retry,mounts=0;
  const pending=new Promise((_,no)=>rejectPending=no),reason=Error('foreign failure');
  const app=host.mount(()=>host.ErrorBoundary({children:()=>{mounts++;foreign.withWasmCore(core,()=>foreign.createResource(()=>mounts===1?pending:Promise.resolve('ready')));return document.createTextNode('content');},fallback:(error,reset)=>{assert.equal(error,reason);retry=reset;return document.createTextNode('error');}}),document.body);
  try{rejectPending(reason);await pending.catch(()=>{});await Promise.resolve();assert.equal(document.body.textContent,'error');retry();assert.equal(mounts,2);await Promise.resolve();assert.equal(document.body.textContent,'content');}finally{app.unmount();}assert.equal(host.getGraphTelemetry().nodes,0);
 });
}
dom.window.close();report.status=report.cases.every(row=>row.status==='passed')?'passed':'failed';report.probeSha256=createHash('sha256').update(await readFile(new URL(import.meta.url))).digest('hex');await writeFile(process.argv[2],JSON.stringify(report,null,2)+'\n');console.log(JSON.stringify(report.cases,null,2));if(report.status!=='passed')process.exitCode=1;
