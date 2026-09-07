// Package the exact WASM build tested by CI; never download a "latest" runtime.
import {readFileSync,mkdirSync,writeFileSync} from 'node:fs';
import {spawnSync} from 'node:child_process';
import {createHash} from 'node:crypto';
import {resolve} from 'node:path';
import {root,versions} from './versions.mjs';
const current=versions();
const info=JSON.parse(readFileSync(resolve(root,'public/vector-pkg/build-info.json'),'utf8'));
const git=spawnSync('git',['rev-parse','HEAD'],{cwd:root,encoding:'utf8'});
if(git.status!==0||info.commit!==git.stdout.trim()||info.runtime!==current.runtime||info.studio!==current.studio||info.wasmBindgen!==current.wasmBindgen)throw Error('WASM build-info does not match checkout; rebuild and test');
if(process.env.CI&&info.dirty)throw Error('CI release build must use a clean checkout');
const distInfo=JSON.parse(readFileSync(resolve(root,'dist/vector-pkg/build-info.json'),'utf8'));
if(JSON.stringify(distInfo)!==JSON.stringify(info))throw Error('Studio does not contain the tested WASM build; run npm run build');
for(const file of ['forma.js','forma_bg.wasm'])if(!readFileSync(resolve(root,`public/vector-pkg/${file}`)).equals(readFileSync(resolve(root,`dist/vector-pkg/${file}`))))throw Error(`Stale Studio runtime: ${file}`);
mkdirSync(resolve(root,'.release'),{recursive:true});
const bundles=[['runtime','public/vector-pkg'],['studio','dist']];
for(const [component,directory] of bundles){
  const name=component==='runtime'?`forma-${current.runtime}-wasm.tar.gz`:`forma-studio-${current.studio}-web.tar.gz`;
  const result=spawnSync('tar',['-czf',resolve(root,'.release',name),'-C',resolve(root,directory),'.'],{stdio:'inherit'});
  if(result.status!==0)throw Error(`Packaging ${name} failed`);
  const hash=createHash('sha256').update(readFileSync(resolve(root,'.release',name))).digest('hex');
  writeFileSync(resolve(root,'.release',`${name}.sha256`),`${hash}  ${name}\n`);
}
console.log(JSON.stringify(info));
