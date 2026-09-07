import {test} from 'node:test';
import assert from 'node:assert/strict';
import {blockRanges} from '../src/folding.js';
test('nested blocks ignore braces in strings and comments',()=>{
  const text="component A {\n // }\n Button {\n text: '}'; /* { */\n }\n}";
  const ranges=blockRanges(text);assert.equal(ranges.length,2);
  assert.equal(ranges[0].to,text.length-1);
  assert.equal(text.slice(ranges[1].from,ranges[1].to),"\n text: '}'; /* { */\n ");
});
test('incomplete outer block still permits complete inner folds',()=>{
  assert.equal(blockRanges('A {\n B {\n x: 1;\n }').length,1);
  assert.deepEqual(blockRanges('A {}'),[]);
});
test('line tracking includes escaped newlines and comments without folding single-line children',()=>{
  const text="A { B {} text:'a\\\nb'; C { /*\n*/ value:1; } D {} }";
  const ranges=blockRanges(text);
  assert.equal(ranges.length,2);
  assert.equal(ranges[0].to,text.length-1);
  assert.equal(text.slice(ranges[1].from,ranges[1].to),' /*\n*/ value:1; ');
});
test('large deeply nested blocks retain all boundaries',()=>{
  const count=4000,text='{'.repeat(count)+'\n'+'x'.repeat(100000)+'}'.repeat(count);
  const ranges=blockRanges(text);assert.equal(ranges.length,count);
  assert.deepEqual(ranges[0],{from:1,to:text.length-1});
  assert.deepEqual(ranges.at(-1),{from:count,to:count+100001});
});
