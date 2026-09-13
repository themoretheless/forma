import {test} from 'node:test';
import assert from 'node:assert/strict';
import {createNativeRunner} from '../native-runner.js';
import {createRustRunner} from '../rust-runner.js';

test('malformed runner requests report errors and release busy state without spawning',async()=>{
 for(const create of [createNativeRunner,createRustRunner]){
  const messages=[];const runner=create(message=>messages.push(message));
  for(const payload of [null,undefined,[],{},42]){
   await runner.run(payload);
   assert.equal(messages.at(-1).kind,'error');
   assert.doesNotMatch(messages.at(-1).text,/уже запущен|уже запущено/);
  }
  assert.equal(messages.length,5);runner.stop();
 }
});
