import {beforeEach,expect,it} from 'vitest';
import {computed,createRoot,effect,getCurrentOwner,getGraphTelemetry,resetReactiveSystem,signal} from '../src/engine';
import {createWasmCoreInstance,getWasmCore,withWasmCore} from '../src/engine/wasm-bridge';
beforeEach(()=>resetReactiveSystem());
type Registrar=(cleanup:()=>void)=>void;

it('invalid effect bodies reject before native allocation or owner registration',()=>{
 let allocations=0;const native=createWasmCoreInstance(),wrapped={...native,svet_create_node:(kind:number)=>{allocations++;return native.svet_create_node(kind);}};
 withWasmCore(wrapped,()=>createRoot(stop=>{const owner=getCurrentOwner()!,core=getWasmCore(),before=core.svet_get_node_count(),hooks=owner.disposables.length,children=owner.children?.size??0;
  for(const value of [false,0,null,undefined,{}]){expect(()=>effect(value as never)).toThrow(TypeError);expect(core.svet_get_node_count()).toBe(before);expect(owner.disposables).toHaveLength(hooks);expect(owner.children?.size??0).toBe(children);}stop();
 }));expect(allocations).toBe(0);
});

it('multiple registrations and a returned cleanup run once each in reverse order',()=>{
 const events:string[]=[],handle=effect(register=>{register(()=>events.push('first'));register(()=>events.push('second'));return()=>events.push('returned');});handle.dispose();handle.dispose();expect(events).toEqual(['returned','second','first']);expect(getGraphTelemetry().nodes).toBe(0);
});

it('each rerun retires all registrations from its previous invocation',()=>{
 const source=signal(0),events:string[]=[],handle=effect(register=>{const value=source();register(()=>events.push(value+':first'));register(()=>events.push(value+':second'));return()=>events.push(value+':returned');});
 source.set(1);expect(events).toEqual(['0:returned','0:second','0:first']);handle.dispose();expect(events).toEqual(['0:returned','0:second','0:first','1:returned','1:second','1:first']);source.dispose();
});

it('all cleanup callbacks run even when several throw and the first cleanup failure stays primary',()=>{
 const first=Error('returned failed'),second=Error('registered failed'),events:string[]=[],handle=effect(register=>{register(()=>events.push('first'));register(()=>{events.push('second');throw second;});return()=>{events.push('returned');throw first;};});
 expect(()=>handle.dispose()).toThrow(first);expect(events).toEqual(['returned','second','first']);expect(getGraphTelemetry().nodes).toBe(0);expect(()=>handle.dispose()).not.toThrow();
});

it('a late disposed registrar executes in its captured core without subscribing its caller',()=>{
 const owned=createWasmCoreInstance(),caller=createWasmCoreInstance();let register!:Registrar,seen:unknown;
 withWasmCore(owned,()=>{const handle=effect(cleanup=>{register=cleanup;});handle.dispose();});
 withWasmCore(caller,()=>{const gate=signal(0),hidden=signal(0);let runs=0;const observer=effect(()=>{gate();runs++;register(()=>{seen=getWasmCore();hidden();});});hidden.set(1);expect(runs).toBe(1);expect(seen).toBe(owned);gate.set(1);expect(runs).toBe(2);expect(getWasmCore()).toBe(caller);observer.dispose();gate.dispose();hidden.dispose();});
});

it('an old invocation registrar runs late cleanup immediately without replacing the current cleanup',()=>{
 const owned=createWasmCoreInstance(),caller=createWasmCoreInstance(),events:number[]=[],cores:unknown[]=[];let register!:Registrar,source!:ReturnType<typeof signal<number>>,handle!:ReturnType<typeof effect>;
 withWasmCore(owned,()=>{source=signal(0);handle=effect(cleanup=>{const value=source();if(value===0)register=cleanup;cleanup(()=>events.push(value));});source.set(1);});
 withWasmCore(caller,()=>register(()=>{events.push(9);cores.push(getWasmCore());}));handle.dispose();expect(events).toEqual([0,9,1]);expect(cores).toEqual([owned]);source.dispose();
});

