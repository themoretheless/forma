import {test} from 'node:test';
import assert from 'node:assert/strict';
import {steps,evaluate,summary,provenance} from '../scripts/full-gate.mjs';

const TARGETS=['aarch64-apple-darwin','x86_64-unknown-linux-gnu'];
test('the gate covers every runner platform CI uses',()=>{
 const {list,skipped}=steps({platform:'linux',installedTargets:TARGETS});
 const ids=list.map(step=>step.id);
 assert.deepEqual(ids.filter(id=>id.startsWith('native-')),['native-host','native-x86_64-unknown-linux-gnu']);
 assert.equal(skipped.length,1);
 assert.match(skipped[0].reason,/rustup target add/);
 assert.equal(new Set([...ids,...skipped.map(step=>step.id)]).size,ids.length+skipped.length);
 assert.equal(list.some(step=>step.id==='studio-legacy'),false,'the WebView host is a macOS job');
 const mac=steps({platform:'darwin',installedTargets:TARGETS});
 assert.equal(mac.list.at(-1).id,'studio-legacy');
 // Without the fixture the host cannot generate its actions module, and the step fails locally
 // while CI passes: the env is part of the step, not of the machine.
 assert.deepEqual(mac.list.at(-1).env,{FORMA_ACTIONS:'.github/fixtures/actions.rs'});
 assert.equal(list.every(step=>step.id!==step.name),true);
});
test('a command that reports nothing is not a pass',()=>{
 const step=steps({platform:'linux',installedTargets:TARGETS}).list[0];
 assert.deepEqual(evaluate(step,{status:0,output:''}),{ok:false,detail:'no test summary in output'});
 assert.equal(evaluate(step,{status:0,output:'# tests 280\n# pass 280\n# fail 0'}).ok,true);
 assert.equal(evaluate(step,{status:0,output:'# tests 280\n# pass 279\n# fail 1'}).ok,false);
 assert.equal(evaluate(step,{status:1,output:'# tests 280\n# pass 280\n# fail 0'}).ok,false);
 assert.equal(summary('# suites 0'),null);
 assert.equal(evaluate({requiresSummary:false},{status:0,output:''}).ok,true);
});
test('runtime provenance reads the built package',()=>{
 const source=provenance();
 assert.ok(['fresh','stale','missing'].includes(source.state),source.detail);
 if(source.state!=='missing')assert.ok(source.detail.length>4);
});
