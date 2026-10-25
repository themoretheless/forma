import {test} from 'node:test';
import assert from 'node:assert/strict';
import {existsSync,readFileSync} from 'node:fs';
import {resolve} from 'node:path';
import {spawnSync} from 'node:child_process';
import {root} from '../scripts/versions.mjs';
import {steps} from '../scripts/full-gate.mjs';

test('svet package and svet-wasm crate live inside packages/svet and are covered by the gate',()=>{
 const pkgPath=resolve(root,'packages/svet/package.json');
 const cargoPath=resolve(root,'packages/svet/crates/svet-wasm/Cargo.toml');
 const wasmBinaryPath=resolve(root,'packages/svet/src/engine/wasm-binary.ts');
 assert.equal(existsSync(pkgPath),true,'packages/svet/package.json must exist');
 assert.equal(existsSync(cargoPath),true,'packages/svet/crates/svet-wasm/Cargo.toml must exist');
 assert.equal(existsSync(wasmBinaryPath),true,'packages/svet/src/engine/wasm-binary.ts must exist');

 const pkg=JSON.parse(readFileSync(pkgPath,'utf8'));
 assert.equal(pkg.name,'svet-engine');
 assert.equal(pkg.exports['.'],'./src/engine/index.ts');

 const gateSteps=steps({platform:'darwin',installedTargets:[],viteEntry:'node_modules/vite/bin/vite.js'}).list;
 const svetStep=gateSteps.find(item=>item.id==='svet-wasm');
 assert.ok(svetStep,'full-gate must include svet-wasm step');
 assert.deepEqual(svetStep.args,['test','--locked','--manifest-path','packages/svet/crates/svet-wasm/Cargo.toml','--lib','--tests']);
});

test('svet embedded WASM reactive DAG and LIS reconciler instantiate and execute in Node',async()=>{
 const wasmBinarySource=readFileSync(resolve(root,'packages/svet/src/engine/wasm-binary.ts'),'utf8');
 const base64Match=/WASM_BASE64\s*=\s*"([^"]+)"/.exec(wasmBinarySource);
 assert.ok(base64Match,'WASM_BASE64 must be present in wasm-binary.ts');
 const bytes=Buffer.from(base64Match[1],'base64');
 const mod=new WebAssembly.Module(bytes);
 const inst=new WebAssembly.Instance(mod,{});
 const core=inst.exports;

 core.svet_reset();
 const sigA=core.svet_create_node(0);
 const compB=core.svet_create_node(1);
 const compC=core.svet_create_node(1);
 const effD=core.svet_create_node(2);

 core.svet_link_1(compB,sigA);
 core.svet_link_1(compC,sigA);
 core.svet_link_2(effD,compB,compC);

 const flushRes=core.svet_trigger_and_flush(sigA);
 assert.equal(flushRes,effD+1,'diamond DAG must flush unique terminal effect once');
 assert.equal(core.svet_get_node_count(),4);

 // Verify O(n log n) LIS in WASM
 const input=[2,1,5,3,6,4,8,9,7];
 const inPtr=core.svet_prepare_lis_input(input.length);
 new Uint32Array(core.memory.buffer,inPtr,input.length).set(input);
 const lisLen=core.svet_compute_lis_buffered(input.length);
 assert.equal(lisLen,5);
 const outPtr=core.svet_get_lis_ptr();
 const lisIndices=Array.from(new Uint32Array(core.memory.buffer,outPtr,lisLen));
 const lisValues=lisIndices.map(idx=>input[idx]);
 assert.deepEqual(lisValues,[1,3,4,8,9]);
});

test('svet-wasm Rust test suite passes via cargo',()=>{
 const run=spawnSync('cargo',['test','--locked','--manifest-path','packages/svet/crates/svet-wasm/Cargo.toml','--lib','--tests'],{cwd:root,encoding:'utf8'});
 assert.equal(run.status,0,run.stderr||run.stdout);
 assert.match(run.stdout,/test result: ok\. 4 passed/);
});
