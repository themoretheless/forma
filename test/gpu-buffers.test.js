import {test} from 'node:test';
import assert from 'node:assert/strict';
import {createStoragePool} from '../src/gpu-buffers.js';
function fixture(limit=1024){
 const created=[],writes=[];
 const device={limits:{maxStorageBufferBindingSize:limit},queue:{writeBuffer(buffer,offset,data){writes.push([buffer,data.byteLength]);}},createBuffer({size}){const b={size,destroyed:false,destroy(){this.destroyed=true;}};created.push(b);return b;}};
 return {pool:createStoragePool(device,128),created,writes,device};
}
test('GPU buffers retain capacity and identity across shrink/grow within capacity',()=>{
 const {pool,created,writes}=fixture();
 assert.equal(pool.update(0,new Uint32Array(60)),true);
 const first=pool.buffers[0];assert.equal(first.size,256);
 assert.equal(pool.update(0,new Uint32Array(12)),false);
 assert.equal(pool.update(0,new Uint32Array(64)),false);
 assert.equal(pool.buffers[0],first);assert.equal(created.length,1);assert.equal(writes.length,3);
 assert.equal(pool.update(0,new Uint32Array(65)),true);
 assert.equal(pool.buffers[0].size,512);assert.equal(first.destroyed,true);
 assert.deepEqual(pool.snapshot(),{bufferAllocations:2,bufferBytes:512,uploadedBytes:804});
 pool.destroy();assert.ok(created.every(b=>b.destroyed));
});
test('capacity is capped at device budget and oversize payloads do not mutate pool',()=>{
 const {pool,created}=fixture(768);pool.update(0,new Uint32Array(170));assert.equal(pool.buffers[0].size,768);
 assert.throws(()=>pool.update(0,new Uint32Array(193)),/budget/);assert.equal(created.length,1);
 assert.throws(()=>pool.update(0,new Uint8Array(3)),/budget/);
});
test('failed buffer growth destroys only the replacement',()=>{
 const {pool,device,created}=fixture();pool.update(0,new Uint32Array(10));
 device.queue.writeBuffer=()=>{throw Error('upload failed');};
 assert.throws(()=>pool.update(0,new Uint32Array(100)),/upload failed/);
 assert.equal(pool.buffers[0],created[0]);assert.equal(created[0].destroyed,false);assert.equal(created[1].destroyed,true);
});
