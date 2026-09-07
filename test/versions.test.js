import {test} from 'node:test';
import assert from 'node:assert/strict';
import {versions,validVersion,release} from '../scripts/versions.mjs';

test('release tags select independent manifest versions',()=>{
 const current={runtime:'0.2.0',studio:'0.5.1'};
 assert.equal(release('forma-v0.2.0',current).component,'runtime');
 assert.equal(release('studio-v0.5.1',current).component,'studio');
 assert.throws(()=>release('studio-v0.2.0',current),/does not match/);
 assert.throws(()=>release('forma-v0.3.0',current),/does not match/);
 for(const tag of ['v0.2.0','runtime-v0.2.0','forma-0.2.0','forma-v0.2.0/extra','forma-v0.2.0+build'])assert.throws(()=>release(tag,current));
});
test('prereleases are explicit and malformed semver cannot trigger a release',()=>{
 assert.equal(release('forma-v0.2.0-rc.1',{runtime:'0.2.0-rc.1'}).prerelease,true);
 assert.equal(release('forma-v0.2.0',{runtime:'0.2.0'}).prerelease,false);
 for(const v of ['01.2.3','1.02.3','1.2','1.2.3-','1.2.3-rc.01','1.2.3-01','1.2.3\n','1.2.3+build'])assert.equal(validVersion(v),false,v);
 assert.equal(validVersion('0.1.0-alpha.0'),true);
});
test('committed manifest and lockfile versions agree',()=>{
 const v=versions();assert.ok(validVersion(v.runtime));assert.ok(validVersion(v.studio));assert.match(v.wasmBindgen,/^\d+\.\d+\.\d+$/);
});

test('version command updates only its component and all associated lockfiles',async()=>{
 const {mkdtempSync,mkdirSync,copyFileSync,readFileSync,rmSync}=await import('node:fs');
 const {tmpdir}=await import('node:os');
 const {join,dirname}=await import('node:path');
 const {spawnSync}=await import('node:child_process');
 const {root}=await import('../scripts/versions.mjs');
 const folder=mkdtempSync(join(tmpdir(),'forma-versions-'));
 try{
  for(const path of ['scripts/versions.mjs','package.json','package-lock.json','vector-ui/Cargo.toml','vector-ui/Cargo.lock','vector-ui/examples/binding-app/Cargo.lock','native-app/Cargo.toml','native-app/Cargo.lock']){
   const dest=join(folder,path);mkdirSync(dirname(dest),{recursive:true});copyFileSync(join(root,path),dest);
  }
  // The CLI uses ESM; its copied root package.json already sets type=module.
  const run=(...args)=>spawnSync(process.execPath,[join(folder,'scripts/versions.mjs'),...args],{encoding:'utf8'});
  const before=JSON.parse(run('check').stdout);
  const runtime=run('set','runtime','0.8.0-rc.1');assert.equal(runtime.status,0,runtime.stderr);
  assert.deepEqual(JSON.parse(runtime.stdout),{...before,runtime:'0.8.0-rc.1'});
  const studio=run('set','studio','0.9.1');assert.equal(studio.status,0,studio.stderr);
  assert.deepEqual(JSON.parse(studio.stdout),{...before,runtime:'0.8.0-rc.1',studio:'0.9.1'});
  const snapshot=readFileSync(join(folder,'package.json'),'utf8');
  assert.notEqual(run('set','studio','0.9.01').status,0);
  assert.equal(readFileSync(join(folder,'package.json'),'utf8'),snapshot,'invalid input must not write files');
 }finally{rmSync(folder,{recursive:true,force:true});}
});
