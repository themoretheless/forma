import assert from 'node:assert/strict';
import {createServer,get} from 'node:http';
import {readFile,writeFile} from 'node:fs/promises';
import {createHash} from 'node:crypto';
import {resolve} from 'node:path';
import {pathToFileURL} from 'node:url';
import {JSDOM} from 'jsdom';
const runtime=process.argv[2]?pathToFileURL(resolve(process.argv[2],'dist/package')+'/'):new URL('../dist/package/',import.meta.url);
const output=process.argv[3],rounds=Number(process.argv[4]??20);assert(Number.isSafeInteger(rounds)&&rounds>0&&rounds<=1000);
const {h,createContext,useContext,signal,onCleanup,getCurrentOwner,getWasmCore,getGraphTelemetry}=await import(new URL('index.js',runtime));
const {renderToStringAsync}=await import(new URL('server.js',runtime));
const dom=new JSDOM('<!doctype html><html><body></body></html>');for(const name of ['document','Node','Element'])globalThis[name]=dom.window[name];
const gate=()=>{let release;const promise=new Promise(resolve=>release=resolve);return {promise,release};};
const context=createContext('missing'),states=new Map(),clients=new Set();let active=0,peak=0,completed=0;
const report={protocol:'concurrent-http-async-ssr-v1',startedAt:new Date().toISOString(),node:process.version,rounds,width:12,rows:[],scope:'Real loopback HTTP, async Node/JSDOM SSR, disconnect before headers. No streaming-body SSR, proxy/browser/Edge or memory-plateau qualification.'};
const bounded=async promise=>{let timer;try{return await Promise.race([promise,new Promise((_,reject)=>timer=setTimeout(()=>reject(Error('HTTP SSR deadline exceeded')),10000))]);}finally{clearTimeout(timer);}};
const server=createServer((request,response)=>{
 const id=new URL(request.url,'http://localhost').searchParams.get('id'),state=states.get(id);
 if(!state||state.started){response.writeHead(404);response.end();return;}
 state.started=true;state.response=response;active++;peak=Math.max(peak,active);
 const abort=new AbortController(),hooks=new Set(),add=abort.signal.addEventListener.bind(abort.signal),remove=abort.signal.removeEventListener.bind(abort.signal);
 abort.signal.addEventListener=(type,fn,options)=>{if(type==='abort')hooks.add(fn);return add(type,fn,options);};
 abort.signal.removeEventListener=(type,fn,options)=>{if(type==='abort')hooks.delete(fn);return remove(type,fn,options);};
 response.once('close',()=>{state.closedBeforeEnd=!response.writableEnded;if(state.closedBeforeEnd)abort.abort(state.disconnect);state.closed.release();});
 const Child=async()=>{
  const owner=getCurrentOwner(),source=signal(id);onCleanup(source.dispose);onCleanup(()=>state.cleanups++);state.entered.release();
  await state.resume.promise;
  if(state.mode==='disconnect'){
   try{h('p',null,'late');state.lateAccepted=true;}catch(error){state.lateError=String(error);}
   state.late.release();return document.createElement('p');
  }
  assert.equal(getCurrentOwner(),owner);assert.equal(useContext(context),id);assert.equal(getWasmCore(),state.core);
  if(state.mode==='failure')throw state.failure;
  return h('p',{'data-request':id},source());
 };
 void (async()=>{
  try{
   const rendered=await renderToStringAsync(()=>{state.core=getWasmCore();return {node:h(context.Provider,{value:id,children:()=>h(Child,null)})};},{signal:abort.signal});
   assert.equal(state.mode,'success');assert(!response.destroyed,'A destroyed response received SSR output');
   state.writes++;response.writeHead(200,{'content-type':'text/html'});response.end(rendered.html);state.outcome='success';
  }catch(error){
   state.outcome=state.mode==='disconnect'&&error===state.disconnect?'disconnected':state.mode==='failure'&&error===state.failure?'failure':'unexpected';
   if(state.outcome==='unexpected')state.unexpected=String(error.stack??error);
   if(!response.destroyed){state.writes++;response.writeHead(500,{'content-type':'application/json'});response.end(JSON.stringify({error:'expected component failure'}));}
  }finally{
   state.nodes=state.core?.svet_get_node_count();state.pending=state.core?.svet_pending_effect_count();state.hooks=hooks.size;active--;completed++;state.settled.release();
  }
 })();
});
function client(url){let request;const observed=new Promise(resolve=>{request=get(url,response=>{let body='';response.setEncoding('utf8');response.on('data',chunk=>body+=chunk);response.on('end',()=>resolve({status:response.statusCode,body}));response.on('error',error=>resolve({error:String(error)}));});request.on('error',error=>resolve({error:String(error)}));});clients.add(request);request.once('close',()=>clients.delete(request));return {request,observed};}
try{
 await new Promise((resolve,reject)=>{server.once('error',reject);server.listen(0,'127.0.0.1',resolve);});const base=`http://127.0.0.1:${server.address().port}`;
 for(let round=0;round<rounds;round++){
  const group=Array.from({length:12},(_,i)=>{const mode=['success','failure','disconnect'][i%3],id=`r${round}-${mode}-${i}`;const state={id,mode,entered:gate(),resume:gate(),closed:gate(),settled:gate(),late:gate(),failure:Error('component failure'),disconnect:Error('client disconnected'),cleanups:0,writes:0};states.set(id,state);return state;});
  const requests=group.map(state=>client(`${base}/?id=${state.id}`));
  await bounded(Promise.all(group.map(state=>state.entered.promise)));
  assert.equal(active,12);assert.equal(new Set(group.map(state=>state.core)).size,12);for(const state of group)assert(state.core.svet_get_node_count()>0);
  group.forEach((state,i)=>{if(state.mode==='disconnect')requests[i].request.destroy(Error('client disconnect'));});
  await bounded(Promise.all(group.filter(state=>state.mode==='disconnect').map(state=>state.closed.promise)));
  await bounded(Promise.all(group.filter(state=>state.mode==='disconnect').map(state=>state.settled.promise)));
  assert.equal(active,8,'Disconnect stopped or released a sibling prematurely');
  for(const state of group){if(state.mode!=='disconnect')assert.equal(state.core.svet_get_node_count()>0,true);state.resume.release();}
  const results=await bounded(Promise.all(requests.map(item=>item.observed)));
  await bounded(Promise.all(group.map(state=>state.settled.promise)));await bounded(Promise.all(group.map(state=>state.closed.promise)));
  await bounded(Promise.all(group.filter(state=>state.mode==='disconnect').map(state=>state.late.promise)));
  group.forEach((state,i)=>{
   assert.equal(state.cleanups,1);assert.equal(state.nodes,0);assert.equal(state.pending,0);assert.equal(state.hooks,0);assert.notEqual(state.outcome,'unexpected',state.unexpected);
   if(state.mode==='success'){assert.equal(results[i].status,200);assert.equal(results[i].body,`<p data-request="${state.id}">${state.id}</p>`);assert.equal(state.closedBeforeEnd,false);}
   else if(state.mode==='failure'){assert.equal(results[i].status,500);assert.equal(state.outcome,'failure');assert.equal(state.closedBeforeEnd,false);}
   else{assert(results[i].error);assert.equal(state.outcome,'disconnected');assert.equal(state.closedBeforeEnd,true);assert.equal(state.writes,0);assert.equal(state.lateAccepted,undefined);assert.match(state.lateError,/completed|disposed|reset/);}
   report.rows.push({id:state.id,mode:state.mode,outcome:state.outcome,client:results[i],closedBeforeEnd:state.closedBeforeEnd,cleanups:state.cleanups,nodes:state.nodes,pending:state.pending,abortHooks:state.hooks,writes:state.writes,lateError:state.lateError});states.delete(state.id);
  });assert.equal(active,0);
 }
 assert.equal(completed,rounds*12);assert.equal(peak,12);assert.equal(getGraphTelemetry().nodes,0);report.status='passed';
}catch(error){report.status='failed';report.error=String(error.stack??error);process.exitCode=1;}
finally{
 for(const state of states.values())state.resume.release();for(const request of clients)request.destroy();server.closeAllConnections();await new Promise(resolve=>server.close(resolve));
 Object.assign(report,{completedAt:new Date().toISOString(),completedRequests:completed,peakRequests:peak,activeRequests:active});
 report.sourceSha256=createHash('sha256').update(await readFile(new URL(import.meta.url))).digest('hex');report.runtimeSha256={};for(const name of ['server.js','dom.js','reactivity.js','wasm-binary.js'])report.runtimeSha256[name]=createHash('sha256').update(await readFile(new URL(name,runtime))).digest('hex');
 if(output)await writeFile(output,JSON.stringify(report,null,2)+'\n');console.log(JSON.stringify({status:report.status,error:report.error,completedRequests:completed,peakRequests:peak,activeRequests:active}));dom.window.close();
}
