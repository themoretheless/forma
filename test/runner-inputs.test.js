import {test} from 'node:test';
import assert from 'node:assert/strict';
import {EventEmitter} from 'node:events';
import {createNativeRunner} from '../native-runner.js';
import {createRustRunner} from '../rust-runner.js';

test('malformed runner requests report errors and release busy state without spawning',async()=>{
 for(const create of [createNativeRunner,createRustRunner]){
  const messages=[];const runner=create(message=>messages.push(message));
  for(const payload of [null,undefined,[],{},42]){
   await runner.run(payload);
   assert.equal(messages.at(-1).kind,'error');
   assert.doesNotMatch(messages.at(-1).text,/уже запущен|уже запущено/);
  }
  assert.equal(messages.length,5);runner.stop();
 }
});

test('rust runner cleans up its temporary directory on process close and spawn error',async()=>{
 const events=[],removed=[],children=[];
 const runner=createRustRunner(e=>events.push(e),{
  async mkdtemp(prefix){return `${prefix}${children.length+1}`;},
  async mkdir(){},
  async writeFile(){},
  async rm(target,opts){removed.push([target,opts]);},
  spawn(){
   const child=new EventEmitter();
   child.stdout=new EventEmitter();
   child.stderr=new EventEmitter();
   child.kill=()=>{};
   children.push(child);
   return child;
  },
 });
 const files={'Cargo.toml':'[package]\nname="a"\nversion="0.1.0"\n','src/main.rs':'fn main(){}'};
 await runner.run(files);
 assert.equal(removed.length,0);
 children[0].emit('close',0,null);
 assert.equal(removed.length,1);
 assert.match(removed[0][0],/forma-rust-1$/);
 assert.deepEqual(removed[0][1],{recursive:true,force:true});

 await runner.run(files);
 children[1].emit('error',new Error('spawn failed'));
 assert.equal(removed.length,2);
 assert.match(removed[1][0],/forma-rust-2$/);
});

test('rust runner enables native only for a native generated-form launch',async()=>{
 const invocations=[],children=[];
 const runner=createRustRunner(()=>{},{
  async mkdtemp(prefix){return `${prefix}${children.length+1}`;},
  async mkdir(){},async writeFile(){},async rm(){},
  spawn(command,args){
   invocations.push([command,args]);
   const child=new EventEmitter();child.stdout=new EventEmitter();child.stderr=new EventEmitter();child.kill=()=>{};children.push(child);return child;
  },
 });
 const files={
  'Cargo.toml':'[package]\nname="a"\nversion="0.1.0"\n[features]\nnative=["forma/native"]\n[dependencies]\nforma={path="../vector-ui"}\n',
  'src/main.rs':'fn main(){}',
  'ui/Form.ui':"component Form { contextType:'crate::Vm'; Frame {} }",
 };
 await runner.run(files,{generated:true,native:true});
 assert.deepEqual(invocations[0][1].slice(0,5),['run','--offline','--features','native','--manifest-path']);
 children[0].emit('close',0,null);
 await runner.run(files,{generated:true});
 assert.ok(!invocations[1][1].includes('--features'));
 children[1].emit('close',0,null);
});
