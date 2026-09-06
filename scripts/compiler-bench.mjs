// Compiler latency and sampled heap, not renderer FPS or allocation counts.
import {readFile, mkdir, writeFile} from 'node:fs/promises';
import {resolve} from 'node:path';
import {performance} from 'node:perf_hooks';
import {createHash} from 'node:crypto';
import {compileComponents} from '../src/components.js';
const read=name=>readFile(new URL(`../vector-ui/examples/${name}`,import.meta.url),'utf8');
const files={'components/Button.ui':await read('Button.slots.ui'),'components/ImageButton.ui':await read('ImageButton.component.ui'),'ui/Demo.ui':await read('ImageButton.ui'),'assets/search.svg':await read('search.svg')};
const out=resolve(process.argv[2]??'.forma/perf/compiler');
const run=()=>compileComponents(files,'ui/Demo.ui');
for(let i=0;i<100;i++)run();
const samples=[];
let retained;
for(let s=0;s<7;s++){
 global.gc?.();
 const before=process.memoryUsage(),start=performance.now();
 for(let i=0;i<500;i++)retained=run();
 const elapsed=performance.now()-start,after=process.memoryUsage();
 samples.push({msPerCompile:elapsed/500,heapDeltaBytes:after.heapUsed-before.heapUsed,rssBytes:after.rss});
}
const result={node:process.version,fixtureSha256:createHash('sha256').update(JSON.stringify(files)).digest('hex'),generatedSha256:createHash('sha256').update(retained.source+retained.template).digest('hex'),gcExposed:!!global.gc,samples,medianMs:[...samples].map(s=>s.msPerCompile).sort((a,b)=>a-b)[3],notes:['Seven batches, 500 compiles each after 100 warmups','Heap deltas include temporary garbage; not allocated-byte counts or retained memory','Renderer, GPU and font metrics are not measured by this fixed-size fixture']};
await mkdir(out,{recursive:true});await writeFile(resolve(out,'compiler.json'),JSON.stringify(result,null,2));
console.log(JSON.stringify(result,null,2));
