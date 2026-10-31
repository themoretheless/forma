import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFile,writeFile,readdir} from 'node:fs/promises';
import {resolve} from 'node:path';
import {pathToFileURL} from 'node:url';
import {parseArgs} from 'node:util';
const {values}=parseArgs({options:{baseline:{type:'string'},candidate:{type:'string'},output:{type:'string'},rounds:{type:'string',default:'7'},sizes:{type:'string',default:'1000,5000,20000'}}});
if(!values.baseline)throw Error('Pass --baseline DIR and optionally --candidate DIR --output FILE');
const rounds=Number(values.rounds),sizes=values.sizes.split(',').map(Number);
assert.ok(Number.isInteger(rounds)&&rounds>0&&sizes.every(size=>Number.isInteger(size)&&size>0));
const variants={};const compiledHashes={};
for(const name of ['baseline','candidate'])if(values[name]){const directory=resolve(values[name]);variants[name]=await import(pathToFileURL(resolve(directory,'index.js')).href);const hashes={};for(const file of (await readdir(directory)).filter(file=>file.endsWith('.js')).sort())hashes[file]=createHash('sha256').update(await readFile(resolve(directory,file))).digest('hex');compiledHashes[name]=hashes;}
const median=values=>[...values].sort((a,b)=>a-b)[Math.floor(values.length/2)];
function sequentialSample(engine,size){
 let stop=()=>{},owner,last,elapsed=0;
 try{
  engine.createRoot(dispose=>{stop=dispose;owner=engine.getCurrentOwner();const start=performance.now();for(let i=0;i<size;i++){last=engine.captureReactiveContext();last.dispose();}elapsed=performance.now()-start;});
  assert.equal(last.active,false);assert.equal(owner.disposables.length,0);assert.equal(engine.getGraphTelemetry().nodes,0);
  return{captureMs:elapsed,releaseMs:0,hooks:1,combined:true};
 }finally{stop();}
}
function sample(engine,size,mode){
 const contexts=[],indices=Array.from({length:size},(_,i)=>i);let seed=0x5eed;
 if(mode==='shuffled')for(let i=size-1;i>0;i--){seed=(Math.imul(seed,1664525)+1013904223)>>>0;const j=seed%(i+1);[indices[i],indices[j]]=[indices[j],indices[i]];}
 let stop=()=>{},owner,captureMs=0;
 try{
  engine.createRoot(dispose=>{stop=dispose;owner=engine.getCurrentOwner();const start=performance.now();for(let i=0;i<size;i++)contexts.push(engine.captureReactiveContext());captureMs=performance.now()-start;});
  const hooks=owner.disposables.length,start=performance.now();if(mode==='owner')stop();else for(const index of indices)contexts[index].dispose();const releaseMs=performance.now()-start;
  assert.equal(contexts.every(context=>!context.active),true);assert.equal(owner.disposables.length,0);assert.equal(engine.getGraphTelemetry().nodes,0);if(mode!=='owner')assert.equal(owner.disposed,false);
  return{captureMs,releaseMs,hooks};
 }finally{stop();}
}
const records=[];
for(const {size,mode} of [...sizes.flatMap(size=>['forward','shuffled','owner'].map(mode=>({size,mode}))),{size:50000,mode:'sequential-capture-dispose'}]){
 const samples=Object.fromEntries(Object.keys(variants).map(name=>[name,[]]));
 for(let round=-2;round<rounds;round++)for(const name of round%2===0?Object.keys(variants):Object.keys(variants).reverse()){
  const result=mode==='sequential-capture-dispose'?sequentialSample(variants[name],size):sample(variants[name],size,mode);if(round>=0)samples[name].push(result);
 }
 const result={size,mode,variants:{}};
 for(const [name,data]of Object.entries(samples))result.variants[name]={captureMedianMs:median(data.map(x=>x.captureMs)),releaseMedianMs:median(data.map(x=>x.releaseMs)),totalMedianMs:median(data.map(x=>x.captureMs+x.releaseMs)),hooks:data[0].hooks,samples:data};records.push(result);
}
const result={completedAt:new Date().toISOString(),node:process.version,platform:process.platform,arch:process.arch,rounds,warmups:2,compiledHashes,records};
if(values.output)await writeFile(resolve(values.output),JSON.stringify(result,null,2)+'\n');
console.log(JSON.stringify({node:result.node,rounds,records:records.map(({size,mode,variants})=>({size,mode,...Object.fromEntries(Object.entries(variants).map(([name,data])=>[name,{captureMs:+data.captureMedianMs.toFixed(3),releaseMs:+data.releaseMedianMs.toFixed(3),totalMs:+data.totalMedianMs.toFixed(3),hooks:data.hooks}]))}))},null,2));
