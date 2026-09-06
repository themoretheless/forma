import {test} from 'node:test';
import assert from 'node:assert/strict';
import {bracketLevels} from '../src/brackets.js';
test('bracket pairs share depth for all kinds',()=>{
 assert.deepEqual(bracketLevels('{[()]}').map(x=>x.level),[0,1,2,2,1,0]);
});
test('ignore brackets in strings and comments',()=>{
 assert.deepEqual(bracketLevels(`{ text: '}'; // ]\n /* ( */ }`).map(x=>x.level),[0,0]);
});
test('mismatched closing brackets marked without breaking outer pair',()=>{
 assert.deepEqual(bracketLevels('{]}').map(x=>x.level),[0,-1,0]);
});
