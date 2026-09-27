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
// The same walk, but collecting the bare specifiers rollup resolves into node_modules: a package
// nothing configures still ships, and its unit tests all stay green because nothing imports it.
export function packageName(spec){const parts=spec.split('/');return spec.startsWith('@')?parts.slice(0,2).join('/'):parts[0];}
export function packageSpecs(source){
 return [...new Set([...source.matchAll(SPECS)].map(m=>m[1]??m[2]).filter(spec=>spec&&!spec.startsWith('.')).map(packageName))];
}
export function packages(start=entries()){
 const seen=new Set(),out=new Set(),stack=[...start];
 while(stack.length){const file=stack.pop();if(seen.has(file))continue;seen.add(file);
  for(const spec of packageSpecs(read(file)))out.add(spec);
  for(const next of edges(file))if(!seen.has(next))stack.push(next);}
 return out;
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
test('the shipped graph asks for no CodeMirror package nothing configures',()=>{
 const specs=packageSpecs(`import {basicSetup} from 'codemirror';
import {linter} from '@codemirror/lint';
import {lineNumbers} from '@codemirror/view/dist/index.js';
import {EditorState} from "./state";
const glue=await import('./vector-pkg/forma.js');`);
 assert.deepEqual(specs.sort(),['@codemirror/lint','@codemirror/view','codemirror'],'bare names only, scoped names whole, subpaths folded');
 const shipped=packages();
 // basicSetup hard-imports the lint keymap, so one 'codemirror' line ships a package whose two
 // bindings can only ever open an empty panel. The sizes are in PRODUCTION_READINESS.md.
 assert.equal(shipped.has('codemirror'),false,'the meta package drags the whole basicSetup back in');
 assert.equal(shipped.has('@codemirror/lint'),false,'no linter is installed, so there is nothing for lint to panel or gutter');
 assert.ok(shipped.has('@codemirror/search'),'search stays: Ctrl-F and Mod-D are features the editor uses');
 assert.ok(shipped.has('@codemirror/view')&&shipped.size>8,'the walk has to read imports, not a list of names');
});
