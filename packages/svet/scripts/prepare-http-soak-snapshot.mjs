import assert from 'node:assert/strict';
import {readFile,writeFile,mkdir,mkdtemp,readdir} from 'node:fs/promises';
import {resolve,dirname} from 'node:path';
import {fileURLToPath} from 'node:url';
import {createHash} from 'node:crypto';
assert(process.argv[2]&&process.argv[3],'Usage: prepare-http-soak-snapshot.mjs INSTALLED_PACKAGE MANIFEST');
const root=resolve(dirname(fileURLToPath(import.meta.url)),'..'),installation=resolve(process.argv[2]);
const hash=bytes=>createHash('sha256').update(bytes).digest('hex'),meta=JSON.parse(await readFile(resolve(installation,'package.json'),'utf8'));
assert.equal(meta.name,'svet-engine');assert.equal(meta.exports['.'].import,'./dist/package/index.js');
const cache=resolve(root,'node_modules/.cache/svet-http-soak');await mkdir(cache,{recursive:true});const snapshot=await mkdtemp(resolve(cache,'snapshot-'));await mkdir(resolve(snapshot,'scripts'));
const sourceSha256={};
for(const file of ['soak-ssr-stream-http.mjs','browser-fixture-endpoints.mjs']){
 const bytes=await readFile(resolve(root,'scripts',file));sourceSha256[file]=hash(bytes);await writeFile(resolve(snapshot,'scripts',file),bytes);
 assert.equal(hash(await readFile(resolve(root,'scripts',file))),sourceSha256[file],'Source changed during snapshot');
}
const runtimeSha256={};for(const file of (await readdir(resolve(installation,'dist/package'))).filter(name=>name.endsWith('.js')||name.endsWith('.wasm')).sort())runtimeSha256[file]=hash(await readFile(resolve(installation,'dist/package',file)));
const report={createdAt:new Date().toISOString(),node:process.version,snapshot,harness:resolve(snapshot,'scripts/soak-ssr-stream-http.mjs'),installedPackage:installation,sourceSha256,runtimeSha256,jsdom:JSON.parse(await readFile(resolve(root,'node_modules/jsdom/package.json'),'utf8')).version,scope:'Frozen harness/fixture bytes; public installed package path. Workspace source/build changes do not alter selected artifacts. Host jsdom is a named harness dependency, not production runtime.'};
await writeFile(resolve(process.argv[3]),JSON.stringify(report,null,2)+'\n');await writeFile(resolve(snapshot,'manifest.json'),JSON.stringify(report,null,2)+'\n');console.log(report.harness);
