import {defineConfig} from 'vite';
import {randomUUID} from 'node:crypto';
import {mkdirSync,writeFileSync,chmodSync,unlinkSync} from 'node:fs';
import {fileURLToPath} from 'node:url';
import {createRustRunner} from './rust-runner.js';
import {createNativeRunner} from './native-runner.js';
import {evaluateDesignData} from './design-data-runner.js';
import {createVectorRunner} from './vector-runner.js';
import {readFile} from 'node:fs/promises';

export default defineConfig({build:{rollupOptions:{input:{studio:'index.html',controls:'vector-ui/examples/controls.html'}}},server:{host:'127.0.0.1',port:5173,strictPort:true},plugins:[{
  name:'forma-mcp-bridge',
  configureServer(server){
    // Generated wasm-bindgen modules must be served unchanged, not transformed by Vite.
    server.middlewares.use('/__forma_vector',async(req,res,next)=>{
      const name=req.url?.split('?')[0];
      if(!['/forma.js','/forma_bg.wasm'].includes(name))return next();
      try{const bytes=await readFile(new URL('./public/vector-pkg'+name,import.meta.url));res.setHeader('Content-Type',name.endsWith('.wasm')?'application/wasm':'text/javascript');res.setHeader('Cache-Control','no-store');res.end(bytes);}catch{res.statusCode=503;res.end('Build vector-ui WASM first');}
    });
    const token=randomUUID();const pending=new Map();let client=null;
    const runner=createRustRunner(data=>client?.send('forma:rust-output',data));
    const native=createNativeRunner(data=>client?.send('forma:rust-output',data));
    const vector=createVectorRunner(data=>client?.send('forma:rust-output',data));
    server.ws.on('forma:vector-run',(data,sender)=>{if(sender===client)vector.run(data);});
    server.ws.on('forma:vector-stop',(_,sender)=>{if(sender===client)vector.stop();});
    server.httpServer.once('close',()=>vector.stop());
    let designBusy=false;
    server.ws.on('forma:design-data',async(data,sender)=>{
      if(sender!==client)return;
      if(designBusy){sender.send('forma:design-data-result',{id:data.id,error:'Вычисление уже выполняется'});return;}
      designBusy=true;
      try{sender.send('forma:design-data-result',{id:data.id,values:await evaluateDesignData(data.files,data.refs)});}
      catch(e){sender.send('forma:design-data-result',{id:data.id,error:e.message});}
      finally{designBusy=false;}
    });
    server.ws.on('forma:native-run',(data,sender)=>{if(sender===client)native.run(data);});
    server.ws.on('forma:native-stop',(_,sender)=>{if(sender===client)native.stop();});
    server.httpServer.once('close',()=>native.stop());
    server.ws.on('forma:form-run',(data,sender)=>{if(sender===client)runner.run(data.files,{generated:true,state:data.state});});
    server.ws.on('forma:rust-run',(data,sender)=>{if(sender===client)runner.run(data.files);});
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
