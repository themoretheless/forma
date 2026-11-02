import * as a from 'svet-engine';
import * as b from 'svet-engine-copy';
const check=(value,message)=>{if(!value)throw Error(message);};
export async function crossCopyBoundaries(runtime){
 const cases=[];
 for(const [label,host,foreign] of [['A-B',a,b],['B-A',b,a]]){
  await runtime.runWithIsolatedContextAsync(async()=>{
   const core=a.getWasmCore(),container=document.createElement('section');let resource,resolvePending,aborts=0;
   const pending=new Promise(resolve=>resolvePending=resolve);
   const app=host.withWasmCore(core,()=>host.mount(()=>host.Suspense({fallback:'waiting',children:()=>{
    resource=foreign.createResource(({signal})=>{signal.addEventListener('abort',()=>aborts++);return pending;});return document.createTextNode('content');
   }}),container));
   try{
    const fallback=container.querySelector('[data-svet-suspense-fallback]');check(fallback.style.display==='contents','Foreign Edge pending invisible');
    resolvePending('ready');await pending;await Promise.resolve();check(resource()==='ready'&&fallback.style.display==='none','Foreign Edge completion stale');
    const job=resource.refetch();check(fallback.style.display==='contents','Foreign Edge refetch invisible');app.unmount();await job;
    check(aborts===1&&core.svet_get_node_count()===0,'Foreign Edge pending teardown leaked');
   }finally{app.unmount();}
   cases.push(label+' suspense');
  });
  await runtime.runWithIsolatedContextAsync(async()=>{
   const core=a.getWasmCore(),container=document.createElement('section'),reason=Error('foreign Edge rejection');let rejectPending,retry,mounts=0;
   const pending=new Promise((_,reject)=>rejectPending=reject);
   const app=host.withWasmCore(core,()=>host.mount(()=>host.ErrorBoundary({children:()=>{
    mounts++;foreign.createResource(()=>mounts===1?pending:Promise.resolve('ready'));return document.createTextNode('content');
   },fallback:(error,reset)=>{check(error===reason,'Foreign Edge rejection identity lost');retry=reset;return document.createTextNode('error');}}),container));
   try{
    rejectPending(reason);await pending.catch(()=>{});await Promise.resolve();check(container.textContent==='error','Foreign Edge boundary missed rejection');
    retry();await Promise.resolve();check(mounts===2&&container.textContent==='content','Foreign Edge retry failed '+JSON.stringify({label,mounts,text:container.textContent,html:container.innerHTML}));
   }finally{app.unmount();}check(core.svet_get_node_count()===0,'Foreign Edge boundary graph leaked');cases.push(label+' error');
  });
 }
 return {status:'passed',cases,nodesAfter:0};
}
