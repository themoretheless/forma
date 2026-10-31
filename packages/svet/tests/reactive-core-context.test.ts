import {beforeEach,expect,it} from 'vitest';
import {computed,effect,resetReactiveSystem,signal,type Signal,type ReadonlySignal} from '../src/engine';
import {createWasmCoreInstance,getWasmCore,withWasmCore} from '../src/engine/wasm-bridge';
beforeEach(()=>resetReactiveSystem());

it('foreign signal updates run updater and comparator in the source core',()=>{
 const owned=createWasmCoreInstance(),caller=createWasmCoreInstance(),seen:unknown[]=[];let source!:Signal<number>;
 withWasmCore(owned,()=>{source=signal(0,{equals:(a,b)=>{seen.push(getWasmCore());return a===b;}});});
 withWasmCore(caller,()=>{source.update(value=>{seen.push(getWasmCore());return value+1;});expect(getWasmCore()).toBe(caller);});expect(seen).toEqual([owned,owned]);expect(source()).toBe(1);source.dispose();
});

it.each([1,2])('foreign writes enter the owned core for %i direct/queued subscribers',count=>{
 const owned=createWasmCoreInstance(),caller=createWasmCoreInstance(),seen:unknown[]=[],values:number[]=[];let source!:Signal<number>;const stops:Array<()=>void>=[];
 withWasmCore(owned,()=>{source=signal(0);for(let i=0;i<count;i++)stops.push(effect(()=>{values.push(source());seen.push(getWasmCore());}).dispose);});
 withWasmCore(caller,()=>{source.set(1);expect(getWasmCore()).toBe(caller);});expect(values).toEqual([...Array(count).fill(0),...Array(count).fill(1)]);expect(seen.every(core=>core===owned)).toBe(true);for(const stop of stops)stop();source.dispose();
});

it('foreign writes drain cascades in the owned graph',()=>{
 const owned=createWasmCoreInstance(),caller=createWasmCoreInstance(),values:number[]=[],seen:unknown[]=[];let source!:Signal<number>,derived!:Signal<number>;const stops:Array<()=>void>=[];
 withWasmCore(owned,()=>{source=signal(0);derived=signal(0);stops.push(effect(()=>{const value=source();seen.push(getWasmCore());derived.set(value+1);}).dispose);stops.push(effect(()=>{values.push(derived());seen.push(getWasmCore());}).dispose);});
 withWasmCore(caller,()=>source.set(2));expect(values).toEqual([1,3]);expect(seen.every(core=>core===owned)).toBe(true);for(const stop of stops)stop();source.dispose();derived.dispose();
});

it.each(['read','peek'])('foreign computed %s evaluates and compares in its owned core',kind=>{
 const owned=createWasmCoreInstance(),caller=createWasmCoreInstance(),seen:unknown[]=[];let source!:Signal<number>,value!:ReadonlySignal<number>;
 withWasmCore(owned,()=>{source=signal(1);value=computed(()=>{seen.push(getWasmCore());return source()*2;},{equals:(a,b)=>{seen.push(getWasmCore());return a===b;}});});
 withWasmCore(caller,()=>{expect(kind==='peek'?value.peek():value()).toBe(2);source.set(2);expect(kind==='peek'?value.peek():value()).toBe(4);expect(getWasmCore()).toBe(caller);});
 expect(seen).toEqual([owned,owned,owned]);value.dispose();source.dispose();
});

it('reset in a foreign signal comparator preserves the caller graph',()=>{
 const owned=createWasmCoreInstance(),caller=createWasmCoreInstance();let source!:Signal<number>;
 withWasmCore(owned,()=>{source=signal(0,{equals:()=>{resetReactiveSystem();return false;}});});
 withWasmCore(caller,()=>{const live=signal(17);source.set(1);live.set(18);expect(live()).toBe(18);expect(source()).toBe(0);live.dispose();});
});

