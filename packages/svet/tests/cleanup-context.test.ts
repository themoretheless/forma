import {beforeEach,expect,it} from 'vitest';
import {createRoot,effect,getGraphTelemetry,onCleanup,resetReactiveSystem,runWithOwnerScope,signal} from '../src/engine';
import {createWasmCoreInstance,getWasmCore,installAsyncCoreResolver,withWasmCore} from '../src/engine/wasm-bridge';
beforeEach(()=>resetReactiveSystem());

it('a retained root disposer runs cleanup in its creation core and restores the calling core',()=>{
 const owned=createWasmCoreInstance(),caller=createWasmCoreInstance();let stop!:()=>void,observed:unknown;
 withWasmCore(owned,()=>createRoot(dispose=>{stop=dispose;onCleanup(()=>{observed=getWasmCore();});}));
 withWasmCore(caller,()=>{stop();expect(getWasmCore()).toBe(caller);});expect(observed).toBe(owned);
});

it('a retained effect disposer runs returned cleanup in its creation core',()=>{
 const owned=createWasmCoreInstance(),caller=createWasmCoreInstance();let stop!:()=>void,observed:unknown;
 withWasmCore(owned,()=>{stop=effect(()=>()=>{observed=getWasmCore();}).dispose;});
 withWasmCore(caller,()=>{stop();expect(getWasmCore()).toBe(caller);});expect(observed).toBe(owned);expect(owned.svet_get_node_count()).toBe(0);
});

it('root cleanup reads cannot subscribe the effect calling its disposer',()=>{
 const gate=signal(0),cleanupSource=signal(0);let stop!:()=>void,runs=0;
 createRoot(dispose=>{stop=dispose;onCleanup(()=>{cleanupSource();});});const observer=effect(()=>{gate();runs++;stop();});
 cleanupSource.set(1);expect(runs).toBe(1);gate.set(1);expect(runs).toBe(2);observer.dispose();gate.dispose();cleanupSource.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('effect cleanup reads cannot subscribe the effect calling its disposer',()=>{
 const gate=signal(0),cleanupSource=signal(0);let runs=0;
 const old=effect(()=>()=>{cleanupSource();}),observer=effect(()=>{gate();runs++;old.dispose();});
 cleanupSource.set(1);expect(runs).toBe(1);gate.set(1);expect(runs).toBe(2);observer.dispose();gate.dispose();cleanupSource.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('child scope failure cleanup cannot add dependencies to the surviving caller effect',()=>{
 const gate=signal(0),cleanupSource=signal(0),failure=Error('body failed');let runs=0;
 const observer=effect(()=>{gate();runs++;expect(()=>runWithOwnerScope([],()=>{onCleanup(()=>{cleanupSource();});throw failure;})).toThrow(failure);});
 cleanupSource.set(1);expect(runs).toBe(1);observer.dispose();gate.dispose();cleanupSource.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('throwing returned effect cleanup preserves its error and restores the caller core',()=>{
 const owned=createWasmCoreInstance(),caller=createWasmCoreInstance(),failure=Error('cleanup failed');let stop!:()=>void,observed:unknown;
 withWasmCore(owned,()=>{stop=effect(()=>()=>{observed=getWasmCore();throw failure;}).dispose;});
 withWasmCore(caller,()=>{expect(stop).toThrow(failure);expect(getWasmCore()).toBe(caller);});expect(observed).toBe(owned);expect(owned.svet_get_node_count()).toBe(0);expect(()=>stop()).not.toThrow();
});

it('root construction preserves the original failure and completes cleanup despite cleanup exceptions',()=>{
 const failure=Error('body failed'),calls:string[]=[];
 expect(()=>createRoot(()=>{onCleanup(()=>{calls.push('first');});onCleanup(()=>{calls.push('last');throw Error('cleanup failed');});throw failure;})).toThrow(failure);
 expect(calls).toEqual(['last','first']);expect(getGraphTelemetry().nodes).toBe(0);
});

it('root cleanup reset affects only its captured graph and preserves the dispatching core',()=>{
 const owned=createWasmCoreInstance(),caller=createWasmCoreInstance();let stop!:()=>void;
 withWasmCore(owned,()=>createRoot(dispose=>{stop=dispose;onCleanup(resetReactiveSystem);}));
 withWasmCore(caller,()=>{const live=signal(17);stop();expect(live()).toBe(17);live.set(18);expect(live()).toBe(18);expect(getGraphTelemetry().nodes).toBe(1);live.dispose();});
});

it.each(['root','effect'])('%s cleanup writes still invalidate dependencies already read by its caller',kind=>{
 const source=signal(0),values:number[]=[];let stop!:()=>void;
 if(kind==='root')createRoot(dispose=>{stop=dispose;onCleanup(()=>source.set(1));});
 else stop=effect(()=>()=>source.set(1)).dispose;
 const observer=effect(()=>{values.push(source());stop();});expect(values).toEqual([0,1]);observer.dispose();source.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('reset in returned effect cleanup leaves the calling core graph live',()=>{
 const owned=createWasmCoreInstance(),caller=createWasmCoreInstance();let stop!:()=>void;
 withWasmCore(owned,()=>{stop=effect(()=>()=>resetReactiveSystem()).dispose;});
 withWasmCore(caller,()=>{const live=signal(17);stop();live.set(18);expect(live()).toBe(18);expect(getGraphTelemetry().nodes).toBe(1);live.dispose();});
});

it.each(['root','effect'])('foreign %s cleanup cannot subscribe its calling effect through caller-core sources',kind=>{
 const owned=createWasmCoreInstance(),caller=createWasmCoreInstance();let stop!:()=>void,read!:ReturnType<typeof signal<number>>;
 withWasmCore(caller,()=>{read=signal(0);});
 withWasmCore(owned,()=>{if(kind==='root')createRoot(dispose=>{stop=dispose;onCleanup(()=>{read();});});else stop=effect(()=>()=>{read();}).dispose;});
 withWasmCore(caller,()=>{const gate=signal(0);let runs=0;const observer=effect(()=>{gate();runs++;stop();});read.set(1);expect(runs).toBe(1);observer.dispose();gate.dispose();read.dispose();});
});

it('captured disposal still runs after the calling core resolver starts rejecting work',()=>{
 const owned=createWasmCoreInstance();let stop!:()=>void,cleaned=0;
 withWasmCore(owned,()=>createRoot(dispose=>{stop=dispose;onCleanup(()=>{expect(getWasmCore()).toBe(owned);cleaned++;});}));
 installAsyncCoreResolver(()=>{throw Error('request closed');});
 try{expect(stop).not.toThrow();expect(cleaned).toBe(1);}finally{installAsyncCoreResolver(()=>undefined);}
});
