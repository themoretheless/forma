import {createHash} from 'node:crypto';
import {mkdir,readFile,readdir,writeFile} from 'node:fs/promises';
import {dirname,resolve} from 'node:path';
import {fileURLToPath} from 'node:url';
import ts from 'typescript';

const root=resolve(dirname(fileURLToPath(import.meta.url)),'..');
const output=resolve(root,'node_modules/.cache/svet-adoption-bench');
const source=resolve(root,'src/engine');
const snapshot='snapshot-'+Date.now();
const from=`    const descendants = node.childNodes;
    assertCurrent();
    for (const descendant of descendants) pending.push(descendant);`;
const to=`    const first = node.firstChild;
    assertCurrent();
    for (let descendant = first; descendant; descendant = descendant.nextSibling) pending.push(descendant);`;
const metadata={snapshot,createdAt:new Date().toISOString(),node:process.version,platform:process.platform,arch:process.arch,typescript:ts.version,variants:{}};
for(const variant of ['baseline','candidate']){
 const directory=resolve(output,snapshot,variant);await mkdir(directory,{recursive:true});const hashes={};
 for(const file of (await readdir(source)).filter(file=>file.endsWith('.ts')).sort()){
  let text=await readFile(resolve(source,file),'utf8');
  if(file==='dom.ts'){
   if(text.includes(from)===text.includes(to))throw Error('Expected exactly one known traversal; update benchmark preparation explicitly');
   text=variant==='candidate'?text.replace(from,to):text.replace(to,from);
  }
  hashes[file]=createHash('sha256').update(text).digest('hex');
  const result=ts.transpileModule(text,{fileName:file,compilerOptions:{target:ts.ScriptTarget.ES2022,module:ts.ModuleKind.ESNext,rewriteRelativeImportExtensions:true}});
  await writeFile(resolve(directory,file.replace(/\.ts$/,'.js')),result.outputText);
 }
 metadata.variants[variant]=hashes;
}
await writeFile(resolve(output,'metadata.json'),JSON.stringify(metadata,null,2)+'\n');
console.log(JSON.stringify({output,fixture:'/tests/browser/adoption-bench.html',domHashes:Object.fromEntries(Object.entries(metadata.variants).map(([key,value])=>[key,value['dom.ts']]))},null,2));
