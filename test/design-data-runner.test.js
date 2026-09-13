import {test} from 'node:test';
import assert from 'node:assert/strict';
import {mkdtemp,readdir,rm} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import path from 'node:path';
import {evaluateDesignData} from '../design-data-runner.js';

test('design evaluation removes temporary files after success and compiler/runtime failures',async()=>{
 const parent=await mkdtemp(path.join(tmpdir(),'forma-design-test-'));
 const keys=['TMPDIR','TMP','TEMP'];const old=keys.map(k=>process.env[k]);
 for(const key of keys)process.env[key]=parent;
 try{
  assert.deepEqual(await evaluateDesignData({'src/design.rs':'pub const VALUE: i32 = 42;'},['design.VALUE']),{'design.VALUE':'42'});
  assert.deepEqual((await readdir(parent)).filter(name=>name.startsWith('forma-design-')),[]);
  await assert.rejects(evaluateDesignData({'src/design.rs':'not valid Rust'},['design.VALUE']));
  assert.deepEqual((await readdir(parent)).filter(name=>name.startsWith('forma-design-')),[]);
  await assert.rejects(evaluateDesignData({'src/design.rs':'pub fn value() -> i32 { panic!("expected failure") }'},['design.value()']));
  assert.deepEqual((await readdir(parent)).filter(name=>name.startsWith('forma-design-')),[]);
 }finally{
  keys.forEach((key,i)=>{if(old[i]===undefined)delete process.env[key];else process.env[key]=old[i];});
  await rm(parent,{recursive:true,force:true});
 }
});

test('design result reads enforce a total byte budget',async()=>{
 const {readDesignValues}=await import('../design-data-runner.js');
 const {writeFile}=await import('node:fs/promises');
 const dir=await mkdtemp(path.join(tmpdir(),'forma-result-test-'));
 try{
  await writeFile(path.join(dir,'value-0'),Buffer.alloc(1024*1024,97));
  assert.equal((await readDesignValues(dir,['design.a']))['design.a'].length,1024*1024);
  await writeFile(path.join(dir,'value-1'),'b');
  await assert.rejects(readDesignValues(dir,['design.a','design.b']),/1 МиБ/);
  await writeFile(path.join(dir,'value-0'),Buffer.alloc(1024*1024+1));
  await assert.rejects(readDesignValues(dir,['design.a']),/1 МиБ/);
 }finally{await rm(dir,{recursive:true,force:true});}
});
