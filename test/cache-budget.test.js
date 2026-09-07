import {test} from 'node:test';
import assert from 'node:assert/strict';
import {createCacheBudget,estimateCacheBytes} from '../src/cache-budget.js';

test('shared budget evicts the least recently used entry across namespaces',()=>{
  const cache=createCacheBudget({maxEntries:2,maxBytes:10000});
  cache.set('tree:a',{text:'a'});cache.set('compiler:b',{text:'b'});
  assert.equal(cache.get('tree:a').text,'a');cache.set('compiler:c',{text:'c'});
  assert.equal(cache.get('compiler:b'),undefined);assert.equal(cache.get('tree:a').text,'a');
  assert.equal(cache.snapshot().evictions,1);
});

test('byte budget includes nested AST and dependency sources, handles oversized replacement',()=>{
  const value={nodes:[{text:'hello'}],deps:new Map([['source','x'.repeat(100)]])};
  const maxBytes=estimateCacheBytes(value)+estimateCacheBytes('key');
  const cache=createCacheBudget({maxBytes});assert.equal(cache.set('key',value),true);
  assert.equal(cache.snapshot().estimatedBytes,maxBytes);
  assert.equal(cache.set('key',{text:'x'.repeat(maxBytes)}),false);
  assert.equal(cache.get('key'),undefined);assert.equal(cache.snapshot().estimatedBytes,0);
  for(let i=0;i<100;i++){cache.set('k'+i,{text:'a'});assert.ok(cache.snapshot().estimatedBytes<=maxBytes);}
  cache.clear();assert.equal(cache.snapshot().entries,0);assert.equal(cache.snapshot().estimatedBytes,0);
});

test('accounting handles shared references and cycles; disabled cache retains nothing',()=>{
  const value={};value.self=value;assert.ok(Number.isFinite(estimateCacheBytes(value)));
  const cache=createCacheBudget({maxBytes:0});assert.equal(cache.set('a',value),false);
  assert.equal(cache.snapshot().entries,0);
});
