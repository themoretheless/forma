import {defineConfig,searchForWorkspaceRoot} from 'vite';
import {randomUUID} from 'node:crypto';
import {mkdirSync,writeFileSync,chmodSync,unlinkSync,realpathSync} from 'node:fs';
import {fileURLToPath} from 'node:url';
import {createRustRunner} from './rust-runner.js';
import {createNativeRunner} from './native-runner.js';
import {evaluateDesignData} from './design-data-runner.js';
import {createVectorRunner} from './vector-runner.js';
import {readFile,readdir,writeFile,stat,lstat,realpath,mkdir,rename,rm,cp} from 'node:fs/promises';
import {resolve as resolvePath,relative as relativePath,dirname as dirnamePath,basename as basenamePath,extname as extnamePath,join as joinPath} from 'node:path';

const openScadRoot=realpathSync(fileURLToPath(new URL('../open-scad-viewer',import.meta.url)));
const ALLOWED_HOSTS=new Set(['127.0.0.1:5173','localhost:5173']);
const ALLOWED_ORIGINS=new Set(['http://127.0.0.1:5173','http://localhost:5173']);
function assertLocalWorkspaceRequest(req){
  const host=req.headers.host;
  if(host&&!ALLOWED_HOSTS.has(host))throw Error('Forbidden host');
  if(req.method==='POST'){
    const origin=req.headers.origin;
    if(origin&&!ALLOWED_ORIGINS.has(origin))throw Error('Forbidden origin');
    const fetchSite=req.headers['sec-fetch-site'];
    if(fetchSite&&fetchSite!=='same-origin'&&fetchSite!=='none')throw Error('Cross-site request blocked');
  }
}
async function resolveWorkspacePath(rel=''){
  const clean=String(rel??'').replace(/^\/+/,'');
  if(clean.includes('\0'))throw Error('Invalid workspace path');
  const full=resolvePath(openScadRoot,clean);
  const diff=relativePath(openScadRoot,full);
  if(diff.startsWith('..'))throw Error('Invalid workspace path');
  // Walk up to the nearest existing ancestor and verify its realpath stays inside openScadRoot
  let probe=full;
  while(probe!==openScadRoot){
    try{
      const st=await lstat(probe);
      if(st.isSymbolicLink())throw Error('Symlinks are not allowed in workspace');
      const real=await realpath(probe);
      const realDiff=relativePath(openScadRoot,real);
      if(realDiff.startsWith('..'))throw Error('Workspace path escapes root via symlink');
      break;
    }catch(e){
      if(e.code!=='ENOENT')throw e;
      const parent=dirnamePath(probe);
      if(parent===probe)break;
      probe=parent;
    }
  }
  return full;
}

// The language package is linked from the sibling tokenizer checkout (scripts/tokenizer.mjs); its
// JS reaches Vite through imports, but the WASM it fetches by URL is outside the root and got 403.
const tokenizerPackage=(()=>{try{return realpathSync(fileURLToPath(new URL('./node_modules/@themoretheless/tokenizer-forma',import.meta.url)));}catch{return null;}})();

