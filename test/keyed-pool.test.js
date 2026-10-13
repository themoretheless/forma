import {test} from 'node:test';
import assert from 'node:assert/strict';
import {createKeyedPool} from '../src/keyed-pool.js';

test('a keyed pool hands back the latest item under a key and discards what it drops',()=>{
 const discarded=[],pool=createKeyedPool({maxEntries:3,maxBytes:100,discard:item=>discarded.push(item)});
 pool.put('a',1,10);pool.put('a',2,10);pool.put('b',3,10);
 assert.equal(pool.take('a'),2);assert.equal(pool.take('a'),1);assert.equal(pool.take('a'),undefined);
 pool.put('a',4,10);pool.put('c',5,10);pool.put('d',6,10);
 assert.deepEqual(discarded,[3],'past the entry limit the oldest item goes first');
 pool.put('e',7,81);assert.deepEqual(discarded,[3,4,5],'past the byte limit too');
 assert.equal(pool.put('f',8,101),false);assert.deepEqual(discarded,[3,4,5,8],'an item over the whole budget is refused');
 assert.deepEqual(pool.snapshot(),{entries:2,bytes:91,maxEntries:3,maxBytes:100});
 pool.clear();assert.deepEqual(discarded,[3,4,5,8,6,7]);assert.equal(pool.snapshot().entries,0);assert.equal(pool.take('d'),undefined);
});
