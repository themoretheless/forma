import {createGzip,constants as zlibConstants} from 'node:zlib';
import {pipeline} from 'node:stream/promises';
import {JSDOM} from 'jsdom';
const json=(response,status,value)=>{response.writeHead(status,{'content-type':'application/json','cache-control':'no-store'});response.end(JSON.stringify(value));};
export function createActionFixture(){
const namespace='/__svet_action_http/',requests=new Map();
return {handle(request,response,next){
        const url = new URL(request.url, 'http://127.0.0.1');
        if (!url.pathname.startsWith(namespace)) return next();
        if (request.method !== 'GET') return json(response, 405, {error: 'GET only'});
        const id = url.searchParams.get('id');
        if (!id || id.length > 100) return json(response, 400, {error: 'Test request id required'});
        const operation = url.pathname.slice(namespace.length);
        if (operation === 'wait') {
          if (requests.has(id) || requests.size >= 256) return json(response, 409, {error: 'Duplicate id or fixture capacity'});
          const streaming = url.searchParams.get('stream') === '1';
          const state = {response, streaming, closed: false, complete: false};
          requests.set(id, state);
          response.once('close', () => { state.closed = true; });
          if (streaming) {
            response.writeHead(200, {'content-type': 'application/json', 'cache-control': 'no-store'});
            response.write('{"value":');
          }
          return;
        }
        const state = requests.get(id);
        if (operation === 'stats') return json(response, 200, state ? {
          started: true, streaming: state.streaming, closed: state.closed,
          complete: state.complete, writableEnded: state.response.writableEnded,
        } : {started: false});
        if (operation === 'finish') {
          if (!state || state.closed) return json(response, 404, {error: 'No active test response'});
          state.complete = true;
          const value = Number(url.searchParams.get('value'));
          if (state.streaming) state.response.end(`${JSON.stringify(value)}}`);
          else json(state.response, 200, {value});
          return json(response, 200, {finished: true});
        }
        if (operation === 'forget') {
          if (state && !state.response.writableEnded) state.response.destroy();
          requests.delete(id);
          return json(response, 200, {forgotten: true});
        }
        json(response, 404, {error: 'Unknown fixture operation'});
},close(){for(const state of requests.values())state.response.destroy();requests.clear();}};
}

