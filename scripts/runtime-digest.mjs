import {createHash} from 'node:crypto';
import {lstatSync,readdirSync,readFileSync} from 'node:fs';
import {resolve} from 'node:path';
export function digestInputs(root,inputs){
 const hash=createHash('sha256');
 function add(path){
  const absolute=resolve(root,path),stat=lstatSync(absolute);
  if(stat.isSymbolicLink())throw Error(`Build inputs must not be symlinks: ${path}`);
  if(stat.isDirectory()){for(const entry of readdirSync(absolute).sort())add(`${path}/${entry}`);return;}
  const bytes=readFileSync(absolute);hash.update(JSON.stringify([path,bytes.length]));hash.update(bytes);
 }
 for(const path of [...inputs].sort())add(path);
 return hash.digest('hex');
}
export function runtimeSourceDigest(root){return digestInputs(root,[
 'rust-toolchain.toml','vector-ui/Cargo.toml','vector-ui/Cargo.lock',
 'vector-ui/src','vector-ui/assets','vector-ui/controls',
 'vector-ui/examples/Button.ui','vector-ui/examples/Button.component.ui',
 'scripts/build-wasm.mjs','scripts/runtime-digest.mjs',
]);}
export const runtimeArtifacts=['forma.js','forma_bg.wasm','forma.d.ts','forma_bg.wasm.d.ts'];
export function artifactDigests(directory){return Object.fromEntries(runtimeArtifacts.map(file=>[file,createHash('sha256').update(readFileSync(resolve(directory,file))).digest('hex')]));}
