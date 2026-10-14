import {test} from 'node:test';
import assert from 'node:assert/strict';
import {createLatestOnly} from '../src/latest-only.js';

test('a load is current until a newer one begins or the owner cancels',async()=>{
 const loads=createLatestOnly(),built=[];
 const load=(name,delay)=>{const current=loads.begin();return new Promise(r=>setTimeout(r,delay)).then(()=>{if(current())built.push(name);});};
 await Promise.all([load('first',20),load('second',5)]);
 assert.deepEqual(built,['second'],'a load that finishes after a newer one builds nothing');
 const pending=load('disposed',5);loads.cancel();await pending;
 assert.deepEqual(built,['second'],'a load pending at dispose builds nothing');
});
