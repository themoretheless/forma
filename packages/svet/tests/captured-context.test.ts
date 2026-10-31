/** @vitest-environment jsdom */
import {afterEach,beforeEach,expect,it,vi} from 'vitest';
import {captureReactiveContext,createContext,createRoot,effect,getCurrentOwner,getGraphTelemetry,onCleanup,ReactiveContextDisposedError,resetReactiveSystem,runWithOwnerScope,signal,useContext,type CapturedReactiveContext,type Signal} from '../src/engine';
import {createWasmCoreInstance,getWasmCore,installAsyncCoreResolver,withWasmCore} from '../src/engine/wasm-bridge';
let stops:Array<()=>void>=[],sources:Array<Signal<any>>=[];
beforeEach(()=>{resetReactiveSystem();document.body.replaceChildren();});
afterEach(()=>{installAsyncCoreResolver(()=>undefined);for(const stop of stops.reverse())stop();for(const source of sources)source.dispose();stops=[];sources=[];vi.restoreAllMocks();});
it('reenters its provider after await and owns newly created observers',async()=>{
 const provider=createContext('default'),source=signal(0);sources.push(source);let context!:CapturedReactiveContext,dispose!:()=>void,runs=0,cleaned=0;
 createRoot(stop=>{dispose=stop;stops.push(stop);provider.Provider({value:'bound',children:()=>{context=captureReactiveContext();return document.createTextNode('owner');}});});await Promise.resolve();expect(context.run(()=>{expect(useContext(provider)).toBe('bound');effect(()=>{source();runs++;});onCleanup(()=>{cleaned++;});return 17;})).toBe(17);expect(useContext(provider)).toBe('default');dispose();source.set(1);expect([runs,cleaned,context.active]).toEqual([1,1,false]);
});
it('rejects use after owner disposal without invoking the continuation',()=>{
 let context!:CapturedReactiveContext;createRoot(stop=>{stops.push(stop);context=captureReactiveContext();stop();});const callback=vi.fn();expect(context.active).toBe(false);expect(()=>context.run(callback)).toThrow(ReactiveContextDisposedError);expect(callback).not.toHaveBeenCalled();
});
it('a context captured by an effect never revives on the next invocation',()=>{
 const trigger=signal(0);sources.push(trigger);const contexts:CapturedReactiveContext[]=[];createRoot(stop=>{stops.push(stop);effect(()=>{trigger();contexts.push(captureReactiveContext());});});trigger.set(1);expect(contexts.map(context=>context.active)).toEqual([false,true]);expect(()=>contexts[0].run(()=>{})).toThrow(ReactiveContextDisposedError);expect(contexts[1].run(()=>19)).toBe(19);
});
it('graph reset rejects the old context and preserves replacement graph state',()=>{
 let context!:CapturedReactiveContext;createRoot(stop=>{stops.push(stop);context=captureReactiveContext();});resetReactiveSystem();const fresh=signal(17);sources.push(fresh);expect(context.active).toBe(false);expect(()=>context.run(()=>fresh.set(0))).toThrow(ReactiveContextDisposedError);expect(fresh()).toBe(17);context.dispose();
});
it('manual disposal releases its cleanup hook without disposing the owner',()=>{
 createRoot(stop=>{stops.push(stop);const owner=getCurrentOwner()!,before=owner.disposables.length,context=captureReactiveContext();expect(owner.disposables.length).toBe(before+1);context.dispose();context.dispose();expect(context.active).toBe(false);expect(owner.disposables.length).toBe(before);expect(()=>context.run(()=>{})).toThrow(ReactiveContextDisposedError);expect(owner.disposed).toBe(false);});
});
it('requires an owned lifetime and rejects already stopping owners',()=>{
 expect(()=>captureReactiveContext()).toThrow(/active owner/);createRoot(stop=>{stops.push(stop);stop();expect(()=>captureReactiveContext()).toThrow(ReactiveContextDisposedError);});expect(getGraphTelemetry().nodes).toBe(0);
});
it('continuations are untracked while their explicit observers still track',()=>{
 const source=signal(0),trigger=signal(0);sources.push(source,trigger);let context!:CapturedReactiveContext,callerRuns=0,ownedRuns=0;createRoot(stop=>{stops.push(stop);context=captureReactiveContext();});createRoot(stop=>{stops.push(stop);effect(()=>{trigger();callerRuns++;context.run(()=>source());});});context.run(()=>effect(()=>{source();ownedRuns++;}));source.set(1);expect([callerRuns,ownedRuns]).toEqual([1,2]);trigger.set(1);expect(callerRuns).toBe(2);
});
it('foreign-core reentry restores core and caller tracking',()=>{
 const owned=getWasmCore(),other=createWasmCoreInstance();let context!:CapturedReactiveContext,otherSource!:Signal<number>,runs=0;createRoot(stop=>{stops.push(stop);context=captureReactiveContext();});withWasmCore(other,()=>{otherSource=signal(0);sources.push(otherSource);createRoot(stop=>{stops.push(stop);effect(()=>{runs++;context.run(()=>{expect(getWasmCore()).toBe(owned);otherSource();});});});expect(getWasmCore()).toBe(other);});otherSource.set(1);expect(runs).toBe(1);
});
it('callback exceptions restore the caller owner and context',()=>{
 const provider=createContext('default'),failure=Error('continuation failed');let context!:CapturedReactiveContext;createRoot(stop=>{stops.push(stop);provider.Provider({value:'bound',children:()=>{context=captureReactiveContext();return document.createTextNode('owner');}});});createRoot(stop=>{stops.push(stop);provider.Provider({value:'caller',children:()=>{const owner=getCurrentOwner();expect(()=>context.run(()=>{expect(useContext(provider)).toBe('bound');throw failure;})).toThrow(failure);expect(getCurrentOwner()).toBe(owner);expect(useContext(provider)).toBe('caller');return document.createTextNode('caller');}});});expect(context.active).toBe(true);
});
it('nested contexts restore each provider in turn',()=>{
 const provider=createContext('default'),values:string[]=[];let a!:CapturedReactiveContext,b!:CapturedReactiveContext;createRoot(stop=>{stops.push(stop);provider.Provider({value:'a',children:()=>{a=captureReactiveContext();return document.createTextNode('a');}});provider.Provider({value:'b',children:()=>{b=captureReactiveContext();return document.createTextNode('b');}});});a.run(()=>{values.push(useContext(provider));b.run(()=>values.push(useContext(provider)));values.push(useContext(provider));});expect(values).toEqual(['a','b','a']);expect(getCurrentOwner()).toBeNull();
});
it('rechecks lifetime after an ambient core resolver disposes the owner',()=>{
 let context!:CapturedReactiveContext,dispose!:()=>void;createRoot(stop=>{dispose=stop;stops.push(stop);context=captureReactiveContext();});const callback=vi.fn();let once=true;installAsyncCoreResolver(()=>{if(once){once=false;dispose();}return undefined;});try{expect(()=>context.run(callback)).toThrow(ReactiveContextDisposedError);expect(callback).not.toHaveBeenCalled();}finally{installAsyncCoreResolver(()=>undefined);}
});
it('captured reentry is independent of a closed ambient request resolver',()=>{
 const owned=getWasmCore();let context!:CapturedReactiveContext;createRoot(stop=>{stops.push(stop);context=captureReactiveContext();});installAsyncCoreResolver(()=>{throw Error('request closed');});try{expect(context.run(()=>getWasmCore())).toBe(owned);}finally{installAsyncCoreResolver(()=>undefined);}
});
it('marks contexts inactive throughout rerun cleanup before their own cleanup hook executes',()=>{
 const trigger=signal(0),observed:boolean[]=[];sources.push(trigger);let current!:CapturedReactiveContext;createRoot(stop=>{stops.push(stop);effect(()=>{trigger();runWithOwnerScope([],()=>{onCleanup(()=>{observed.push(current.active);expect(()=>current.run(()=>{})).toThrow(ReactiveContextDisposedError);});});current=captureReactiveContext();});});trigger.set(1);expect(observed).toEqual([false]);expect(current.active).toBe(true);
});
it('rejects invalid callbacks without changing the active context',()=>{
 createRoot(stop=>{stops.push(stop);const context=captureReactiveContext();expect(()=>context.run(null as unknown as ()=>void)).toThrow(TypeError);expect(context.active).toBe(true);context.dispose();expect(()=>context.run(null as unknown as ()=>void)).toThrow(ReactiveContextDisposedError);});
});
it('parallel continuations keep distinct providers when completed in reverse order',async()=>{
 const provider=createContext('default'),contexts:CapturedReactiveContext[]=[];for(const value of ['a','b'])createRoot(stop=>{stops.push(stop);provider.Provider({value,children:()=>{contexts.push(captureReactiveContext());return document.createTextNode(value);}});});let resolveA!:()=>void,resolveB!:()=>void;const a=new Promise<void>(resolve=>{resolveA=resolve;}),b=new Promise<void>(resolve=>{resolveB=resolve;});const jobs=[(async()=>{await a;return contexts[0].run(()=>useContext(provider));})(),(async()=>{await b;return contexts[1].run(()=>useContext(provider));})()];resolveB();await Promise.resolve();expect(useContext(provider)).toBe('default');resolveA();expect(await Promise.all(jobs)).toEqual(['a','b']);expect(getCurrentOwner()).toBeNull();
});
it('canceling one awaited owner leaves an independent continuation live',async()=>{
 const provider=createContext('default'),contexts:CapturedReactiveContext[]=[];let stopA!:()=>void;for(const value of ['a','b'])createRoot(stop=>{if(value==='a')stopA=stop;stops.push(stop);provider.Provider({value,children:()=>{contexts.push(captureReactiveContext());return document.createTextNode(value);}});});let release!:()=>void;const gate=new Promise<void>(resolve=>{release=resolve;}),callback=vi.fn();const pending=(async()=>{await gate;return contexts[0].run(callback);})();const results=Promise.allSettled([pending,Promise.resolve().then(()=>contexts[1].run(()=>useContext(provider)))]);stopA();release();const settled=await results;expect(settled[0].status).toBe('rejected');if(settled[0].status==='rejected')expect(settled[0].reason).toBeInstanceOf(ReactiveContextDisposedError);expect(settled[1]).toEqual({status:'fulfilled',value:'b'});expect(callback).not.toHaveBeenCalled();
});
it('a callback returning a promise does not retain global owner context across its await',async()=>{
 const provider=createContext('default'),values:string[]=[];let context!:CapturedReactiveContext;createRoot(stop=>{stops.push(stop);provider.Provider({value:'bound',children:()=>{context=captureReactiveContext();return document.createTextNode('owner');}});});const pending=context.run(async()=>{values.push(useContext(provider));await Promise.resolve();values.push(useContext(provider));});expect(getCurrentOwner()).toBeNull();expect(useContext(provider)).toBe('default');await pending;expect(values).toEqual(['bound','default']);expect(context.active).toBe(true);
});
it('disposing a capture does not dispose view resources created before a callback error',()=>{
 const source=signal(0),failure=Error('owned callback failed');sources.push(source);let context!:CapturedReactiveContext,dispose!:()=>void,runs=0;createRoot(stop=>{dispose=stop;stops.push(stop);context=captureReactiveContext();});expect(()=>context.run(()=>{effect(()=>{source();runs++;});throw failure;})).toThrow(failure);context.dispose();source.set(1);expect(runs).toBe(2);dispose();source.set(2);expect(runs).toBe(2);
});
it('generation changes inside ambient resolution reject continuation before touching fresh state',()=>{
 let context!:CapturedReactiveContext,fresh!:Signal<number>,once=true;createRoot(stop=>{stops.push(stop);context=captureReactiveContext();});installAsyncCoreResolver(()=>{if(once){once=false;resetReactiveSystem();fresh=signal(17);sources.push(fresh);}return undefined;});try{expect(()=>context.run(()=>fresh.set(0))).toThrow(ReactiveContextDisposedError);expect(fresh()).toBe(17);}finally{installAsyncCoreResolver(()=>undefined);context.dispose();}
});
it('owner disposal during run leaves an explicitly independent root live',()=>{
 const source=signal(0),values:number[]=[];sources.push(source);let context!:CapturedReactiveContext,dispose!:()=>void;createRoot(stop=>{dispose=stop;stops.push(stop);context=captureReactiveContext();});expect(context.run(()=>{dispose();createRoot(stop=>{stops.push(stop);effect(()=>{values.push(source());});});return 17;})).toBe(17);context.dispose();source.set(1);expect(context.active).toBe(false);expect(values).toEqual([0,1]);
});
it('disposal of a foreign dispatching owner cannot attach subsequent reads to retired observers',()=>{
 const other=createWasmCoreInstance();let context!:CapturedReactiveContext,disposeCaller!:()=>void,source!:Signal<number>,fresh!:Signal<number>,runs=0;createRoot(stop=>{stops.push(stop);context=captureReactiveContext();});withWasmCore(other,()=>{source=signal(0);sources.push(source);createRoot(stop=>{disposeCaller=stop;stops.push(stop);effect(()=>{runs++;context.run(()=>{disposeCaller();withWasmCore(other,()=>{fresh=signal(41);sources.push(fresh);});});source();});});});source.set(1);fresh.set(42);expect(runs).toBe(1);expect(fresh()).toBe(42);
});
it('reset during run preserves fresh effects after the old capture and owner are disposed',()=>{
 const old=signal(0),values:number[]=[];sources.push(old);let context!:CapturedReactiveContext,dispose!:()=>void,fresh!:Signal<number>,observer!:ReturnType<typeof effect>;createRoot(stop=>{dispose=stop;stops.push(stop);effect(()=>{old();});context=captureReactiveContext();});expect(context.run(()=>{resetReactiveSystem();fresh=signal(17);sources.push(fresh);observer=effect(()=>{values.push(fresh());});stops.push(()=>observer.dispose());return fresh();})).toBe(17);expect(context.active).toBe(false);context.dispose();dispose();fresh.set(18);expect(values).toEqual([17,18]);
});
it('many captures share one owner cleanup registration and release without per-token array searches',()=>{
 createRoot(stop=>{stops.push(stop);const owner=getCurrentOwner()!,before=owner.disposables.length,contexts=Array.from({length:2000},()=>captureReactiveContext()),search=vi.spyOn(owner.disposables,'indexOf');expect(owner.disposables.length).toBe(before+1);for(const context of contexts)context.dispose();expect(owner.disposables.length).toBe(before);expect(search.mock.calls.length).toBeLessThanOrEqual(1);expect(contexts.every(context=>!context.active)).toBe(true);const fresh=captureReactiveContext();expect(fresh.active).toBe(true);expect(contexts[0].active).toBe(false);fresh.dispose();expect(owner.disposables.length).toBe(before);
 });
});
it('shared capture cleanup preserves user cleanup order and invalidates every handle before callbacks',()=>{
 const events:string[]=[],contexts:CapturedReactiveContext[]=[];createRoot(stop=>{onCleanup(()=>{events.push('first:'+contexts.map(context=>context.active).join(','));});contexts.push(captureReactiveContext());onCleanup(()=>{events.push('middle:'+contexts.map(context=>context.active).join(','));});contexts.push(captureReactiveContext());onCleanup(()=>{events.push('last:'+contexts.map(context=>context.active).join(','));});stop();});expect(events).toEqual(['last:false,false','middle:false,false','first:false,false']);expect(contexts.every(context=>!context.active)).toBe(true);
});
it('shared cleanup survives earlier hook removal and capture additions after partial release',()=>{
 createRoot(stop=>{stops.push(stop);const owner=getCurrentOwner()!,earlier=effect(()=>{}),a=captureReactiveContext(),b=captureReactiveContext();earlier.dispose();a.dispose();const c=captureReactiveContext();expect([a.active,b.active,c.active]).toEqual([false,true,true]);b.dispose();expect(c.run(()=>17)).toBe(17);c.dispose();expect(owner.disposables.length).toBe(0);const single=captureReactiveContext();single.dispose();expect(owner.disposables.length).toBe(0);
 });
});
it('bulk cleanup invalidates every capture before an effect owner is reused',()=>{
 const trigger=signal(0),versions:CapturedReactiveContext[][]=[];sources.push(trigger);createRoot(stop=>{stops.push(stop);effect(()=>{trigger();versions.push(Array.from({length:256},()=>captureReactiveContext()));expect(getCurrentOwner()!.disposables.length).toBe(1);});});trigger.set(1);trigger.set(2);expect(versions[0].every(context=>!context.active)).toBe(true);expect(versions[1].every(context=>!context.active)).toBe(true);expect(versions[2].every(context=>context.active)).toBe(true);
});
it('capture methods remain detachable and retain the public own-property shape',()=>{
 createRoot(stop=>{stops.push(stop);const context=captureReactiveContext(),{run,dispose}=context;expect(Object.keys(context)).toEqual(['active','run','dispose']);expect(Object.getOwnPropertyDescriptor(context,'active')?.enumerable).toBe(true);expect(run(()=>17)).toBe(17);dispose();expect(context.active).toBe(false);expect(()=>run(()=>0)).toThrow(ReactiveContextDisposedError);});
});