// Pre-bundling moves the package's JS into .vite/deps, where its relative WASM URL no longer resolves.
export default defineConfig({optimizeDeps:{exclude:['@themoretheless/tokenizer-forma']},build:{target:'esnext',chunkSizeWarningLimit:600,rollupOptions:{input:{studio:'index.html',controls:'vector-ui/examples/controls.html'},output:{manualChunks(id){if(id.includes('@codemirror/')||id.includes('@lezer/'))return 'editor';}}}},server:{host:'127.0.0.1',port:5173,strictPort:true,fs:{allow:[searchForWorkspaceRoot(fileURLToPath(new URL('.',import.meta.url))),...(tokenizerPackage?[tokenizerPackage]:[])]}},plugins:[{
  name:'forma-mcp-bridge',
  configureServer(server){
    // Generated wasm-bindgen modules must be served unchanged, not transformed by Vite.
    server.middlewares.use('/__forma_vector',async(req,res,next)=>{
      const name=req.url?.split('?')[0];
      if(!['/forma.js','/forma_bg.wasm'].includes(name))return next();
      try{const bytes=await readFile(new URL('./public/vector-pkg'+name,import.meta.url));res.setHeader('Content-Type',name.endsWith('.wasm')?'application/wasm':'text/javascript');res.setHeader('Cache-Control','no-store');res.end(bytes);}catch{res.statusCode=503;res.end('Build vector-ui WASM first');}
    });
    server.middlewares.use('/__forma_tokenizer',async(req,res,next)=>{
      const name=req.url?.split('?')[0];
      if(!['/tokenizer_wasm.js','/tokenizer_wasm_bg.wasm'].includes(name))return next();
      try{const bytes=await readFile(new URL('../tokenizer/playground/public/wasm'+name,import.meta.url));res.setHeader('Content-Type',name.endsWith('.wasm')?'application/wasm':'text/javascript');res.setHeader('Cache-Control','no-store');res.end(bytes);}catch{res.statusCode=503;res.end('Tokenizer WASM unavailable');}
    });
    server.middlewares.use('/__forma_workspace',async(req,res)=>{
      res.setHeader('Content-Type','application/json');
      const send=(status,value)=>{res.statusCode=status;res.end(JSON.stringify(value));};
      try{
        assertLocalWorkspaceRequest(req);
        const url=new URL(req.url??'/','http://127.0.0.1:5173');
        if(req.method==='GET'&&url.pathname==='/list'){
          const dir=url.searchParams.get('dir')??'';
          const full=await resolveWorkspacePath(dir);
          const dirents=await readdir(full,{withFileTypes:true});
          const visible=dirents.filter(d=>!d.name.startsWith('.')&&!d.isSymbolicLink());
          const entries=await Promise.all(visible.map(async d=>{
            const rel=dir?`${dir}/${d.name}`:d.name;
            if(!d.isDirectory())return {name:d.name,path:rel,kind:'file'};
            let count=0;
            try{const sub=await readdir(joinPath(full,d.name));count=sub.filter(n=>!n.startsWith('.')).length;}catch{}
            return {name:d.name,path:rel,kind:'directory',count};
          }));
          entries.sort((a,b)=>a.kind!==b.kind?(a.kind==='directory'?-1:1):a.name.localeCompare(b.name,undefined,{sensitivity:'base'}));
          return send(200,{dir,entries});
        }
        if(req.method==='GET'&&url.pathname==='/read'){
          const path=url.searchParams.get('path')??'';
          const full=await resolveWorkspacePath(path);
          const info=await stat(full);
          if(info.size>2_000_000)return send(413,{error:'Файл слишком большой для предпросмотра (> 2 МБ)'});
          const content=await readFile(full,'utf8');
          return send(200,{path,content});
        }
        if(req.method==='POST'&&url.pathname==='/write'){
          const path=url.searchParams.get('path')??'';
          const full=await resolveWorkspacePath(path);
          let body='';for await(const chunk of req){body+=chunk;if(body.length>2_000_000)return send(413,{error:'File too large'});}
          const {content}=JSON.parse(body||'{}');
          await mkdir(dirnamePath(full),{recursive:true});
          await writeFile(full,String(content??''),'utf8');
          return send(200,{ok:true,path});
        }
        if(req.method==='POST'&&url.pathname==='/mkdir'){
          const path=url.searchParams.get('path')??'';
          const full=await resolveWorkspacePath(path);
          await mkdir(full,{recursive:true});
          return send(200,{ok:true,path});
        }
        if(req.method==='POST'&&url.pathname==='/rename'){
          let body='';for await(const chunk of req){body+=chunk;if(body.length>65_536)return send(413,{error:'Payload too large'});}
          const {from,to}=JSON.parse(body||'{}');
          if(!from||!to)return send(400,{error:'from and to are required'});
          const fullFrom=await resolveWorkspacePath(from),fullTo=await resolveWorkspacePath(to);
          if(fullFrom===openScadRoot||fullTo===openScadRoot)return send(400,{error:'Cannot move workspace root'});
          if(fullTo.startsWith(fullFrom+'/'))return send(400,{error:'Нельзя переместить папку внутрь самой себя'});
          await mkdir(dirnamePath(fullTo),{recursive:true});
          await rename(fullFrom,fullTo);
          return send(200,{ok:true,from,to});
        }
        if(req.method==='POST'&&url.pathname==='/delete'){
          let body='';for await(const chunk of req){body+=chunk;if(body.length>65_536)return send(413,{error:'Payload too large'});}
          const {path}=JSON.parse(body||'{}');
          if(!path)return send(400,{error:'path is required'});
          const full=await resolveWorkspacePath(path);
          if(full===openScadRoot)return send(400,{error:'Cannot delete workspace root'});
          await rm(full,{recursive:true,force:false});
          return send(200,{ok:true,path});
        }
        if(req.method==='POST'&&url.pathname==='/duplicate'){
          let body='';for await(const chunk of req){body+=chunk;if(body.length>65_536)return send(413,{error:'Payload too large'});}
          const {path}=JSON.parse(body||'{}');
          if(!path)return send(400,{error:'path is required'});
          const full=await resolveWorkspacePath(path);
          const dir=dirnamePath(path)==='.'?'':dirnamePath(path);
          const ext=extnamePath(path),stem=basenamePath(path,ext);
          let copyRel='',copyFull='';
          for(let i=1;i<100;i++){
            const candidateName=`${stem}-copy${i>1?'-'+i:''}${ext}`;
            copyRel=dir?`${dir}/${candidateName}`:candidateName;
            copyFull=await resolveWorkspacePath(copyRel);
            try{await stat(copyFull);}catch{break;}
          }
          await cp(full,copyFull,{recursive:true});
          return send(200,{ok:true,path:copyRel});
        }
        return send(404,{error:'Not found'});
      }catch(e){return send(e.message.startsWith('Forbidden')||e.message.startsWith('Cross-site')?403:500,{error:e.message});}
    });
    const token=randomUUID();const pending=new Map();let client=null,selectedBackend='svet',selectedPlatform='desktop';
    const runner=createRustRunner(data=>client?.send('forma:rust-output',data));
    const native=createNativeRunner(data=>client?.send('forma:rust-output',data));
    const vector=createVectorRunner(data=>client?.send('forma:rust-output',data));
    server.ws.on('forma:backend',(data,sender)=>{if(sender===client&&['egui','svet','ask'].includes(data?.backend))selectedBackend=data.backend;});
    server.ws.on('forma:platform',(data,sender)=>{if(sender===client&&['desktop','ios','android'].includes(data?.platform))selectedPlatform=data.platform;});
    server.ws.on('forma:vector-run',(data,sender)=>{if(sender===client)vector.run({...data,backend:data?.backend??(selectedBackend!=='ask'?selectedBackend:undefined),platform:data?.platform??(selectedPlatform!=='desktop'?selectedPlatform:undefined)});});
    server.ws.on('forma:vector-update',(data,sender)=>{if(sender===client)vector.update(data);});
    server.ws.on('forma:vector-stop',(_,sender)=>{if(sender===client)vector.stop();});
    server.httpServer.once('close',()=>vector.stop());
    let designBusy=false;
    server.ws.on('forma:design-data',async(data,sender)=>{
      if(sender!==client)return;
      if(designBusy){sender.send('forma:design-data-result',{id:data?.id,error:'Вычисление уже выполняется'});return;}
      designBusy=true;
      try{sender.send('forma:design-data-result',{id:data?.id,values:await evaluateDesignData(data?.files,data?.refs)});}
      catch(e){sender.send('forma:design-data-result',{id:data?.id,error:e.message});}
      finally{designBusy=false;}
    });
    server.ws.on('forma:native-run',(data,sender)=>{if(sender===client)native.run(data);});
    server.ws.on('forma:native-update',(data,sender)=>{if(sender===client)native.update(data);});
    server.ws.on('forma:native-stop',(_,sender)=>{if(sender===client)native.stop();});
    server.httpServer.once('close',()=>native.stop());
    server.ws.on('forma:form-run',(data,sender)=>{if(sender===client)runner.run(data?.files,{generated:true,native:true,backend:data?.backend??selectedBackend,platform:data?.platform??selectedPlatform});});
    server.ws.on('forma:rust-run',(data,sender)=>{if(sender===client)runner.run(data?.files);});
    server.ws.on('forma:rust-stop',(_,sender)=>{if(sender===client)runner.stop();});
    server.httpServer.once('close',()=>runner.stop());
    const directory=fileURLToPath(new URL('./.forma/',import.meta.url));
    const descriptor=directory+'bridge.json';
    mkdirSync(directory,{recursive:true});
    server.httpServer.once('listening',()=>{
      writeFileSync(descriptor,JSON.stringify({url:'http://127.0.0.1:5173/__forma_mcp',token}),{mode:0o600});chmodSync(descriptor,0o600);
    });
    server.httpServer.once('close',()=>{try{unlinkSync(descriptor);}catch{}for(const p of pending.values())p.reject(Error('IDE server stopped'));});
    server.ws.on('forma:hello',(_,sender)=>{if(!client||client.socket.readyState!==1)client=sender;});
    server.ws.on('forma:result',(data,sender)=>{if(sender!==client)return;const p=pending.get(data.id);if(p){pending.delete(data.id);p.resolve(data);}});
    server.middlewares.use('/__forma_mcp',async(req,res)=>{
      res.setHeader('Content-Type','application/json');
      const send=(status,value)=>{res.statusCode=status;res.end(JSON.stringify(value));};
      if(req.headers.authorization!==`Bearer ${token}`||req.headers.origin||req.headers.host!=='127.0.0.1:5173')return send(403,{error:'Forbidden'});
      if(req.method!=='POST')return send(405,{error:'POST required'});
      if(!client||client.socket.readyState!==1)return send(503,{error:'Open the IDE browser tab first'});
      try{
        let body='';for await(const chunk of req){body+=chunk;if(body.length>2_000_000)return send(413,{error:'Request too large'});}
        const command=JSON.parse(body),id=randomUUID();
        const result=await new Promise((resolve,reject)=>{
          const timer=setTimeout(()=>{pending.delete(id);reject(Error('IDE did not reply. Outcome may be unknown; inspect state before retrying a write.'));},8000);
          pending.set(id,{resolve:r=>{clearTimeout(timer);resolve(r);},reject:e=>{clearTimeout(timer);reject(e);}});
          client.send('forma:command',{id,...command});
        });send(200,result);
      }catch(e){send(500,{error:e.message});}
    });
  }
} ]});
