import {spawnSync} from 'node:child_process';
import {cpSync,rmSync,writeFileSync} from 'node:fs';
import {resolve} from 'node:path';
import {root,versions} from './versions.mjs';
const current=versions();
function run(command,args,quiet=false){
  const result=spawnSync(command,args,{cwd:root,encoding:'utf8',stdio:quiet?'pipe':'inherit'});
  if(result.error||result.status!==0)throw Error(`${command} failed: ${result.error?.message??result.stderr??result.status}`);
  return result.stdout?.trim();
}
const cli=run('wasm-bindgen',['--version'],true);
if(cli!==`wasm-bindgen ${current.wasmBindgen}`)throw Error(`Install matching CLI: cargo install wasm-bindgen-cli --version ${current.wasmBindgen} --locked`);
run('cargo',['build','--locked','--release','--manifest-path','vector-ui/Cargo.toml','--target','wasm32-unknown-unknown','--lib']);
run('wasm-bindgen',['vector-ui/target/wasm32-unknown-unknown/release/forma.wasm','--target','web','--out-dir','public/vector-pkg']);
// Remove the old generated module so release bundles expose only the forma name.
for(const file of ['forma_vector.js','forma_vector.d.ts','forma_vector_bg.wasm','forma_vector_bg.wasm.d.ts'])rmSync(resolve(root,'public/vector-pkg',file),{force:true});
const controls=resolve(root,'public/vector-pkg/controls');
rmSync(controls,{recursive:true,force:true});
cpSync(resolve(root,'vector-ui/controls'),controls,{recursive:true});
const commit=run('git',['rev-parse','HEAD'],true);
const dirty=!!run('git',['status','--porcelain','--untracked-files=normal'],true);
writeFileSync(resolve(root,'public/vector-pkg/build-info.json'),JSON.stringify({...current,commit,dirty},null,2)+'\n');
