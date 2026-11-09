import { createHash } from 'node:crypto';
import { readFile, writeFile } from 'node:fs/promises';
import { pathToFileURL } from 'node:url';
import { resolve } from 'node:path';
const deps = process.argv[2],
  output = process.argv[3];
const { Miniflare } = await import(
  pathToFileURL(resolve(deps, 'node_modules/miniflare/dist/src/index.js')).href
);
const script = `import {AsyncLocalStorage} from 'node:async_hooks';
const context=new AsyncLocalStorage();
export default {async fetch(request){
 const pressure=Array.from({length:8},()=>new WebAssembly.Memory({initial:17}));
 const abort=new AbortController();const timer=setTimeout(()=>abort.abort(Error('deadline')),10);
 const mode=new URL(request.url).searchParams.get('mode');
 let started=false;const stream=new ReadableStream({async pull(controller){if(started)return;started=true;
  if(mode==='direct')abort.signal.addEventListener('abort',()=>controller.error(abort.signal.reason));
  else {
   const work=async()=>{
    const cancelled=new Promise((_,reject)=>abort.signal.addEventListener('abort',()=>reject(abort.signal.reason)));
    try{await Promise.race([(async()=>{await new Promise(()=>{});})(),cancelled]);}finally{await Promise.resolve();}
   };
   const pending=mode==='als'?context.run({},work):work();
   void pending.then(()=>{},error=>controller.error(error));
  }
 await Promise.resolve();controller.enqueue(new Uint8Array([1]));}},{highWaterMark:0});
 const reader=stream.getReader();await reader.read();let watchdog;
 try{if(new URL(request.url).searchParams.has('watchdog'))await Promise.race([reader.closed,new Promise((_,reject)=>watchdog=setTimeout(()=>reject(Error('watchdog')),100))]);else await reader.closed;return Response.json({status:'unexpected-success'});}
 catch(error){return Response.json({status:error.message==='deadline'?'passed':'failed',error:String(error)});}
 finally{clearTimeout(timer);clearTimeout(watchdog);if(pressure.length!==8)throw Error('pressure');}
}};`;
const mf = new Miniflare({
  modules: true,
  script,
  compatibilityDate: '2026-07-30',
  compatibilityFlags: ['nodejs_compat', 'enable_request_signal'],
  cf: false,
});
const report = {
  scriptVersion: 'native-closed-v2',
  wasmMemoryObjectsPerRequest: 8,
  wasmMemoryPages: 17,
  lazyPull: true,
  scope:
    'Minimal native workerd timer→AbortSignal→ReadableStream.error→reader.closed, no Svet/DOM, direct vs promise-hop vs ALS',
  node: process.version,
  results: [],
};
try {
  for (const mode of ['direct', 'hop', 'als'])
    for (const watchdog of [false, true]) {
      const rows = await Promise.all(
        Array.from({ length: 8 }, async () => {
          const response = await mf.dispatchFetch(
            'http://fixture.test/?mode=' + mode + (watchdog ? '&watchdog=1' : ''),
          );
          return { status: response.status, body: await response.text() };
        }),
      );
      report.results.push({ mode, watchdog, rows });
    }
} finally {
  await mf.dispose();
  report.sourceSha256 = createHash('sha256')
    .update(await readFile(new URL(import.meta.url)))
    .digest('hex');
  if (output) await writeFile(output, JSON.stringify(report, null, 2) + '\n');
  console.log(JSON.stringify(report));
}