export function createResumptionFixture(engine,provenance=null){
const dom = new JSDOM('<!doctype html><html><body></body></html>');
const {getGraphTelemetry,h,onCleanup,renderToString,signal}=engine;
let renders = 0;
// A server host must keep its DOM implementation alive across await boundaries.
const globals = new Map(['document', 'Node', 'Element'].map(name => [name, Object.getOwnPropertyDescriptor(globalThis, name)]));
for (const name of globals.keys()) Object.defineProperty(globalThis, name, {value: dom.window[name], configurable: true, writable: true});
function renderFixture() {
    const named = {};
    let isolatedNodesAfterCleanup = -1;
    const result = renderToString(() => {
      renders++;
      onCleanup(() => { isolatedNodesAfterCleanup = getGraphTelemetry().nodes; });
      for (const [key, value] of Object.entries({checked:true, amount:12, choice:'b', many:['a','c'], text:'<b>not markup</b> & "quoted"', disabled:false, kind:'text', typed:true})) {
        named[key] = signal(value);
        onCleanup(named[key].dispose);
      }
      const input = (id, type, key, props = {}) => h('input', {id, type, 'data-svet-model':key, model:named[key], ...props});
      return h('section', null,
        h('label', null, 'Checked ', input('checked', 'checkbox', 'checked')),
        h('label', null, 'Amount ', input('amount', 'number', 'amount')),
        h('label', null, 'Choice A ', input('choice-a', 'radio', 'choice', {name:'resume-choice',value:'a'})),
        h('label', null, 'Choice B ', input('choice-b', 'radio', 'choice', {name:'resume-choice',value:'b'})),
        h('select', {id:'many',multiple:true,'data-svet-model':'many',model:named.many}, ...['a','b','c'].map(value=>h('option',{value},value))),
        h('label', null, 'Text ', input('text', 'text', 'text')),
        h('label', null, 'Reactive type ', input('typed', named.kind, 'typed', {'data-svet-attr':'type:kind'})),
        h('button', {id:'pressed','aria-pressed':named.checked,'data-svet-attr':'aria-pressed:checked'}, 'Pressed state'),
        h('button', {id:'disabled',disabled:named.disabled,'data-svet-attr':'disabled:disabled'}, 'Disabled state'));
    }, named);
    return {...result, renderCount:renders, isolatedNodesAfterCleanup, mainGraphNodes:getGraphTelemetry().nodes};

}

return {handle(request,response,next){
if(request.url==='/__svet_async_component_fixture'){
 if(request.method!=='GET')return json(response,405,{error:'GET only'});
 if(!engine.renderToStringAsync)return json(response,503,{error:'Async server entry unavailable'});
 const abort=new AbortController();response.once('close',()=>{if(!response.writableEnded)abort.abort(Error('HTTP client disconnected'));});
 const context=engine.createContext('missing'),named={};let core,cleaned=0;
 return engine.renderToStringAsync(()=>{
  core=engine.getWasmCore();
  const Child=async({label})=>{await Promise.resolve();const value=engine.useContext(context);engine.onCleanup(()=>cleaned++);return h('p',{'data-owned':label},value);};
  const Form=async()=>{await Promise.resolve();named.count=signal(7);onCleanup(named.count.dispose);return h('input',{id:'async-count',type:'number','data-svet-model':'count',model:named.count});};
  return {node:h('section',null,h(context.Provider,{value:'left',children:()=>h(Child,{label:'left'})}),h(context.Provider,{value:'right',children:()=>h(Child,{label:'right'})}),h(Form,null)),signals:named};
 },{signal:abort.signal}).then(result=>json(response,200,{...result,cleaned,requestNodes:core.svet_get_node_count(),mainGraphNodes:getGraphTelemetry().nodes,provenance}),error=>{if(!response.destroyed)json(response,500,{error:String(error)});});
}
if(request.url!=='/__svet_resumption_fixture')return next();
if(request.method!=='GET')return json(response,405,{error:'GET only'});
try{return json(response,200,{...renderFixture(),provenance});}catch(error){return json(response,500,{error:String(error)});}
},close(){for(const [name,descriptor] of globals){if(descriptor)Object.defineProperty(globalThis,name,descriptor);else delete globalThis[name];}dom.window.close();}};
}


