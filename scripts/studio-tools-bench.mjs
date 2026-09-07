// Source-processing microbenchmarks, not browser layout/paint timings.
import {mkdir,writeFile} from 'node:fs/promises';
import {resolve} from 'node:path';
import {pathToFileURL} from 'node:url';
import {performance} from 'node:perf_hooks';
import {createHash} from 'node:crypto';
const root=resolve(process.argv[3]??'.');
const {blockRanges}=await import(pathToFileURL(resolve(root,'src/folding.js')));
const {treeRows}=await import(pathToFileURL(resolve(root,'src/control-tree.js')));
const out=resolve(process.argv[2]??'.forma/perf/studio-tools');
const digest=value=>createHash('sha256').update(JSON.stringify(value)).digest('hex');
const roots=[{id:'root',label:'Scene',children:Array.from({length:10000},(_,i)=>({id:`n${i}`,label:`Control ${i}`,children:[]}))}];
const collapsed=new Set(['root']);
const nested='{'.repeat(1024)+'x'.repeat(65536)+'\n'+'}'.repeat(1024);
const flat=Array.from({length:4096},()=>"Frame { Text { text:'Label'; }\n}").join('\n');
const results=[];
for(const [name,run,iterations,fixture] of [
 ['tree_collapsed',()=>treeRows(roots,collapsed),500,roots],
 ['tree_expanded',()=>treeRows(roots),20,roots],
 ['fold_nested',()=>blockRanges(nested),100,nested],
 ['fold_flat',()=>blockRanges(flat),50,flat],
]){
 let result;for(let i=0;i<10;i++)result=run();
 const samples=[];
 for(let batch=0;batch<5;batch++){
  global.gc?.();const start=performance.now();
  for(let i=0;i<iterations;i++)result=run();
  samples.push((performance.now()-start)/iterations);
 }
 results.push({name,iterations,samples,medianMs:[...samples].sort((a,b)=>a-b)[2],fixtureSha256:digest(fixture),resultSha256:digest(result)});
}
await mkdir(out,{recursive:true});
await writeFile(resolve(out,'tools.json'),JSON.stringify({node:process.version,root,results},null,2)+'\n');
console.log(JSON.stringify(results.map(({name,medianMs})=>({name,medianMs})),null,2));
