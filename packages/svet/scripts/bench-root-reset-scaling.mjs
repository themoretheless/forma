/** Paired diagnostic; timings are not application/browser performance qualification. */
import assert from 'node:assert/strict';
import {readFile,writeFile,readdir} from 'node:fs/promises';
import {createHash} from 'node:crypto';
import {spawnSync} from 'node:child_process';
import {cpus,loadavg,platform,arch} from 'node:os';
import {resolve} from 'node:path';
import {fileURLToPath,pathToFileURL} from 'node:url';
const script=fileURLToPath(import.meta.url);
const median=values=>{const a=[...values].sort((x,y)=>x-y),i=a.length>>1;return a.length%2?a[i]:(a[i-1]+a[i])/2;};
async function hashes(directory){const result={};for(const name of (await readdir(directory)).sort())if(/\.(js|wasm)$/.test(name))result[name]=createHash('sha256').update(await readFile(resolve(directory,name))).digest('hex');return result;}
if(process.argv[2]==='--worker'){
 const [installation,label,roundText]=process.argv.slice(3),directory=resolve(installation,'dist/package');
 const e=await import(pathToFileURL(resolve(directory,'reactivity.js')));
 const before=await hashes(directory),rows=[];
 for(const operation of [512,2048,8192].flatMap(size=>['none','interleaved','tail'].map(pattern=>`${size}:${pattern}`))){
  const times=[];
  for(let sample=-2;sample<6;sample++){
   e.resetReactiveSystem();let elapsed;
   const [sizeText,pattern]=operation.split(':'),size=Number(sizeText);
   const source=e.signal(2);let stop,cleanups=0,derived;
   e.createRoot(dispose=>{stop=dispose;derived=Array.from({length:size},()=>{
    const value=e.computed(()=>source()*2);assert.equal(value(),4);
    if(pattern==='interleaved')e.onCleanup(()=>{cleanups++;});return value;
   });if(pattern==='tail')for(let n=0;n<size;n++)e.onCleanup(()=>{cleanups++;});});
   const start=performance.now();e.resetReactiveSystem();elapsed=performance.now()-start;
   assert.equal(cleanups,0);assert.equal(derived[0](),4);assert.equal(derived.at(-1)(),4);
   const fresh=e.signal(9);stop();stop();assert.equal(fresh(),9);fresh.dispose();
   assert.equal(cleanups,pattern==='none'?0:size);
   assert.equal(e.getGraphTelemetry().nodes,0);
   if(sample>=0)times.push(elapsed);
  }
  rows.push({operation,samplesMs:times,medianMs:median(times)});
 }
 assert.deepEqual(await hashes(directory),before);
 console.log(JSON.stringify({label,round:Number(roundText),rows,load:loadavg(),artifactSha256:before}));
}else{
 const [baseline,candidate,output]=process.argv.slice(2);assert(baseline&&candidate&&output);
 const workers=[],orders=[['baseline','control','candidate'],['candidate','control','baseline'],['control','baseline','candidate'],['candidate','baseline','control'],['baseline','candidate','control'],['control','candidate','baseline']];
 for(let round=0;round<orders.length;round++)for(const label of orders[round]){
  const result=spawnSync(process.execPath,[script,'--worker',label==='candidate'?candidate:baseline,label,String(round)],{encoding:'utf8',timeout:120000,maxBuffer:4*1024*1024});
  assert.equal(result.status,0,result.stderr||String(result.error));workers.push(JSON.parse(result.stdout));
 }
 const summary=[];
 for(const operation of workers[0].rows.map(row=>row.operation)){
  const pairs=orders.map((_,round)=>{const get=label=>workers.find(worker=>worker.round===round&&worker.label===label).rows.find(row=>row.operation===operation).medianMs;const a=get('baseline');return {round,baselineMs:a,controlMs:get('control'),candidateMs:get('candidate'),aaRatio:get('control')/a,abRatio:get('candidate')/a};});
  summary.push({operation,pairs,medianAaRatio:median(pairs.map(pair=>pair.aaRatio)),medianAbRatio:median(pairs.map(pair=>pair.abRatio))});
 }
 await writeFile(output,JSON.stringify({status:'diagnostic',createdAt:new Date().toISOString(),node:process.version,host:{platform:platform(),arch:arch(),cpus:cpus().length,model:cpus()[0].model,load:loadavg()},warmups:2,samplesPerWorkerOperation:6,rounds:6,workers,summary,scope:'Root-owned computed reset scaling with none/interleaved/tail user cleanup registrations. Serial isolated Node workers, balanced orders, baseline A/A and A/B. No quiet-host, application latency, browser or memory plateau qualification.'},null,2)+'\n');
 console.log(JSON.stringify(summary.map(({operation,medianAaRatio,medianAbRatio})=>({operation,medianAaRatio,medianAbRatio})),null,2));
}