it('reset in a foreign computed body preserves the caller graph',()=>{
 const owned=createWasmCoreInstance(),caller=createWasmCoreInstance();let value!:ReadonlySignal<number>;
 withWasmCore(owned,()=>{value=computed(()=>{resetReactiveSystem();return 1;});});
 withWasmCore(caller,()=>{const live=signal(17);expect(value()).toBeUndefined();live.set(18);expect(live()).toBe(18);live.dispose();});
});

it('foreign updater reads do not subscribe its calling effect',()=>{
 const owned=createWasmCoreInstance(),caller=createWasmCoreInstance();let target!:Signal<number>;
 withWasmCore(owned,()=>{target=signal(0);});
 withWasmCore(caller,()=>{const gate=signal(0),foreignRead=signal(4);let runs=0;const observer=effect(()=>{gate();runs++;target.set(()=>foreignRead());});foreignRead.set(5);expect(runs).toBe(1);gate.set(1);expect(runs).toBe(2);expect(target()).toBe(5);observer.dispose();gate.dispose();foreignRead.dispose();});target.dispose();
});

it('foreign computed reads isolate the caller while tracking owned dependencies',()=>{
 const owned=createWasmCoreInstance(),caller=createWasmCoreInstance();let anchor!:Signal<number>,value!:ReadonlySignal<number>,foreign!:Signal<number>;
 withWasmCore(caller,()=>{foreign=signal(4);});withWasmCore(owned,()=>{anchor=signal(0);value=computed(()=>{anchor();return foreign();});});
 withWasmCore(caller,()=>{const gate=signal(0),values:number[]=[];const observer=effect(()=>{gate();values.push(value());});foreign.set(5);expect(values).toEqual([4]);anchor.set(1);gate.set(1);expect(values).toEqual([4,5]);observer.dispose();gate.dispose();foreign.dispose();});value.dispose();anchor.dispose();
});

it('foreign callback failures restore caller tracking after the operation',()=>{
 const owned=createWasmCoreInstance(),caller=createWasmCoreInstance(),failure=Error('updater failed');let target!:Signal<number>;
 withWasmCore(owned,()=>{target=signal(0);});
 withWasmCore(caller,()=>{const hidden=signal(0),after=signal(0);let runs=0;const observer=effect(()=>{runs++;expect(()=>target.set(()=>{hidden();throw failure;})).toThrow(failure);after();expect(getWasmCore()).toBe(caller);});hidden.set(1);expect(runs).toBe(1);after.set(1);expect(runs).toBe(2);observer.dispose();hidden.dispose();after.dispose();});target.dispose();
});

it('same-core updater/comparator tracking retains its existing behavior',()=>{
 const read=signal(0),target=signal(0,{equals:(a,b)=>{read();return a===b;}});let runs=0;
 const observer=effect(()=>{runs++;target.set(1);});read.set(1);expect(runs).toBe(2);observer.dispose();read.dispose();target.dispose();
});

it('foreign callbacks still invalidate a dependency already read by their calling effect',()=>{
 const owned=createWasmCoreInstance(),caller=createWasmCoreInstance();let target!:Signal<number>;
 withWasmCore(owned,()=>{target=signal(0);});
 withWasmCore(caller,()=>{const source=signal(0),values:number[]=[];const observer=effect(()=>{values.push(source());target.set(()=>{if(source.peek()===0)source.set(1);return 0;});});expect(values).toEqual([0,1]);observer.dispose();source.dispose();});target.dispose();
});

it('foreign computed failures restore caller tracking without collecting callback reads',()=>{
 const owned=createWasmCoreInstance(),caller=createWasmCoreInstance(),failure=Error('compute failed');let value!:ReadonlySignal<number>,hidden!:Signal<number>;
 withWasmCore(caller,()=>{hidden=signal(0);});withWasmCore(owned,()=>{value=computed(()=>{hidden();throw failure;});});
 withWasmCore(caller,()=>{const after=signal(0);let runs=0;const observer=effect(()=>{runs++;expect(value).toThrow(failure);after();expect(getWasmCore()).toBe(caller);});hidden.set(1);expect(runs).toBe(1);after.set(1);expect(runs).toBe(2);observer.dispose();after.dispose();hidden.dispose();});value.dispose();
});
