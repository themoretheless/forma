import {test} from 'node:test';
import assert from 'node:assert/strict';
import {execFileSync} from 'node:child_process';
import {readFileSync} from 'node:fs';
import {dirname,relative,resolve,sep} from 'node:path';
import {root} from '../scripts/versions.mjs';

// A crate nothing compiles rots in silence, and so does a module nothing boots: deleting the last
// import of a designer file leaves its own unit tests green while the browser never loads it, and
// vite reports a clean build because the module simply is not in the bundle. So the shipped graph
// is walked from the things that actually execute, and every tracked src module must turn up.
const tracked=execFileSync('git',['ls-files'],{cwd:root,encoding:'utf8'}).split('\n').filter(Boolean);
const read=f=>readFileSync(resolve(root,f),'utf8');
const SPECS=/(?:^|[^\w.$])(?:import|export)[\s\S]{0,120}?\bfrom\s*['"]([^'"]+)['"]|(?:^|[^\w.$])import\s*\(\s*['"]([^'"]+)['"]/g;
// Resolved the way this repo imports: relative paths only, exact file name, no extension rewrite.
function target(from,spec){
 if(!spec?.startsWith('.'))return null;
 const path=relative(root,resolve(root,dirname(from),spec)).split(sep).join('/');
 return tracked.includes(path)?path:null;
}
function edges(file){
 return [...read(file).matchAll(SPECS)].map(m=>target(file,m[1]??m[2])).filter(Boolean);
}
// Roots are the entry points the repo runs, not a guess about the studio: the html pages, the CLI
// scripts (generate-form.mjs builds the Rust forms), and the vite config with its dev-server runners.
export function entries(){
 const html=tracked.filter(f=>f.endsWith('.html')).flatMap(file=>[...read(file).matchAll(/<script[^>]*\ssrc="([^"?#]+)/g)]
  .map(m=>tracked.includes(m[1].replace(/^\//,''))?m[1].replace(/^\//,''):target(file,m[1].replace(/^\//,'./'))));
 const commands=tracked.filter(f=>/^scripts\/.*\.mjs$/.test(f)||/^\w[\w-]*-runner\.js$/.test(f)||f==='vite.config.js');
 return [...new Set([...html,...commands].filter(Boolean))];
}
export function shipped(start=entries()){
 const seen=new Set(start),stack=[...start];
 while(stack.length)for(const next of edges(stack.pop()))if(!seen.has(next)){seen.add(next);stack.push(next);}
 return seen;
}
export function sources(){
 return tracked.filter(f=>f.startsWith('src/')&&f.endsWith('.js'));
}
export function orphans(start=entries()){
 const reach=shipped(start);
 return sources().filter(file=>!reach.has(file));
}
test('every tracked src module is reachable from something that runs',()=>{
 assert.ok(sources().length>40,'the walk has to see the whole studio, not an empty tree');
 assert.ok(entries().includes('src/main.js'),'index.html boots the studio through src/main.js');
 assert.deepEqual(orphans(),[],`${orphans().join(', ')} is imported by nothing that executes`);
});
test('the check reads the graph rather than the file list',()=>{
 // Cut the studio's own entry and the app below it falls out of the closure. A list of names
 // could never report this, and that is the difference between a graph walk and an inventory.
 const without=orphans(entries().filter(entry=>entry!=='src/main.js'));
 assert.ok(without.includes('src/main.js'),'cutting the entry must orphan the app');
 assert.ok(without.length>30,'the modules hanging off that entry are the studio itself');
 // The closure must be the app's, not the suite's: unit tests import half of src directly, so a
 // graph that let them in would call every module reachable forever.
 assert.deepEqual([...shipped()].filter(f=>f.startsWith('test/')),[]);
 assert.ok(shipped().size>40,'the shipped graph is the studio, not a handful of files');
});
