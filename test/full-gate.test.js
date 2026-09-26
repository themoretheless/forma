import {test} from 'node:test';
import assert from 'node:assert/strict';
import {steps,evaluate,summary,provenance} from '../scripts/full-gate.mjs';

const TARGETS=['aarch64-apple-darwin','x86_64-unknown-linux-gnu'];
const VITE='node_modules/vite/bin/vite.js';
const matrix=(overrides={})=>steps({platform:'linux',installedTargets:TARGETS,viteEntry:VITE,...overrides});
const step=(id,overrides)=>matrix(overrides).list.find(item=>item.id===id);
test('the gate covers every runner platform CI uses',()=>{
 const {list,skipped}=matrix();
 const ids=list.map(step=>step.id);
 assert.deepEqual(ids.filter(id=>id.startsWith('native-')),['native-host','native-x86_64-unknown-linux-gnu']);
 assert.equal(skipped.length,1);
 assert.match(skipped[0].reason,/rustup target add/);
 assert.equal(new Set([...ids,...skipped.map(step=>step.id)]).size,ids.length+skipped.length);
 assert.equal(list.some(step=>step.id==='studio-legacy'),false,'the WebView host is a macOS job');
 const mac=matrix({platform:'darwin'});
 assert.equal(mac.list.at(-1).id,'studio-legacy');
 // Without the fixture the host cannot generate its actions module, and the step fails locally
 // while CI passes: the env is part of the step, not of the machine.
 assert.deepEqual(mac.list.at(-1).env,{FORMA_ACTIONS:'.github/fixtures/actions.rs'});
 assert.equal(list.every(step=>step.id!==step.name),true);
});
test('the gate runs the studio steps CI runs before it publishes',()=>{
 // CI checks the versions, runs the suite, then builds the bundle release.yml uploads; a local
 // pass that skips the build leaves the shipped bundle unverified.
 assert.deepEqual(matrix().list.map(item=>item.id).slice(0,3),['version-check','studio-js','studio-build']);
 assert.deepEqual(step('version-check').args,['scripts/versions.mjs','check']);
 assert.deepEqual(step('studio-build').args,[VITE,'build'],'vite runs through its own entry, so no shell is involved');
 // An uninstalled vite is a skipped step with the command that fixes it, not a failing one.
 const {list,skipped}=matrix({viteEntry:null});
 assert.equal(list.some(item=>item.id==='studio-build'),false);
 assert.deepEqual(skipped.at(-1),{id:'studio-build',reason:'vite is not installed; run npm ci'});
});
test('a command that reports nothing is not a pass',()=>{
 const suite=step('studio-js');
 assert.deepEqual(evaluate(suite,{status:0,output:''}),{ok:false,detail:'no test summary in output'});
 assert.equal(evaluate(suite,{status:0,output:'# tests 280\n# pass 280\n# fail 0'}).ok,true);
 assert.equal(evaluate(suite,{status:0,output:'# tests 280\n# pass 279\n# fail 1'}).ok,false);
 assert.equal(evaluate(suite,{status:1,output:'# tests 280\n# pass 280\n# fail 0'}).ok,false);
 assert.equal(summary('# suites 0'),null);
 // The versions CLI once exited 0 without printing anything, which would have waved a release tag
 // through: a check that says nothing has not checked the thing it names.
 assert.deepEqual(evaluate(step('version-check'),{status:0,output:'  \n'}),{ok:false,detail:'printed nothing'});
 assert.equal(evaluate(step('version-check'),{status:0,output:'{"runtime":"0.1.0"}'}).ok,true);
 assert.equal(evaluate({},{status:0,output:''}).ok,true);
});
test('a built bundle that is not the tested runtime is not a pass',()=>{
 const build=step('studio-build');
 assert.match(evaluate(build,{status:0,output:'built',dist:[]}).detail,/match public/);
 assert.deepEqual(evaluate(build,{status:0,output:'',dist:['forma_bg.wasm missing','build-info.json']}).ok,false);
 assert.match(evaluate(build,{status:0,output:'',dist:['forma_bg.wasm missing','build-info.json']}).detail,/stale in dist/);
 // A failed build is reported by its exit code, not by the drift of whatever dist already held.
 assert.deepEqual(evaluate(build,{status:2,output:'',dist:['everything']}),{ok:false,detail:'exit 2'});
});
test('runtime provenance reads the built package',()=>{
 const source=provenance();
 assert.ok(['fresh','stale','missing'].includes(source.state),source.detail);
 if(source.state!=='missing')assert.ok(source.detail.length>4);
});
