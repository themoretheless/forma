import {createHash} from 'node:crypto';
import {mkdir,readFile,readdir,writeFile} from 'node:fs/promises';
import {dirname,resolve,relative} from 'node:path';
import {fileURLToPath} from 'node:url';
import {parseArgs} from 'node:util';
import {build,preview} from 'vite';
const {values}=parseArgs({options:{serve:{type:'boolean',default:false},port:{type:'string',default:'4200'}}});
const root=resolve(dirname(fileURLToPath(import.meta.url)),'..'),port=Number(values.port);
if(!Number.isInteger(port)||port<1024||port>65535)throw Error('Invalid preview port');
const cache=resolve(root,'node_modules/.cache/svet-dom-gpu-soak'),snapshot=`snapshot-${Date.now()}`,output=resolve(cache,snapshot,'dist');
const hash=data=>createHash('sha256').update(data).digest('hex');
const files=(await readdir(resolve(root,'src/engine'))).filter(file=>file.endsWith('.ts')).sort().map(file=>`src/engine/${file}`);
files.push('tests/browser/dom-gpu-soak.html','scripts/prepare-dom-gpu-soak.mjs','vite.config.ts','package.json');
const hashes={};for(const file of files)hashes[file]=hash(await readFile(resolve(root,file)));
await build({root,configFile:resolve(root,'vite.config.ts'),build:{outDir:output,emptyOutDir:true,rollupOptions:{input:resolve(root,'tests/browser/dom-gpu-soak.html')}}});
for(const file of files)if(hashes[file]!==hash(await readFile(resolve(root,file))))throw Error(`Source changed while building: ${file}`);
const artifacts={};async function scan(directory){for(const entry of await readdir(directory,{withFileTypes:true})){const path=resolve(directory,entry.name);if(entry.isDirectory())await scan(path);else artifacts[relative(output,path)]=hash(await readFile(path));}}await scan(output);
const vite=JSON.parse(await readFile(resolve(root,'node_modules/vite/package.json'),'utf8'));
const metadata={snapshot,createdAt:new Date().toISOString(),node:process.version,vite:vite.version,sourceSha256:hashes,artifactSha256:artifacts,
 scope:'Frozen production DOM/GPU lifecycle soak: four live scenes, two cores, shared device, rotating owners and explicit snapshots. No forced-GC, driver memory/RSS or long-term plateau qualification.'};
await writeFile(resolve(output,'metadata.json'),JSON.stringify(metadata,null,2)+'\n');
await mkdir(cache,{recursive:true});await writeFile(resolve(cache,'latest.json'),JSON.stringify({snapshot,output,url:`http://127.0.0.1:${port}/tests/browser/dom-gpu-soak.html`},null,2)+'\n');
console.log(JSON.stringify({snapshot,output,url:`http://127.0.0.1:${port}/tests/browser/dom-gpu-soak.html`},null,2));
if(values.serve){const server=await preview({root,configFile:false,build:{outDir:output},preview:{host:'127.0.0.1',port,strictPort:true}});server.printUrls();}
