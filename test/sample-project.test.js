import {test} from 'node:test';
import assert from 'node:assert/strict';
import {sampleProject} from '../src/sample-project.js';
import {validateProject} from '../src/browser-storage.js';
import {parse} from '../src/language.js';

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
