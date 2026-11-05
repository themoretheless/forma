import assert from 'node:assert/strict';
import {mkdtemp,readFile,writeFile,cp,symlink,stat,realpath} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import {resolve,dirname} from 'node:path';
import {pathToFileURL} from 'node:url';
import {createHash} from 'node:crypto';
import {createServer} from 'vite';
import {writeProbeSource,writeModes} from './forma-probe-writes.mjs';
assert(process.argv[2]&&process.argv[3],'Expected installed package and report path');
const port=Number(process.argv[4]??4215);assert(Number.isInteger(port)&&port>=1024&&port<=65535);
const minInterval=Number(process.argv[5]??250);assert(Number.isInteger(minInterval)&&minInterval>=0&&minInterval<=1000);
const writeMode=process.argv[7]??'direct';assert(writeModes.includes(writeMode));
const installed=resolve(process.argv[2]),output=resolve(process.argv[3]),root=await mkdtemp(resolve(tmpdir(),'svet-real-hmr-'));
const {svetVitePlugin}=await import(pathToFileURL(resolve(installed,'dist/package/compiler.js')));
const source=version=>`component HmrCard { ${version===8?'#[range(100,10)] prop invalid: Number = 5; ':''}state count: Number = 1; Column { Text { key:'label'; text:'v${version}:\${state.count}'; } Button { key:'increment'; text:'add'; clicked -> state.increment('count',1); } } }`;
const entry=resolve(installed,'dist/package/index.js'),runtime='/@fs'+entry;
const fixtureName=process.argv[6]??'forma-hmr-runner.html';assert(['forma-hmr-runner.html','forma-hmr-burst.html'].includes(fixtureName));
const fixture=await readFile(new URL('../tests/browser/'+fixtureName,import.meta.url),'utf8');
await cp(resolve(installed,'dist/package'),resolve(root,'peer'),{recursive:true});
await symlink(dirname(installed),resolve(root,'node_modules'),'dir');
await writeFile(resolve(root,'index.html'),fixture.replace('__RUNTIME_IMPORT__',JSON.stringify(runtime)).replace('__PEER_RUNTIME_IMPORT__',JSON.stringify('/peer/index.js')));
await writeFile(resolve(root,'View.ui'),source(1));
const changes=[],watchTrace=[];let lastUpdate=0;
const trace=(kind,data={})=>{if(watchTrace.length<512)watchTrace.push({kind,at:Date.now(),...data});};
const finalDiagnostic=async server=>{
 const file=resolve(root,'View.ui'),bytes=await readFile(file),info=await stat(file),last=changes.at(-1);
 const expected=last?createHash('sha256').update(source(last.version)).digest('hex'):null;
 const canonicalFile=await realpath(file),byUrl=await server.moduleGraph.getModuleByUrl('/View.ui');
 const found=new Set([...(server.moduleGraph.getModulesByFile(file)??[]),...(server.moduleGraph.getModulesByFile(canonicalFile)??[]),...(byUrl?[byUrl]:[])]);
 const modules=[...found].map(node=>({
  id:node.id,lastHMRTimestamp:node.lastHMRTimestamp,lastInvalidationTimestamp:node.lastInvalidationTimestamp,
  cachedCodeSha256:node.transformResult?createHash('sha256').update(node.transformResult.code).digest('hex'):null,
  cachedVersionMarkers:node.transformResult?[...new Set(node.transformResult.code.match(/v\d+:/g)??[])]:[],
 }));
 const sha256=createHash('sha256').update(bytes).digest('hex');
 return {lastRequestedVersion:last?.version??null,disk:{sha256,expectedSha256:expected,matchesExpected:expected===sha256,size:info.size,mtimeMs:info.mtimeMs},modules,traceTruncated:watchTrace.length>=512};
};
const probe={name:'svet-hmr-probe-host',configureServer(server){
 server.ws.on('svet-probe:ready',(_data,client)=>{trace('client-ready');client.send('svet-probe:ready',{ready:true});});
 server.watcher.on('all',(event,file)=>{if(file.endsWith('/View.ui'))trace('watcher',{event,file});});
 server.watcher.on('raw',(event,file)=>{if(String(file).includes('View.ui'))trace('raw',{event,file});});
 const send=server.ws.send.bind(server.ws);server.ws.send=(...args)=>{trace('ws',{payload:args[0]});return send(...args);};
 server.middlewares.use(async(req,res,next)=>{try{
 if(req.url==='/__probe/update'&&req.method==='POST'){let body='';for await(const part of req){body+=part;if(body.length>16)throw Error('Invalid update');}const version=Number(body);assert(Number.isInteger(version)&&version>=2&&version<=200);const pause=Math.max(0,minInterval-(Date.now()-lastUpdate));if(pause)await new Promise(resolve=>setTimeout(resolve,pause));trace('write-start',{version});await writeProbeSource(resolve(root,'View.ui'),source(version),writeMode);trace('write-end',{version});lastUpdate=Date.now();changes.push({version,at:new Date().toISOString()});res.end('updated');return;}
 if(req.url==='/__probe/result'&&req.method==='POST'){let body='';for await(const part of req){body+=part;if(body.length>65536)throw Error('Invalid result');}const report=JSON.parse(body);report.finalDiagnostic=await finalDiagnostic(server);report.fileChanges=changes;report.updateMinIntervalMs=minInterval;report.watchTrace=watchTrace;report.serverNode=process.version;report.writeMode=writeMode;report.installedPackage=installed;report.peerRuntime=resolve(root,'peer');report.fixtureSha256=createHash('sha256').update(fixture).digest('hex');await writeFile(output,JSON.stringify(report,null,2)+'\n');res.end('saved');return;}
 next();}catch(error){res.statusCode=500;res.end(String(error));}});}};
const observer={name:'svet-hmr-observer',handleHotUpdate(ctx){trace('hot-hook',{file:ctx.file});},transform(code,id){if(id.split('?')[0].endsWith('/View.ui'))trace('transform',{sha256:createHash('sha256').update(code).digest('hex')});}};
const server=await createServer({root,configFile:false,plugins:[svetVitePlugin({runtimeImportSource:runtime}),probe,observer],server:{host:'127.0.0.1',port,strictPort:true,fs:{allow:[root,dirname(installed)]}}});
await server.listen();server.printUrls();console.log(JSON.stringify({root,installed,output}));