export function createStreamingFixture(engine,provenance=null,diagnostics=null){
 const states=new Map();
 const gate=()=>{let release;const promise=new Promise(resolve=>release=resolve);return {promise,release};};
 return {handle(request,response,next){
  const url=new URL(request.url,'http://localhost');if(!url.pathname.startsWith('/__svet_stream/'))return next();
  if(request.method!=='GET')return json(response,405,{error:'GET only'});
  const id=url.searchParams.get('id');if(!id||id.length>100)return json(response,400,{error:'Missing id'});
  const operation=url.pathname.slice('/__svet_stream/'.length),state=states.get(id);
  if(operation==='stats')return json(response,200,state?{started:true,finished:state.finished,nodes:state.core?.svet_get_node_count(),drains:state.drains,gzip:state.gzip,flushes:state.flushes,compressedBytes:state.compressedBytes,phase:state.phase,flushInjection:state.flushInjection,pipelineSettled:state.pipelineSettled,compressorClosed:state.compressorClosed,cleaned:state.cleaned,payloadSize:state.payloadSize}: {started:false});
  if(operation==='release'){if(!state)return json(response,404,{error:'No stream'});state.gate.release();return json(response,200,{released:true});}
  if(operation==='abort'){if(!state)return json(response,404,{error:'No stream'});state.abort.abort(Error('Fixture external abort'));return json(response,200,{aborted:true});}
  if(operation==='forget'){state?.abort.abort(Error('Fixture forgotten'));state?.gate.release();states.delete(id);return json(response,200,{forgotten:true});}
  if(operation!=='open')return json(response,404,{error:'Unknown operation'});
  if(state||states.size>=32)return json(response,409,{error:'Fixture capacity'});
  const offer=request.headers[engine.SSR_STREAM_CODEC_HEADER],codec=engine.negotiateSsrStreamCodec(typeof offer==='string'?offer:null);
  if(!codec)return json(response,406,{error:'No compatible stream codec'});
  const requestedSize=url.searchParams.get('payload'),payloadSize=requestedSize===null?response.writableHighWaterMark*2+1024:Number(requestedSize);
  if(!Number.isSafeInteger(payloadSize)||payloadSize<1||payloadSize>4*1024*1024)return json(response,400,{error:'Invalid payload size'});
  response.once('error',()=>{});
  const current={gate:gate(),abort:new AbortController(),finished:false,cleaned:0,drains:0,gzip:url.searchParams.get('gzip')==='1',flushes:0,compressedBytes:0,phase:'pending',pipelineSettled:false,compressorClosed:false,core:null,payloadSize};states.set(id,current);
  response.once('close',()=>{if(!response.writableEnded)current.abort.abort(Error('HTTP stream disconnected'));});
  void(async()=>{try{
   const named={};const chunks=engine.renderToChunksAsync(()=>{
    current.core=engine.getWasmCore();diagnostics?.onCore?.(current.core);return {node:engine.h('main',null,engine.h('b',null,'early shell'),engine.h(async()=>{
     engine.onCleanup(()=>current.cleaned++);const pending=engine.signal(0);engine.onCleanup(pending.dispose);await current.gate.promise;named.count=engine.signal(7);engine.onCleanup(named.count.dispose);
     let text='x'.repeat(current.payloadSize);if(url.searchParams.get('varied')==='1'){const bytes=Buffer.alloc(current.payloadSize);let seed=1701;for(let i=0;i<bytes.length;i++){seed=(Math.imul(seed,1664525)+1013904223)>>>0;bytes[i]=97+((seed>>>16)%26);}text=bytes.toString('ascii');}
     return engine.h('section',null,engine.h('input',{id:'stream-count',type:'number','data-svet-model':'count',model:named.count}),engine.h('span',{'data-large':'yes'},text),engine.h('p',null,'<script>unsafe text</script>'));
    },null)),signals:named};
   },{signal:current.abort.signal});
   const encoder=codec==='identity'?null:engine.createSsrStreamTransportEncoder();
   const compressor=current.gzip?createGzip():null,target=compressor??response;
   compressor?.on('data',chunk=>current.compressedBytes+=chunk.length);
   const compressed=compressor?pipeline(compressor,response).then(()=>{current.pipelineSettled=true;return {error:null};},error=>{current.pipelineSettled=true;return {error};}):null;
   compressor?.once('close',()=>{current.compressorClosed=true;});
   const stopTransport=()=>{compressor?.destroy();response.destroy();};
   current.abort.signal.addEventListener('abort',stopTransport,{once:true});
   response.writeHead(200,{...(compressor?{'content-encoding':'gzip'}:{}),'content-type':'application/x-ndjson','cache-control':'no-store',[engine.SSR_STREAM_VERSION_HEADER]:'1',[engine.SSR_STREAM_CODEC_HEADER]:codec,vary:engine.SSR_STREAM_CODEC_HEADER});
   try{for await(const chunk of chunks){current.phase='writing';if(!target.write(JSON.stringify(encoder?encoder.encode(chunk):chunk)+'\n')){
    current.phase='drain';current.drains++;await new Promise((resolve,reject)=>{
     const done=()=>{cleanup();resolve();},closed=()=>{cleanup();reject(Error('HTTP stream closed during drain'));};
     const cleanup=()=>{target.removeListener('drain',done);response.removeListener('close',closed);};target.once('drain',done);response.once('close',closed);
    });
   }
   if(compressor){current.phase='flush';await new Promise((resolve,reject)=>{
    let pending=true;
    compressor.flush(zlibConstants.Z_SYNC_FLUSH,error=>{pending=false;error?reject(error):resolve();});
    if(url.searchParams.get('abort-flush')==='patch'&&current.flushes===1){
     current.flushInjection={pending,phase:current.phase,nodes:current.core.svet_get_node_count(),flushes:current.flushes};
     current.abort.abort(Error('Fixture abort during native pending flush'));
    }
   });current.flushes++;}current.phase='next';
   }
   if(compressor){compressor.end();const result=await compressed;if(result.error)throw result.error;}
   else response.end();
   }finally{current.abort.signal.removeEventListener('abort',stopTransport);encoder?.dispose();compressor?.destroy();}
   current.phase='finished';current.finished=true;
  }catch(error){current.error=String(error);current.phase='failed';current.finished=true;response.destroy(error);}})();
 },close(){for(const state of states.values()){state.abort.abort(Error('Server closed'));state.gate.release();}states.clear();}};
}
