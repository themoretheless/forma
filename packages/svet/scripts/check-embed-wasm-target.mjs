import {test} from 'node:test';
import assert from 'node:assert/strict';
import {mkdtemp,mkdir,copyFile,writeFile,readFile,rm} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import {resolve,relative,dirname} from 'node:path';
import {spawnSync} from 'node:child_process';
const empty=Buffer.from([0,97,115,109,1,0,0,0]),selected=Buffer.from([0,97,115,109,1,0,0,0,0,2,1,120]);
for(const mode of ['default','absolute','relative'])test(`embed uses ${mode} Cargo target`,async()=>{
 const temporary=await mkdtemp(resolve(tmpdir(),'svet-embed-target-'));
 try{
  const scripts=resolve(temporary,'scripts'),crate=resolve(temporary,'crates/svet-wasm'),other=resolve(temporary,'isolated-target'),suffix='wasm32-unknown-unknown/release/svet_wasm.wasm';
  await mkdir(scripts,{recursive:true});await copyFile(new URL('./embed-wasm.mjs',import.meta.url),resolve(scripts,'embed-wasm.mjs'));
  for(const [path,bytes]of [[resolve(crate,'target',suffix),empty],[resolve(other,suffix),selected]]){await mkdir(dirname(path),{recursive:true});await writeFile(path,bytes);}
  const env={...process.env};delete env.CARGO_TARGET_DIR;if(mode!=='default')env.CARGO_TARGET_DIR=mode==='absolute'?other:relative(crate,other);
  const result=spawnSync(process.execPath,[resolve(scripts,'embed-wasm.mjs')],{cwd:crate,env,encoding:'utf8'});assert.equal(result.status,0,result.stderr);
  const source=await readFile(resolve(temporary,'src/engine/wasm-binary.ts'),'utf8'),base64=/WASM_BASE64: string = "([^"]+)"/.exec(source)?.[1];assert(base64);assert.deepEqual(Buffer.from(base64,'base64'),mode==='default'?empty:selected);
 }finally{await rm(temporary,{recursive:true,force:true});}
});
