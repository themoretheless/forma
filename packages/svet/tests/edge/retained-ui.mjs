import * as a from 'svet-engine';
import * as b from 'svet-engine-copy';
const check=(value,message)=>{if(!value)throw Error(message);};
export function createRetainedUiProbe(runtime){
 const targets=[],held=[];let ran=false;
 const track=(kind,value)=>targets.push({kind,ref:new WeakRef(value)});
 return {
  async run(){
   check(!ran,'Retained UI fixture already ran');ran=true;
   for(const engine of [a,b])for(let i=0;i<4;i++)for(const kind of ['boundary','router'])await runtime.runWithIsolatedContextAsync(()=>{
    const core=a.getWasmCore();track(kind+'Core',core);track(kind+'Memory',core.memory);
    engine.withWasmCore(core,()=>{
     const container=document.createElement('section');const app=engine.mount(()=>{
      if(kind==='boundary')return engine.ErrorBoundary({children:()=>{throw Error('controlled');},fallback:(_,retry)=>{held.push(retry);return document.createTextNode('error');}});
      const router=engine.createRouter({routes:[],initialPath:'/retired?q=last#done'});held.push(router,router.path,router.navigate,router.beforeEach(()=>{}));return engine.RouterView({router});
     },container);app.unmount();check(engine.getGraphTelemetry().nodes===0,'Retained UI graph survived unmount');
    });
   });
   // Call only terminal APIs after all request carriers have closed.
   for(const handle of held)if(typeof handle==='function')handle();else{
    check(handle.path()==='/retired'&&handle.query().q==='last'&&handle.hash()==='done','Terminal router snapshot changed');
    check(handle.navigate('/late')===false,'Terminal Edge router navigated');handle.dispose();
   }
   return {status:'passed',requests:16,retainedReferences:held.length,nodesAfter:0};
  },
  inspect(){const kinds={};for(const row of targets)kinds[row.kind]=(kinds[row.kind]??0)+1;return {targets:targets.length,alive:targets.filter(row=>row.ref.deref()).length,kinds};}
 };
}
