/** Scoped inspector connection for the generated local test Worker only. */
export async function openWorkerdInspector(miniflare){
 const base=await miniflare.getInspectorURL();const discovery=new URL('/json',base);discovery.protocol='http:';
 const response=await fetch(discovery,{signal:AbortSignal.timeout(10000)}),targets=await response.json();
 const candidates=targets.filter(target=>target.id?.startsWith('core:user:'));
 if(candidates.length!==1)throw Error('Expected one generated user Worker inspector target');
 const target=candidates[0];
 if(!target?.webSocketDebuggerUrl)throw Error('No workerd inspector target');
 const socket=new WebSocket(target.webSocketDebuggerUrl),pending=new Map(),events=[],subscribers=new Set();let sequence=0;
 socket.addEventListener('close',event=>{events.push({closed:event.code,reason:event.reason});for(const row of pending.values()){clearTimeout(row.timer);row.reject(Error('Inspector closed '+JSON.stringify(events.slice(-8))));}pending.clear();});
 socket.addEventListener('message',event=>{const message=JSON.parse(String(event.data));events.push({id:message.id,method:message.method,error:message.error});if(events.length>64)events.shift();for(const callback of subscribers)callback(message);const entry=pending.get(message.id);if(!entry)return;pending.delete(message.id);clearTimeout(entry.timer);message.error?entry.reject(Error(JSON.stringify(message.error))):entry.resolve(message.result);});
 await new Promise((resolve,reject)=>{const timer=setTimeout(()=>{socket.close();reject(Error('Inspector connection deadline'));},10000);socket.addEventListener('open',()=>{clearTimeout(timer);resolve();},{once:true});socket.addEventListener('error',error=>{clearTimeout(timer);reject(error);},{once:true});socket.addEventListener('close',()=>{clearTimeout(timer);reject(Error('Inspector closed before connection'));},{once:true});});
 return {target:{id:target.id,title:target.title},events,subscribe(callback){subscribers.add(callback);return ()=>subscribers.delete(callback);},send(method,params={}){return new Promise((resolve,reject)=>{const id=++sequence,timer=setTimeout(()=>{pending.delete(id);reject(Error('Inspector command deadline: '+method+' '+JSON.stringify(events.slice(-8))));},10000);pending.set(id,{resolve,reject,timer});socket.send(JSON.stringify({id,method,params}));});},close(){for(const row of pending.values()){clearTimeout(row.timer);row.reject(Error('Inspector closed'));}pending.clear();socket.close();}};
}
