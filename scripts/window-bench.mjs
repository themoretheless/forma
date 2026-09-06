// Opens temporary labeled windows; use after perf-bench.mjs has generated scenes.
import {readFile,writeFile} from 'node:fs/promises';
import {spawnSync} from 'node:child_process';
import {resolve} from 'node:path';
import {fileURLToPath} from 'node:url';
const root=fileURLToPath(new URL('../',import.meta.url));
if(!process.argv[2])throw Error('Usage: node scripts/window-bench.mjs .forma/perf/RUN');
const out=resolve(root,process.argv[2]);
await readFile(resolve(out,'image.ui'));
const run=(cmd,args)=>{const r=spawnSync(cmd,args,{cwd:root,encoding:'utf8',timeout:120000,maxBuffer:8*1024*1024});if(r.status!==0)throw Error(r.stderr||String(r.error));return r.stdout;};
console.error('Building native window benchmark…');
run('cargo',['build','--offline','--release','--manifest-path','vector-ui/Cargo.toml','--features','native','--example','window_bench']);
const rows=[];
for(let repetition=1;repetition<=3;repetition++)for(const [width,height] of [[800,400],[1920,1080],[3840,2160]]){
 console.error(`Window ${width}×${height}, 5 seconds, repetition ${repetition}/3`);
 let result;
 try {
  const lines=run(resolve(root,'vector-ui/target/release/examples/window_bench'),[resolve(out,'image.ui'),resolve(out,'image.template.ui'),String(width),String(height),'5']).trim().split('\n');
  result=JSON.parse(lines.at(-1));
 } catch(error) {
  await writeFile(resolve(out,'window-failure.json'),JSON.stringify({createdAt:new Date().toISOString(),repetition,width,height,error:error.message,nativeFps:null},null,2));
  throw error;
 }
 rows.push({repetition,...result});
 await writeFile(resolve(out,'windows.json'),JSON.stringify(rows,null,2));
}
console.log(resolve(out,'windows.json'));
