import {spawnSync,execSync} from 'node:child_process';
import {readFileSync} from 'node:fs';
import {hostname,release,cpus,loadavg} from 'node:os';
import {resolve} from 'node:path';
import {root,versions} from './versions.mjs';
import {runtimeSourceDigest,artifactDigests} from './runtime-digest.mjs';

// CI sat red on two of three runner platforms for eleven runs while every local command stayed
// green, because no single local step builds the native bin for a foreign target. This is that
// step: the whole matrix, one exit code, and an environment block so a printed result says where
// it came from.
const CI_TARGETS=['x86_64-unknown-linux-gnu','x86_64-pc-windows-msvc'];
export function steps({platform,installedTargets}){
 return {
  list:[
   {id:'studio-js',name:'Studio test suite (node --test)',argv:process.execPath,args:['--test'],requiresSummary:true},
   {id:'runtime-gpu',name:'Forma Rust tests with GPU features',argv:'cargo',args:['test','--locked','--manifest-path','vector-ui/Cargo.toml','--features','gpu','--lib','--tests']},
   {id:'binding-native',name:'Native binding example',argv:'cargo',args:['test','--locked','--manifest-path','vector-ui/examples/binding-app/Cargo.toml','--features','native']},
   {id:'native-host',name:'Native bin compiles on the host',argv:'cargo',args:['check','--locked','--manifest-path','vector-ui/Cargo.toml','--features','native','--bin','forma']},
   ...CI_TARGETS.filter(target=>installedTargets.includes(target)).map(target=>(
    {id:`native-${target}`,name:`Native bin compiles for ${target}`,argv:'cargo',args:['check','--locked','--manifest-path','vector-ui/Cargo.toml','--features','native','--bin','forma','--target',target]})),
   // The WebView host generates its actions module from a fixture, exactly as ci.yml runs it.
   ...(platform==='darwin'?[{id:'studio-legacy',name:'Studio legacy WebView host',argv:'cargo',args:['check','--locked','--manifest-path','native-app/Cargo.toml'],env:{FORMA_ACTIONS:'.github/fixtures/actions.rs'}}]:[]),
  ],
  skipped:CI_TARGETS.filter(target=>!installedTargets.includes(target)).map(target=>({id:`native-${target}`,reason:`not installed; rustup target add ${target}`})),
 };
}
export function summary(text){
 const number=key=>Number(new RegExp(`^\\s*#\\s*${key}\\s+(\\d+)$`,'m').exec(text)?.[1]??NaN);
 const tests=number('tests'),pass=number('pass'),fail=number('fail');
 if(!Number.isInteger(tests)||!Number.isInteger(pass)||!Number.isInteger(fail))return null;
 return {tests,pass,fail,skipped:number('skipped')};
}
export function evaluate(step,{status,output}){
 if(status!==0)return{ok:false,detail:`exit ${status}`};
 // An empty run is not a pass: a shadowed or piped command prints nothing and still exits 0.
 if(step.requiresSummary){
  const counted=summary(output);
  return counted?{ok:counted.tests>0&&counted.fail===0,detail:`${counted.pass}/${counted.tests} pass`}:{ok:false,detail:'no test summary in output'};
 }
 return {ok:true,detail:''};
}
export function provenance(){
 let info=null;
 try{info=JSON.parse(readFileSync(resolve(root,'public/vector-pkg/build-info.json'),'utf8'));}
 catch{return {state:'missing',detail:'public/vector-pkg is not built; run npm run build:wasm'};}
 if(info.sourceDigest!==runtimeSourceDigest(root))return {state:'stale',detail:'runtime sources changed since the WASM build; rebuild and retest'};
 if(JSON.stringify(info.artifacts)!==JSON.stringify(artifactDigests(resolve(root,'public/vector-pkg'))))return {state:'stale',detail:'WASM artifacts changed since the build'};
 return {state:'fresh',detail:`source ${String(info.sourceDigest).slice(0,16)} · commit ${String(info.commit).slice(0,8)}${info.dirty?' dirty':''}`};
}
export function environment(){
 const git=args=>spawnSync('git',args,{cwd:root,encoding:'utf8'}).stdout?.trim()??'';
 let installedTargets=[];
 try{installedTargets=execSync('rustup target list --installed',{encoding:'utf8'}).split('\n').map(line=>line.trim()).filter(Boolean);}catch{}
 return {node:process.version,rustc:spawnSync('rustc',['--version'],{encoding:'utf8'}).stdout?.trim()??'rustc unavailable',
  wasmBindgen:versions().wasmBindgen,hostname:hostname(),system:`${process.platform} ${release()} ${process.arch}`,
  cores:cpus().length,load:loadavg().map(value=>Number(value.toFixed(2))),installedTargets,
  branch:git(['rev-parse','--abbrev-ref','HEAD']),head:git(['rev-parse','--short','HEAD']),dirty:!!git(['status','--porcelain']).length};
}
const invoked=process.argv[1]&&import.meta.url===new URL(`file://${resolve(process.argv[1])}`).href;
if(invoked){
 const args=process.argv.slice(2);
 const option=name=>args.find(arg=>arg.startsWith(`--${name}=`))?.slice(name.length+3);
 const env=environment(),{list,skipped}=steps({platform:option('platform')??process.platform,installedTargets:env.installedTargets});
 if(args.includes('--list')){for(const step of list)console.log(`${step.id}\t${step.name}`);process.exit(0);}
 const only=args.filter(arg=>!arg.startsWith('-'));
 const chosen=only.length?list.filter(step=>only.includes(step.id)):list;
 if(!chosen.length){console.error(`no such step: ${only.join(' ')}\ntry: ${list.map(step=>step.id).join(', ')}`);process.exit(2);}
 console.log(`forma full gate · ${env.branch}@${env.head}${env.dirty?' (dirty)':''} · ${env.system}, ${env.cores} cores, load ${env.load.join(' ')}`);
 console.log(`  node ${env.node} · ${env.rustc} · wasm-bindgen ${env.wasmBindgen}`);
 const results=[];
 for(const step of chosen){
  const started=Date.now();
  const run=spawnSync(step.argv,step.args,{cwd:root,encoding:'utf8',maxBuffer:64<<20,env:step.env&&{...process.env,...Object.fromEntries(Object.entries(step.env).map(([key,value])=>[key,resolve(root,value)]))}});
  const output=`${run.stdout??''}${run.stderr??''}`;
  const verdict=evaluate(step,{status:run.status??1,output});
  results.push(verdict);
  console.log(`  ${verdict.ok?'ok  ':'FAIL'} ${step.id.padEnd(32)}${((Date.now()-started)/1000).toFixed(1)}s ${verdict.detail}`);
  if(!verdict.ok&&process.env.GATE_VERBOSE)console.log(output.trimEnd().replace(/^/gm,'       '));
 }
 for(const target of skipped)console.log(`  skip ${target.id.padEnd(30)}${target.reason}`);
 const source=provenance();
 console.log(`  ${source.state==='fresh'?'ok  ':source.state==='stale'?'FAIL':'--  '} wasm-provenance          ${source.detail}`);
 if(source.state==='missing')console.log('        WASM integration tests skip without public/vector-pkg');
 console.log('  remaining limits and contract-decision levers: PRODUCTION_READINESS.md');
 process.exit(results.every(result=>result.ok)&&source.state!=='stale'?0:1);
}
