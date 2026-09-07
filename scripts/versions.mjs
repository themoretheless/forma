import {readFileSync,writeFileSync} from 'node:fs';
import {fileURLToPath} from 'node:url';
import {resolve} from 'node:path';

export const root=fileURLToPath(new URL('../',import.meta.url));
const read=path=>readFileSync(resolve(root,path),'utf8');
export function validVersion(value){
  if(typeof value!=='string')return false;
  const match=/^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?$/.exec(value);
  return !!match&&(!match[4]||match[4].split('.').every(id=>!/^\d+$/.test(id)||id==='0'||!id.startsWith('0')));
}
function cargoVersion(text,name){
  const section=text.split(/(?=^\[\[?[^\n]+\]\]?\s*$)/m).find(s=>new RegExp(`^name = "${name}"$`,'m').test(s));
  const version=section?.match(/^version = "([^"]+)"$/m)?.[1];
  if(!version)throw Error(`Missing version for ${name}`);
  return version;
}
export function versions(){
  const studio=JSON.parse(read('package.json')).version;
  const lock=JSON.parse(read('package-lock.json'));
  const runtime=cargoVersion(read('vector-ui/Cargo.toml'),'forma');
  if(!validVersion(runtime)||!validVersion(studio))throw Error('Invalid component version');
  if(cargoVersion(read('vector-ui/Cargo.lock'),'forma')!==runtime)throw Error('Runtime Cargo.lock version mismatch');
  if(lock.version!==studio||lock.packages[''].version!==studio)throw Error('Studio package-lock.json version mismatch');
  for(const path of ['native-app/Cargo.toml','native-app/Cargo.lock'])if(cargoVersion(read(path),'forma-native')!==studio)throw Error(`Studio host version mismatch: ${path}`);
  const wasmBindgen=read('vector-ui/Cargo.toml').match(/^wasm-bindgen = "=([^"]+)"$/m)?.[1];
  if(!wasmBindgen||cargoVersion(read('vector-ui/Cargo.lock'),'wasm-bindgen')!==wasmBindgen)throw Error('wasm-bindgen must be exactly pinned and match Cargo.lock');
  return {runtime,studio,wasmBindgen};
}
export function release(tag,current=versions()){
  const match=/^(forma|studio)-v(.+)$/.exec(tag??'');
  if(!match||!validVersion(match[2]))throw Error('Expected forma-vX.Y.Z or studio-vX.Y.Z (optional prerelease)');
  const [,name,version]=match,component=name==='forma'?'runtime':'studio';
  if(current[component]!==version)throw Error(`Tag ${tag} does not match ${component} manifest version ${current[component]}`);
  return {component,version,tag,prerelease:version.includes('-')};
}
function replaceCargo(text,name,version){
  let found=false;
  const next=text.split(/(?=^\[\[?[^\n]+\]\]?\s*$)/m).map(section=>{
    if(!new RegExp(`^name = "${name}"$`,'m').test(section))return section;
    found=true;return section.replace(/^version = "[^"]+"$/m,`version = "${version}"`);
  }).join('');
  if(!found)throw Error(`Missing package ${name}`);return next;
}
export function setVersion(component,version){
  versions(); // Validate all inputs before preparing any writes.
  if(!['runtime','studio'].includes(component)||!validVersion(version))throw Error('set runtime|studio X.Y.Z[-prerelease]');
  const updates=new Map();
  const directory=component==='runtime'?'vector-ui':'native-app',name=component==='runtime'?'forma':'forma-native';
  for(const file of ['Cargo.toml','Cargo.lock']){const path=`${directory}/${file}`;updates.set(path,replaceCargo(read(path),name,version));}
  if(component==='studio'){
    for(const path of ['package.json','package-lock.json']){
      const value=JSON.parse(read(path));value.version=version;if(value.packages)value.packages[''].version=version;
      updates.set(path,JSON.stringify(value,null,2)+'\n');
    }
  }
  for(const [path,text] of updates)writeFileSync(resolve(root,path),text);
  return versions();
}
if(process.argv[1]&&resolve(process.argv[1])===fileURLToPath(import.meta.url)){
  const [command='check',a,b]=process.argv.slice(2);
  try{
    if(command==='check')console.log(JSON.stringify(a?release(a):versions()));
    else if(command==='wasm-bindgen')console.log(versions().wasmBindgen);
    else if(command==='set')console.log(JSON.stringify(setVersion(a,b)));
    else throw Error('Usage: versions.mjs check [tag] | wasm-bindgen | set runtime|studio version');
  }catch(error){console.error(error.message);process.exitCode=1;}
}
