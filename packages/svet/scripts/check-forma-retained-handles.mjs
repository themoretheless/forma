import assert from 'node:assert/strict';
import {readFile,writeFile,readdir} from 'node:fs/promises';
import {createHash} from 'node:crypto';
import {resolve} from 'node:path';
import {pathToFileURL} from 'node:url';
import {JSDOM} from 'jsdom';
assert(global.gc,'Use --expose-gc');const [installation,output]=process.argv.slice(2);assert(installation&&output);
const directory=resolve(installation,'dist/package'),engine=await import(pathToFileURL(resolve(directory,'index.js'))),runtime=await import(pathToFileURL(resolve(directory,'server.js')));
const dom=new JSDOM('<!doctype html><body></body>');for(const key of ['window','document','Node','Element'])globalThis[key]=dom.window[key];
const source="component App { state count: Number = 1; let doubled = state.count * 2; Column { Button { text:doubled; clicked -> state.increment('count',1); } } }";
const kinds=['instance','mount-handle','bound-render','button'],held=[],refs=[],graph=[];
let controls=Array.from({length:4},()=>({live:true}));const controlRefs=controls.map(value=>new WeakRef(value));
for(const kind of kinds)for(let n=0;n<4;n++)await runtime.runWithIsolatedContextAsync(()=>{
 const core=engine.getWasmCore();refs.push({kind,core:new WeakRef(core),memory:new WeakRef(core.memory)});
 const instance=engine.compileFormaAst(engine.parseFormaSource(source)),container=document.createElement('div'),handle=instance.mount(container);
 held.push({kind,value:kind==='instance'?instance:kind==='mount-handle'?handle:kind==='bound-render'?instance.render.bind(instance):container.querySelector('button')});
 instance.dispose();handle.unmount();graph.push(engine.getGraphTelemetry().nodes);assert.equal(graph.at(-1),0);
});
async function collect(){for(let n=0;n<16;n++){await new Promise(setImmediate);global.gc();await new Promise(setImmediate);}}
await collect();const retained=Object.fromEntries(kinds.map(kind=>[kind,{cores:refs.filter(row=>row.kind===kind&&row.core.deref()).length,memories:refs.filter(row=>row.kind===kind&&row.memory.deref()).length}]));
assert.equal(controlRefs.filter(ref=>ref.deref()).length,4);controls=null;await collect();assert.equal(controlRefs.filter(ref=>ref.deref()).length,0);
for(const row of held){if(row.kind==='instance')row.value.dispose();else if(row.kind==='mount-handle')row.value.unmount();else if(row.kind==='bound-render')assert.throws(row.value,/Forma instance is disposed/);else row.value.click();}
const artifactSha256={};for(const name of (await readdir(directory)).sort())if(/\.(js|wasm)$/.test(name))artifactSha256[name]=createHash('sha256').update(await readFile(resolve(directory,name))).digest('hex');
const report={status:Object.values(retained).every(row=>row.cores===0&&row.memories===0)?'passed':'retained',retained,graph,heldReferences:held.length,controlsHeld:4,controlsReleased:0,artifactSha256,probeSha256:createHash('sha256').update(await readFile(new URL(import.meta.url))).digest('hex'),node:process.version,scope:'Installed Node isolated contexts+JSDOM; retained disposed Forma instances, mount handles, bound render methods and detached buttons; explicit forced GC.'};await writeFile(output,JSON.stringify(report,null,2)+'\n');console.log(JSON.stringify({status:report.status,retained}));dom.window.close();if(report.status!=='passed')process.exitCode=1;
