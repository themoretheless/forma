import {createServer} from 'node:http';
import {readFile,writeFile} from 'node:fs/promises';
import {resolve} from 'node:path';
import {createHash} from 'node:crypto';
if(!process.argv[2]||!process.argv[3])throw Error('Expected wasm-bindgen --target web output directory and result path');
const root=resolve(process.argv[2]),result=resolve(process.argv[3]);
const files={'/':['index.html','text/html'],'/forma.js':['forma.js','text/javascript'],'/forma_bg.wasm':['forma_bg.wasm','application/wasm']};
const hashes={};for(const [file] of Object.values(files))hashes[file]=createHash('sha256').update(await readFile(resolve(root,file))).digest('hex');
const server=createServer(async(req,res)=>{try{
 if(req.url==='/result'&&req.method==='POST'){
  let text='';for await(const part of req){text+=part;if(text.length>65536)throw Error('Oversized result');}
  const report=JSON.parse(text);report.servedArtifactSha256=hashes;
  await writeFile(result,JSON.stringify(report,null,2)+'\n');res.end('saved');return;
 }
 const file=files[req.url];if(!file){res.writeHead(404);res.end();return;}
 const bytes=await readFile(resolve(root,file[0]));
 if(createHash('sha256').update(bytes).digest('hex')!==hashes[file[0]])throw Error('Artifact changed after launch');
 res.setHeader('Content-Type',file[1]);res.end(bytes);
}catch(error){res.writeHead(500);res.end(String(error));}});
server.listen(0,'127.0.0.1',()=>console.log('http://127.0.0.1:'+server.address().port+'/'));
