import assert from 'node:assert/strict';
import {readFile,writeFile,readdir} from 'node:fs/promises';
import {createHash} from 'node:crypto';
import {resolve} from 'node:path';
import {pathToFileURL} from 'node:url';
import {JSDOM} from 'jsdom';
assert(global.gc,'Use --expose-gc');const [installation,output]=process.argv.slice(2);assert(installation&&output);
const directory=resolve(installation,'dist/package'),engine=await import(pathToFileURL(resolve(directory,'index.js'))),runtime=await import(pathToFileURL(resolve(directory,'server.js')));
const dom=new JSDOM('<!doctype html><body></body>');for(const key of ['window','document','Node','Element'])globalThis[key]=dom.window[key];
const source="component App { state count: Number = 1; prop limit: Number = 5; let doubled = state.count * 2; Column { Button { text:doubled; clicked -> state.increment('count',1); } } }";
const kinds=['state-signal','prop-signal','computed','signal-peek','action','event-unsubscribe'],held=[],refs=[],graph=[];
let controls=Array.from({length:4},()=>({live:true}));const controlRefs=controls.map(value=>new WeakRef(value));
for(const kind of kinds)for(let n=0;n<4;n++)await runtime.runWithIsolatedContextAsync(()=>{
 const core=engine.getWasmCore();refs.push({kind,core:new WeakRef(core),memory:new WeakRef(core.memory)});
 const instance=engine.compileFormaAst(engine.parseFormaSource(source)),container=document.createElement('div'),handle=instance.mount(container);
 const values={'state-signal':instance.state.count,'prop-signal':instance.props.limit,computed:instance.computedLets.doubled,'signal-peek':instance.state.count.peek,action:instance.actions.increment,'event-unsubscribe':instance.onEvent('probe',()=>{})};
 held.push({kind,value:values[kind]});
 instance.dispose();handle.unmount();graph.push(engine.getGraphTelemetry().nodes);assert.equal(graph.at(-1),0);
});
async function collect(){for(let n=0;n<16;n++){await new Promise(setImmediate);global.gc();await new Promise(setImmediate);}}
await collect();const retained=Object.fromEntries(kinds.map(kind=>[kind,{cores:refs.filter(row=>row.kind===kind&&row.core.deref()).length,memories:refs.filter(row=>row.kind===kind&&row.memory.deref()).length}]));
assert.equal(controlRefs.filter(ref=>ref.deref()).length,4);controls=null;await collect();assert.equal(controlRefs.filter(ref=>ref.deref()).length,0);
for(const row of held){if(row.kind==='action')row.value('count',1);else if(row.kind==='event-unsubscribe')row.value();else assert.equal(row.value(),row.kind==='prop-signal'?5:row.kind==='computed'?2:1);}
const artifactSha256={};for(const name of (await readdir(directory)).sort())if(/\.(js|wasm)$/.test(name))artifactSha256[name]=createHash('sha256').update(await readFile(resolve(directory,name))).digest('hex');
const report={status:Object.values(retained).every(row=>row.cores===0&&row.memories===0)?'passed':'retained',retained,graph,heldReferences:held.length,controlsHeld:4,controlsReleased:0,artifactSha256,probeSha256:createHash('sha256').update(await readFile(new URL(import.meta.url))).digest('hex'),node:process.version,scope:'Installed Node isolated contexts+JSDOM; separately captured state/prop/computed/peek/action/unsubscribe handles after Forma disposal; explicit forced GC.'};await writeFile(output,JSON.stringify(report,null,2)+'\n');console.log(JSON.stringify({status:report.status,retained}));dom.window.close();if(report.status!=='passed')process.exitCode=1;
