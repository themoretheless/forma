// Compile latency only: no GPU/FPS claims and no inference of JS allocation counts.
import {readFile,writeFile,mkdir} from 'node:fs/promises';
import {resolve} from 'node:path';
import {pathToFileURL} from 'node:url';
import {performance} from 'node:perf_hooks';
import {createHash} from 'node:crypto';
const modulePath=resolve(process.argv[3]??'src/components.js');
const {compileComponents,createComponentCompiler}=await import(pathToFileURL(modulePath));
const out=resolve(process.argv[2]??'.forma/perf/compiler-session');
const read=name=>readFile(new URL(`../vector-ui/examples/${name}`,import.meta.url),'utf8');
const components={'components/Button.ui':await read('Button.slots.ui'),'components/ImageButton.ui':await read('ImageButton.component.ui'),'assets/search.svg':await read('search.svg')};
const results=[];
for(const count of [1,64,256])for(const mode of ['one-shot','session']){
 const files={...components,'Demo.ui':`component Demo { Frame { width:900; height:600; ${Array.from({length:count},(_,i)=>`ImageButton { text:'Item ${i}'; width:180; height:50; }`).join(' ')} } }`};
 const compile=mode==='session'&&createComponentCompiler?createComponentCompiler():compileComponents;
 let result;const run=()=>result=compile(files,'Demo.ui');
 for(let i=0;i<15;i++)run();
 const iterations=count===1?300:count===64?10:3,samples=[];
 for(let sample=0;sample<5;sample++){
  global.gc?.();const start=performance.now();
  for(let i=0;i<iterations;i++)run();
  samples.push((performance.now()-start)/iterations);
 }
 results.push({count,mode,iterations,samples,medianMs:[...samples].sort((a,b)=>a-b)[2],fixtureSha256:createHash('sha256').update(JSON.stringify(files)).digest('hex'),generatedSha256:createHash('sha256').update(result.source+result.template).digest('hex')});
}
await mkdir(out,{recursive:true});
await writeFile(resolve(out,'compiler-session.json'),JSON.stringify({node:process.version,modulePath,gcExposed:!!global.gc,results},null,2)+'\n');
console.log(JSON.stringify(results.map(({count,mode,medianMs})=>({count,mode,medianMs})),null,2));
