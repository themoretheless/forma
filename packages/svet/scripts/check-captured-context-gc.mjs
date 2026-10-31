import assert from 'node:assert/strict';
import {writeFile} from 'node:fs/promises';
import {captureReactiveContext,createRoot,getCurrentOwner,getGraphTelemetry} from '../dist/package/index.js';
if(typeof global.gc!=='function')throw Error('Run with node --expose-gc after building the package');
const retained=[],owners=[];
function allocate(){
 let stop,owner;
 createRoot(dispose=>{stop=dispose;owner=getCurrentOwner();const contexts=Array.from({length:4},()=>captureReactiveContext());contexts.forEach(context=>context.dispose());retained.push(...contexts);});
 stop();owners.push(new WeakRef(owner));
}
for(let i=0;i<1000;i++)allocate();
await new Promise(setImmediate);
let alive=1000,round=0;
for(;round<10;round++){
 global.gc();await new Promise(setImmediate);
 alive=owners.filter(owner=>owner.deref()!==undefined).length;
 if(alive===0)break;
 await new Promise(setImmediate);
}
assert.equal(alive,0);assert.equal(retained.length,4000);assert.equal(retained.every(context=>!context.active),true);assert.equal(getGraphTelemetry().nodes,0);
const result={node:process.version,ownerRoots:owners.length,retainedDisposedCaptures:retained.length,liveOwnerRoots:alive,gcRounds:round+1,graphNodes:getGraphTelemetry().nodes};
if(process.argv[2])await writeFile(process.argv[2],JSON.stringify(result,null,2)+'\n');
console.log(JSON.stringify(result,null,2));
