import {test} from 'node:test';
import assert from 'node:assert/strict';
import {sampleProject} from '../src/sample-project.js';
import {validateProject} from '../src/browser-storage.js';
import {parse} from '../src/language.js';
import {prepareFormProject} from '../form-project.js';
import {buildNativeRustExport} from '../src/native-export.js';

// The project every fresh Studio opens must load cleanly: a broken sample is the first thing a user sees.
test('the sample project is a valid project whose design references resolve',()=>{
 validateProject(sampleProject);
 for(const [path,source] of Object.entries(sampleProject))
  for(const [,ref] of source.matchAll(/#\[design\('\.\/([^']+)'\)\]/g))
   assert.ok(`${path.slice(0,path.lastIndexOf('/')+1)}${ref}` in sampleProject,`${path} → ${ref}`);
});
test('every sample markup file parses',()=>{
 const files=Object.keys(sampleProject).filter(p=>p.endsWith('.ui'));
 assert.ok(files.length>0);
 for(const path of files)assert.doesNotThrow(()=>parse(sampleProject[path]),path);
});

test('the sample is a live typed Rust application in Studio and native export',()=>{
 const document=parse(sampleProject['ui/SearchWindow.ui']);
 assert.equal(document.defaults.contextType,'crate::SearchVm');
 assert.match(sampleProject['ui/SearchWindow.ui'],/clicked -> state\.search\(\)/);
 assert.match(sampleProject['ui/SearchWindow.ui'],/clicked -> state\.reset\(\)/);
 assert.ok('ui/TwoButtons.ui' in sampleProject);
 assert.match(sampleProject['ui/TwoButtons.ui'],/clicked -> actions\.first\(\)/);
 assert.match(sampleProject['ui/TwoButtons.ui'],/clicked -> actions\.second\(\)/);
 assert.match(sampleProject['src/app.rs'],/pub struct SearchVm/);
 assert.match(sampleProject['src/app.rs'],/pub fn search\(&self\)/);
 assert.match(sampleProject['src/app.rs'],/pub fn reset\(&self\)/);
 assert.ok(!Object.hasOwn(sampleProject,'src/actions.rs'));
 assert.ok(!Object.hasOwn(sampleProject,'src/forma_generated.rs'));

 const staged=prepareFormProject(sampleProject,{}, {nativeApp:true});
 assert.equal(staged.count,1);
 assert.match(staged.files['src/forma_generated.rs'],/NativeApp for SearchWindow/);
 assert.match(staged.files['src/forma_generated.rs'],/vm\.search\(\)/);
 assert.match(staged.files['src/forma_generated.rs'],/state\.query/);

 const exported=buildNativeRustExport(sampleProject,'ui/SearchWindow.ui');
 assert.equal(exported.files['src/app.rs'],sampleProject['src/app.rs']);
 assert.match(exported.files['src/forma_generated.rs'],/NativeApp for SearchWindow/);
 assert.match(exported.files['Cargo.toml'],/default = \[\]/);
});
