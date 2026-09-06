// Reproducible, isolated-process native renderer benchmark. Artifacts stay in .forma.
import {readFile,writeFile,mkdir} from 'node:fs/promises';
import {spawnSync} from 'node:child_process';
import {createHash} from 'node:crypto';
import {cpus,totalmem,platform,release,arch} from 'node:os';
import {resolve} from 'node:path';
import {fileURLToPath} from 'node:url';
import {compileComponents} from '../src/components.js';
const root=fileURLToPath(new URL('../',import.meta.url));
const out=resolve(root,process.argv[2]??`.forma/perf/${new Date().toISOString().replaceAll(':','-')}`);
const repeats=Number(process.env.FORMA_BENCH_REPEATS??3);
const frames=Number(process.env.FORMA_BENCH_FRAMES??120);
if(!Number.isInteger(repeats)||repeats<1||repeats>10||!Number.isInteger(frames)||frames<20||frames>10000)throw Error('Invalid repeats/frames');
await mkdir(out,{recursive:true});
const command=(cmd,args)=>{const r=spawnSync(cmd,args,{cwd:root,encoding:'utf8',maxBuffer:32*1024*1024});if(r.status!==0)throw Error(`${cmd}: ${r.stderr||r.error}`);return r.stdout;};
const read=name=>readFile(resolve(root,'vector-ui/examples',name),'utf8');
const files={'components/Button.ui':await read('Button.slots.ui'),'components/ImageButton.ui':await read('ImageButton.component.ui'),'ui/Demo.ui':await read('ImageButton.ui'),'assets/search.svg':await read('search.svg')};
const image=compileComponents(files,'ui/Demo.ui');
const baseSource='component Demo { Frame { width:900; height:600; radius:12; clip:true; padding:0; Button { width:900; height:600; } } }';
const labels=Array.from({length:40},(_,i)=>`ContentText { x:${16+(i%4)*216}; y:${12+Math.floor(i/4)*54}; width:210; height:48; text:'Forma интерфейс ${i}'; fontSize:16; color:#ffffff; }`).join('\n');
const text={source:baseSource,template:`component Button { Rectangle { radius:12; background:Brush { color:#28354b; hover:#38455b; transition:140ms; }; ${labels} PointerArea { clicked -> events.clicked(); } } }`};
const nested={source:baseSource,template:`component Button { Rectangle { radius:12; background:#28354b; ${Array.from({length:12},(_,i)=>`ContentClip { x:${i*2}; y:${i*2}; width:${900-i*4}; height:${600-i*4}; radius:12; }`).join(' ')} ${labels} ${'ContentClipEnd {} '.repeat(12)} } }`};
for(const [name,scene] of Object.entries({image,text,nested})){
 await writeFile(resolve(out,`${name}.ui`),scene.source);await writeFile(resolve(out,`${name}.template.ui`),scene.template);
}
console.error('Building release benchmark…');
command('cargo',['build','--offline','--release','--manifest-path','vector-ui/Cargo.toml','--features','gpu','--example','perf_bench']);
const binary=resolve(root,'vector-ui/target/release/examples/perf_bench');
const binaryHash=async()=>createHash('sha256').update(await readFile(binary)).digest('hex');
const benchmarkBinarySha256=await binaryHash();
const revision=command('git',['rev-parse','HEAD']).trim();
const metadata={createdAt:new Date().toISOString(),revision,dirty:!!command('git',['status','--porcelain']).trim(),platform:platform(),osRelease:release(),arch:arch(),cpu:cpus()[0]?.model,logicalCpus:cpus().length,systemRamBytes:totalmem(),rust:command('rustc',['--version']).trim(),repeats,frames,notes:['Offscreen serialized render throughput, not display FPS','GPU timings collected in a separate 30-frame pass after CPU/allocator sampling','Rust allocator counts exclude native driver/ObjC allocations','RSS is current process memory; GPU owned resources are not VRAM residency','DPI 2; fixed scene when viewport changes; resize creates replacement target (no window surface)','No renderer loop runs during synthetic idle; not an OS-window wakeup test']};
metadata.benchmarkBinarySha256=benchmarkBinarySha256;
await writeFile(resolve(out,'metadata.json'),JSON.stringify(metadata,null,2));
const cases=[
 ['image','gpu','animation',800,400],['image','gpu','animation',1920,1080],['image','gpu','animation',3840,2160],
 ['image','gpu','resize',3840,2160],['image','gpu','forced',3840,2160],['image','gpu','idle',800,400],
 ['text','gpu','animation',1920,1080],['nested','gpu','forced',1920,1080],
 ['image','cpu','animation',800,400],['image','cpu','animation',3840,2160],['image','cpu','resize',3840,2160],
];
const results=[];
for(let repetition=1;repetition<=repeats;repetition++)for(const [scene,backend,mode,w,h] of cases){
 console.error(`${repetition}/${repeats} ${scene} ${backend} ${mode} ${w}×${h}`);
 const lines=command(binary,[resolve(out,`${scene}.ui`),resolve(out,`${scene}.template.ui`),backend,mode,String(w),String(h),String(frames)]).trim().split('\n');
 const result={scene,repetition,...JSON.parse(lines.at(-1))};results.push(result);
 await writeFile(resolve(out,'results.json'),JSON.stringify(results,null,2));
}
if(await binaryHash()!==benchmarkBinarySha256)throw Error('Benchmark executable changed during measurement');
const median=v=>v.sort((a,b)=>a-b)[Math.floor(v.length/2)];
const lines=['# Forma renderer benchmark','',`CPU: ${metadata.cpu}; ${metadata.logicalCpus} logical cores; ${(totalmem()/2**30).toFixed(0)} GiB unified/system RAM. ${repeats} independent processes per case, ${frames} frames after 20 warmup frames.`,``,
'Throughput is offscreen, serialized submit + completion, **not on-screen FPS**. CPU % uses one full logical core = 100%. Allocations count Rust heap calls only. Medians across processes.','',
'| Scene / backend / mode / target | render frames/s | completed p95 ms | GPU pass mean ms | CPU % | alloc + realloc / frame | requested KiB / frame | RSS MiB | GPU buffers MiB | target MiB |',
'|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|'];
for(const [scene,backend,mode,w,h] of cases){
 const rows=results.filter(r=>r.scene===scene&&r.backend===backend&&r.mode===mode&&r.width===w&&r.height===h);
 const m=fn=>median(rows.map(fn));const n=fn=>m(fn).toFixed(2);
 lines.push(`| ${scene} / ${backend} / ${mode} / ${w}×${h} | ${n(r=>r.render_throughput_fps)} | ${mode==='idle'?'n/a':n(r=>r.completed_ms.p95)} | ${rows.every(r=>r.gpu_pass)?n(r=>r.gpu_pass.mean_ms):'n/a'} | ${n(r=>r.cpu_percent_one_core)} | ${mode==='idle'?'n/a':n(r=>(r.rust_allocations+r.rust_reallocations)/r.frames)} | ${mode==='idle'?'n/a':n(r=>r.rust_requested_bytes/r.frames/1024)} | ${n(r=>r.rss_after_bytes/2**20)} | ${n(r=>r.owned_gpu_buffer_bytes/2**20)} | ${n(r=>r.target_payload_bytes/2**20)} |`);
}
lines.push('','See metadata.json for provenance and results.json for raw samples, peaks and deltas. GPU resources exclude driver heaps, staging, swapchain and alignment/compression overhead. GPU timestamps cover the render pass, not presentation or texture allocation.');
await writeFile(resolve(out,'REPORT.md'),lines.join('\n')+'\n');
console.log(out);
