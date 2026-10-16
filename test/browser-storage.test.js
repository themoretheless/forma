import {test} from 'node:test';
import assert from 'node:assert/strict';
import {readStorage,writeStorage,restoreProject,validateProject} from '../src/browser-storage.js';
test('storage access and quota failures do not crash startup or claim a successful write',()=>{
 const denied={get localStorage(){throw Error('SecurityError');}};
 assert.equal(readStorage('localStorage','x',denied),null);assert.equal(writeStorage('localStorage','x','value',denied),false);
 const full={localStorage:{getItem:()=>null,setItem(){throw Error('QuotaExceededError');}}};
 assert.equal(writeStorage('localStorage','x','value',full),false);assert.equal(writeStorage('sessionStorage','x','value',{}),false);
 const data=new Map(),ok={localStorage:{getItem:k=>data.get(k),setItem:(k,v)=>data.set(k,v)}};
 assert.equal(writeStorage('localStorage','x','value',ok),true);assert.equal(readStorage('localStorage','x',ok),'value');
});
test('project restoration validates structure and unsafe paths without touching stored data',()=>{
 const fallback={'ui/Main.ui':'component Main {}'};
 for(const raw of [null,'invalid','null','[]','{}','42','"project"','{"ui/A.ui":4}','{"../outside":"x"}','{"__proto__":"x"}'])assert.equal(restoreProject(raw,fallback),fallback);
 const raw='{"ui/Window.ui":"component Window {}"}';assert.deepEqual(restoreProject(raw,fallback),JSON.parse(raw));
 for(const key of ['/a','a//b','a/../b','a\\b','constructor/x'])assert.throws(()=>validateProject({[key]:'x'}),/путь/);
});

test('project save preserves invalid data and refuses to overwrite a different recovery',async()=>{
 const {writeProject}=await import('../src/browser-storage.js');
 const values=new Map([['project','{broken']]);
 const host={localStorage:{getItem:k=>values.get(k)??null,setItem:(k,v)=>values.set(k,v)}};
 assert.equal(writeProject('project',{'main.ui':'ok'},host),true);
 assert.equal(values.get('project:recovery'),'{broken');
 values.set('project','second broken');
 assert.equal(writeProject('project',{'main.ui':'new'},host),false);
 assert.equal(values.get('project'),'second broken');
 assert.equal(values.get('project:recovery'),'{broken');
 values.delete('project:recovery');
 host.localStorage.setItem=()=>{throw Error('quota');};
 assert.equal(writeProject('project',{'main.ui':'new'},host),false);
 assert.equal(values.get('project'),'second broken');
});

test('unchanged valid project does not rewrite storage',async()=>{
 const {writeProject}=await import('../src/browser-storage.js');
 const files={'main.ui':'same'};let reads=0;
 const host={localStorage:{getItem(){reads++;return JSON.stringify(files);},setItem(){throw Error('must not write');}}};
 assert.equal(writeProject('project',files,host),true);assert.equal(reads,1);
});

test('local directory project reader and writer sync nested .ui/.rs files and skip ignored directories',async()=>{
 const {readDirectoryProject,writeDirectoryProject}=await import('../src/browser-storage.js');
 function mockDir(tree){
  return {
   async *entries(){for(const [k,v]of Object.entries(tree))yield [k,v];},
   async getDirectoryHandle(name){return tree[name]??(tree[name]={kind:'directory',...mockDir({})});},
   async getFileHandle(name){return tree[name]??(tree[name]={kind:'file',content:'',async getFile(){return {size:this.content.length,text:async()=>this.content};},async createWritable(){return {write:async c=>{tree[name].content=c;},close:async()=>{}};}});},
  };
 }
 const root=mockDir({
  '.git':{kind:'directory',...mockDir({'config':{kind:'file',async getFile(){return {size:4,text:async()=>'nope'};}}})},
  'target':{kind:'directory',...mockDir({'debug.rs':{kind:'file',async getFile(){return {size:4,text:async()=>'nope'};}}})},
  'ui':{kind:'directory',...mockDir({
   'Main.ui':{kind:'file',content:'component Main { Frame {} }',async getFile(){return {size:27,text:async()=>this.content};},async createWritable(){return {write:async c=>{this.content=c;},close:async()=>{}};}},
  })},
  'Cargo.toml':{kind:'file',content:'[package]\nname = "demo"\n',async getFile(){return {size:24,text:async()=>this.content};},async createWritable(){return {write:async c=>{this.content=c;},close:async()=>{}};}},
 });
 const read=await readDirectoryProject(root);
 assert.deepEqual(read,{'ui/Main.ui':'component Main { Frame {} }','Cargo.toml':'[package]\nname = "demo"\n'});
 const updated={'ui/Main.ui':'component Main { Frame { Text { text: "Hi"; } } }','Cargo.toml':'[package]\nname = "demo"\n','src/main.rs':'fn main() {}\n'};
 await writeDirectoryProject(root,updated,read);
 const reread=await readDirectoryProject(root);
 assert.deepEqual(reread,updated);
});
