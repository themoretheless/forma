import {test} from 'node:test';
import assert from 'node:assert/strict';
import {rememberDocument} from '../src/editor.js';

test('remembered editor states are capped, evicting the least recently left file',()=>{
 const documents=new Map();
 for(const path of ['a','b','c'])rememberDocument(documents,path,path.toUpperCase(),2);
 assert.deepEqual([...documents.keys()],['b','c']);
 rememberDocument(documents,'b','B2',2);rememberDocument(documents,'d','D',2);
 assert.deepEqual([...documents.entries()],[['b','B2'],['d','D']],'leaving a file again refreshes it');
});
