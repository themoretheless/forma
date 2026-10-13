import {test} from 'node:test';
import assert from 'node:assert/strict';
import {createStudioEvents,studioEventTypes} from '../src/studio-events.js';

test('subscribers receive the last published value immediately and every later one',()=>{
  const events=createStudioEvents(),seen=[];
  events.emit(studioEventTypes.mode,{mode:'design'});
  events.on(studioEventTypes.mode,detail=>seen.push(detail.mode));
  assert.deepEqual(seen,['design'],'a late subscriber starts from the current state');
  events.emit(studioEventTypes.mode,{mode:'interact'});
  assert.deepEqual(seen,['design','interact']);
  assert.deepEqual(events.current(studioEventTypes.mode),{mode:'interact'});
});

test('an aborted subscription stops receiving and an unknown type has no current value',()=>{
  const events=createStudioEvents(),controller=new AbortController();
  let count=0;
  events.on(studioEventTypes.problems,()=>{count++;},{signal:controller.signal});
  events.emit(studioEventTypes.problems,{count:1});
  controller.abort();
  events.emit(studioEventTypes.problems,{count:2});
  assert.equal(count,1);
  assert.equal(events.current(studioEventTypes.save),undefined);
});