it('invalid cleanup arguments preserve already registered cleanup during failed initialization',()=>{
 const events:string[]=[];
 expect(()=>effect(register=>{register(()=>events.push('valid'));register(1 as never);})).toThrow(TypeError);expect(events).toEqual(['valid']);expect(getGraphTelemetry().nodes).toBe(0);
});

it('ten thousand cleanup callbacks drain iteratively without recursive composition',()=>{
 let count=0,total=0;const handle=effect(register=>{for(let i=0;i<10000;i++)register(()=>{count++;total+=i;});});expect(()=>handle.dispose()).not.toThrow();expect(count).toBe(10000);expect(total).toBe(49995000);expect(getGraphTelemetry().nodes).toBe(0);
});

it('reentrant late registration while draining cleanup runs exactly once',()=>{
 const events:string[]=[];const handle=effect(register=>{register(()=>events.push('first'));register(()=>{events.push('second');register(()=>events.push('late'));});});handle.dispose();expect(events).toEqual(['second','late','first']);
});

it('undefined thrown as the first cleanup error is retained while remaining callbacks finish',()=>{
 const events:string[]=[],handle=effect(register=>{register(()=>events.push('remaining'));return()=>{throw undefined;};});let failed=false;
 try{handle.dispose();}catch(error){failed=true;expect(error).toBeUndefined();}expect(failed).toBe(true);expect(events).toEqual(['remaining']);expect(getGraphTelemetry().nodes).toBe(0);
});

it('a queued effect can retry after a cleanup failure without repeating already drained callbacks',()=>{
 const source=signal(0),failure=Error('cleanup failed'),events:string[]=[],seen:number[]=[];
 const first=effect(register=>{const value=source();seen.push(value);register(()=>events.push(value+':first'));return()=>{events.push(value+':returned');if(value===0)throw failure;};});
 const second=effect(()=>{source();});
 expect(()=>source.set(1)).toThrow(failure);expect(events).toEqual(['0:returned','0:first']);source.set(2);expect(seen).toEqual([0,2]);first.dispose();second.dispose();expect(events).toEqual(['0:returned','0:first','2:returned','2:first']);source.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('cleanup failure also permits retry through dirty computed ancestors',()=>{
 const source=signal(0),value=computed(()=>source()*2),failure=Error('cleanup failed'),seen:number[]=[];
 const first=effect(()=>{const current=value();seen.push(current);return()=>{if(current===0)throw failure;};}),second=effect(()=>{source();});
 expect(()=>source.set(1)).toThrow(failure);source.set(2);expect(seen).toEqual([0,4]);first.dispose();second.dispose();value.dispose();source.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('recovery cannot mutate a replacement computed occupying a retired dependency slot',()=>{
 const source=signal(0),replacement=signal(17),value=computed(()=>source()),failure=Error('cleanup failed'),core=getWasmCore();let fresh!:ReturnType<typeof computed<number>>,state=-1;
 const first=effect(()=>{value();return()=>{value.dispose();fresh=computed(()=>replacement());state=core.svet_get_state(fresh.id);throw failure;};}),second=effect(()=>{source();});
 expect(()=>source.set(1)).toThrow(failure);expect(core.svet_get_state(fresh.id)).toBe(state);expect(fresh()).toBe(17);expect(()=>first.dispose()).not.toThrow();second.dispose();fresh.dispose();source.dispose();replacement.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('native cache-repair failure cannot replace the original cleanup exception',()=>{
 const native=createWasmCoreInstance(),primary=Error('cleanup primary'),wrapped={...native,svet_depends_on:()=>{throw Error('repair failed');}};
 withWasmCore(wrapped,()=>{const source=signal(0),value=computed(()=>source()),first=effect(()=>{value();return()=>{throw primary;};}),second=effect(()=>{source();});expect(()=>source.set(1)).toThrow(primary);first.dispose();second.dispose();value.dispose();source.dispose();expect(getGraphTelemetry().nodes).toBe(0);});
});
