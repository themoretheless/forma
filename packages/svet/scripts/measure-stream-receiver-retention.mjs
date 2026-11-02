import assert from 'node:assert/strict';
import {pathToFileURL} from 'node:url';
import {resolve} from 'node:path';
import {writeFile} from 'node:fs/promises';
import {JSDOM} from 'jsdom';
const dom=new JSDOM('<!doctype html><body></body>');
for(const key of ['window','document','Node','HTMLElement','Element','Comment','Text','DocumentFragment'])globalThis[key]=dom.window[key];
const engine=await import(process.argv[3]?pathToFileURL(resolve(process.argv[3],'dist/package/index.js')).href:'../dist/package/index.js');
assert.equal(typeof global.gc,'function','Run node --expose-gc');
const retained=[];
function allocate(dispose,index){
  const container=document.createElement('main');
  const receiver=engine.createSsrStreamReceiver(container);
  receiver.apply({version:1,sequence:0,streamId:'retention-probe',payload:{type:'shell',html:'<p>probe</p>'}});
  const mode=['dispose','reset','replacement','protocol-error'][index%4];
  if(mode==='reset')engine.resetReactiveSystem();
  else if(mode==='replacement')engine.createSsrStreamReceiver(container).dispose();
  else if(mode==='protocol-error')assert.throws(()=>receiver.apply({version:2}),/version/);
  else receiver.dispose();
  assert.equal(receiver.active,false);receiver.dispose();
  assert.throws(()=>receiver.apply({get version(){throw Error('payload accessed');}}),/disposed/);
  if(!dispose)retained.push(receiver);
  return new WeakRef(container);
}
const retainedRefs=Array.from({length:32},(_,i)=>allocate(false,i));
const controlRefs=Array.from({length:32},(_,i)=>allocate(true,i));
async function collect(){
  await new Promise(setImmediate);
  for(let round=0;round<10;round++){global.gc();await new Promise(setImmediate);}
  return {retainedHandleContainers:retainedRefs.filter(r=>r.deref()).length,controlContainers:controlRefs.filter(r=>r.deref()).length};
}
const beforeRelease=await collect();
retained.length=0;
const afterRelease=await collect();
assert.equal(beforeRelease.retainedHandleContainers,0,'Disposed receiver handles retain DOM containers');
assert.equal(beforeRelease.controlContainers,0,'Control containers must be collectible');
assert.equal(afterRelease.retainedHandleContainers,0,'Containers must collect after releasing handles');
const result={measuredAt:new Date().toISOString(),node:process.version,receivers:32,beforeRelease,afterRelease,status:beforeRelease.retainedHandleContainers?'retention-reproduced':'passed',scope:'Disposed/reset/replaced/protocol-error receivers with caller-retained handles; detached JSDOM containers. Does not measure iterator, active complete receiver, RSS or browser GC.'};
if(process.argv[2])await writeFile(process.argv[2],JSON.stringify(result,null,2)+'\n');
console.log(JSON.stringify(result,null,2));
dom.window.close();
