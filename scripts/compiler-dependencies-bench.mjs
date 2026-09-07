// Source/link preparation latency, not browser FPS or VM heap measurement.
import {mkdir,writeFile} from 'node:fs/promises';
import {resolve} from 'node:path';
import {pathToFileURL} from 'node:url';
import {createHash} from 'node:crypto';
import {performance} from 'node:perf_hooks';
const modulePath=resolve(process.argv[3]??'src/components.js');
const {createComponentCompiler}=await import(pathToFileURL(modulePath));
const output=resolve(process.argv[2]??'.forma/perf/compiler-dependencies');
const results=[];
for(const mode of ['warm','override-edit']){
  const files={'Demo.ui':"component Demo { Frame { C23 { text:state.label; } } }",
    'components/C0.ui':"component C0 { Rectangle { Border { key:'outline'; width:1; background:#abcdef; } Text { text:props.text; } } }"};
  for(let i=1;i<24;i++){
    files[`components/C${i}.ui`]=`component C${i} : C${i-1} { override outline from '../styles/B${i}.ui'; }`;
    files[`styles/B${i}.ui`]=`Border { width:${i%4+1}; background:#abcdef; }`;
  }
  const fixtureSha256=createHash('sha256').update(JSON.stringify(files)).digest('hex');
  const compile=createComponentCompiler();let result;
  function run(i){
    if(mode==='override-edit')files['styles/B23.ui']=`Border { width:${i%2+1}; background:#abcdef; }`;
    result=compile(files,'Demo.ui',{label:'State '+i%2});
  }
  for(let i=0;i<20;i++)run(i);
  const iterations=100,samples=[];
  for(let batch=0;batch<5;batch++){
    global.gc?.();const start=performance.now();
    for(let i=0;i<iterations;i++)run(i);
    samples.push((performance.now()-start)/iterations);
  }
  results.push({mode,iterations,samples,medianMs:[...samples].sort((a,b)=>a-b)[2],fixtureSha256,
    generatedSha256:createHash('sha256').update(JSON.stringify(result)).digest('hex'),cache:compile.cacheStats?.()??null});
}
await mkdir(output,{recursive:true});
await writeFile(resolve(output,'dependencies.json'),JSON.stringify({node:process.version,modulePath,results},null,2)+'\n');
console.log(JSON.stringify(results.map(({mode,medianMs,cache})=>({mode,medianMs,cache})),null,2));
