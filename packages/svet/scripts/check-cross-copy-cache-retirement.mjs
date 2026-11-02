import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {resolve} from 'node:path';
import {pathToFileURL} from 'node:url';
import {createHash} from 'node:crypto';
assert.equal(typeof global.gc,'function','Use node --expose-gc');assert(process.argv[2]&&process.argv[3],'Expected output and installed-copy manifest');
const manifest=JSON.parse(await readFile(resolve(process.argv[3]),'utf8')),build=JSON.parse(await readFile(resolve(manifest.output,'metadata.json'),'utf8'));
for(const [path,sha] of Object.entries(build.artifactSha256))assert.equal(createHash('sha256').update(await readFile(resolve(manifest.output,path))).digest('hex'),sha);
const a=await import(pathToFileURL(resolve(manifest.output,'first/index.js')).href),b=await import(pathToFileURL(resolve(manifest.output,'second/index.js')).href),internal=await import(pathToFileURL(resolve(manifest.output,'first/reactivity.js')).href);
function allocate(){
 const core=a.createWasmCoreInstance();a.withWasmCore(core,()=>a.getGraphTelemetry());b.withWasmCore(core,()=>b.getGraphTelemetry());internal.retireReactiveContextCache(core);
 // Late cleanup in the foreign module must not repopulate a retired hot cache.
 b.withWasmCore(core,()=>b.getGraphTelemetry());return [new WeakRef(core),new WeakRef(core.memory)];
}
const refs=allocate();let controls=Array.from({length:4},()=>({live:true}));const controlRefs=controls.map(value=>new WeakRef(value));
async function collect(references){let alive;for(let i=0;i<10;i++){await new Promise(setImmediate);global.gc();await new Promise(setImmediate);alive=references.filter(ref=>ref.deref()).length;if(!alive)break;}return alive;}
const beforeForeignCacheEviction=await collect(refs);assert.equal(controlRefs.filter(ref=>ref.deref()).length,4);
// Diagnostic only: if the foreign hot cache was a retainer, selecting its
// ambient core should remove that root. This cannot turn a failed result green.
 b.getGraphTelemetry();const afterForeignCacheEviction=await collect(refs);controls=undefined;const controlsReleased=await collect(controlRefs);
assert.equal(afterForeignCacheEviction,0);assert.equal(controlsReleased,0);
const report={status:beforeForeignCacheEviction===0?'passed':'retained-in-foreign-cache',beforeForeignCacheEviction,afterForeignCacheEviction,controlsHeld:4,controlsReleased,build,probeSha256:createHash('sha256').update(await readFile(new URL(import.meta.url))).digest('hex'),scope:'Retirement in copy A plus late cleanup in copy B. Core/Memory WeakRefs, both modules held; optional B cache eviction diagnoses roots without qualifying the failed held phase.'};
await writeFile(resolve(process.argv[2]),JSON.stringify(report,null,2)+'\n');console.log(JSON.stringify({status:report.status,beforeForeignCacheEviction,afterForeignCacheEviction}));if(report.status!=='passed')process.exitCode=1;
