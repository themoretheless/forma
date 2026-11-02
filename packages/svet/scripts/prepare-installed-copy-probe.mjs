import assert from 'node:assert/strict';
import {readFile,writeFile,mkdtemp,readdir,mkdir} from 'node:fs/promises';
import {resolve,dirname,relative} from 'node:path';
import {createHash} from 'node:crypto';
assert(process.argv[2]&&process.argv[3],'Usage: prepare-installed-copy-probe.mjs INSTALLED_PACKAGE MANIFEST');
const installation=resolve(process.argv[2]),metadata=JSON.parse(await readFile(resolve(installation,'package.json'),'utf8'));
const entry=resolve(installation,metadata.exports['.'].import);assert.equal(relative(installation,entry),'dist/package/index.js');
const source=dirname(entry),output=await mkdtemp(resolve(installation,'..','..','svet-installed-copies-')),hash=data=>createHash('sha256').update(data).digest('hex');
const files=await readdir(source,{withFileTypes:true});assert(files.every(file=>file.isFile()),'Unexpected nested runtime tree');
const sourceHashes={},artifactSha256={};await writeFile(resolve(output,'package.json'),JSON.stringify({type:'module'}));
for(const variant of ['first','second']){
 await mkdir(resolve(output,variant));
 for(const file of files){const bytes=await readFile(resolve(source,file.name));sourceHashes[file.name]=hash(bytes);await writeFile(resolve(output,variant,file.name),bytes);artifactSha256[variant+'/'+file.name]=hash(bytes);}
}
for(const [name,sha] of Object.entries(sourceHashes))assert.equal(hash(await readFile(resolve(source,name))),sha,'Installed source changed during copy');
const build={createdAt:new Date().toISOString(),installedPackage:installation,packageName:metadata.name,packageVersion:metadata.version,sourceHashes,artifactSha256,scope:'Two byte-equal independent filesystem copies of installed public runtime. No symlink/module-cache aliasing; browser and foreign dependency qualification separate.'};
await writeFile(resolve(output,'metadata.json'),JSON.stringify(build,null,2)+'\n');await writeFile(resolve(process.argv[3]),JSON.stringify({snapshot:'installed-'+output.split('/').at(-1),output},null,2)+'\n');console.log(output);
