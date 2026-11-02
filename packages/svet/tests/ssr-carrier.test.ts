import {withWasmCore,createWasmCoreInstance} from '../src/engine/wasm-bridge';
/** @vitest-environment jsdom */
import {AsyncLocalStorage} from 'node:async_hooks';
import {randomUUID} from 'node:crypto';
import {expect,it} from 'vitest';
import {assertSsrAsyncContextCarrier,createSsrRuntime,SsrTimeoutError,type SsrAsyncContextCarrier} from '../src/engine/ssr-runtime';
import {h,signal,onCleanup,getWasmCore,effect,captureReactiveContext} from '../src/engine';
it('qualifies real native await, concurrent contexts, exit and rejection restoration',async()=>{
 await expect(assertSsrAsyncContextCarrier(()=>new AsyncLocalStorage<object>())).resolves.toBeUndefined();
});
it('rejects a synchronous stack carrier that loses context across native await',async()=>{
 const create=():SsrAsyncContextCarrier<object>=>{let value:object|undefined;return {getStore:()=>value,run(store,callback){const old=value;value=store;try{return callback();}finally{value=old;}},exit(callback){const old=value;value=undefined;try{return callback();}finally{value=old;}}};};
 await expect(assertSsrAsyncContextCarrier(create)).rejects.toThrow(/native await/);
});
it('validates independent carriers before installing hooks, then renders with an injected host',async()=>{
 expect(()=>createSsrRuntime({createAsyncContext:()=>({} as any),randomUUID})).toThrow(/carrier/);
 const shared=new AsyncLocalStorage<any>();expect(()=>createSsrRuntime({createAsyncContext:()=>shared,randomUUID})).toThrow(/independent/);
 const requestTokens:Array<{closed?:boolean;core?:unknown}>=[],renderTokens:any[]=[],ownerTokens:any[]=[];let carrierIndex=0,ownerRunFailure:unknown;
 const runtime=createSsrRuntime({createAsyncContext:<T>()=>{
  const index=carrierIndex++,storage=new AsyncLocalStorage<T>();
  return {getStore:()=>storage.getStore(),run<R>(value:T,callback:()=>R):R{if(index===0)requestTokens.push(value as any);if(index===2)renderTokens.push(value);if(index===1){ownerTokens.push(value);if(ownerRunFailure)throw ownerRunFailure;}return storage.run(value,callback);},exit<R>(callback:()=>R):R{return storage.exit(callback);}};
 },randomUUID});
 const cores=new Set();let cleaned=0;
 const results=await Promise.all(Array.from({length:4},(_,i)=>runtime.renderToStringAsync(()=>{
  cores.add(getWasmCore());return {node:h(async()=>{await Promise.resolve();const value=signal(i);onCleanup(()=>{value.dispose();cleaned++;});await Promise.resolve();return h('p',null,String(value()));},null)};
 })));
 expect(results.map(result=>result.html)).toEqual(['<p>0</p>','<p>1</p>','<p>2</p>','<p>3</p>']);expect(cores.size).toBe(4);expect(cleaned).toBe(4);
 const reason=Error('retired request failure');await expect(runtime.runWithIsolatedContextAsync(()=>{throw reason;})).rejects.toBe(reason);
 let resume!:()=>void,late!:Promise<unknown>;
 await runtime.runWithIsolatedContextAsync(()=>{late=new Promise<void>(resolve=>resume=resolve).then(()=>getWasmCore());return 1;});
 const lateFailure=expect(late).rejects.toThrow(/already completed/);resume();await lateFailure;
 const reader=runtime.renderToReadableStreamAsync(()=>({node:h('p',null,'stream')})).getReader();
 while(!(await reader.read()).done){}await reader.closed;
 await expect(runtime.renderToStringAsync(()=>new Promise(()=>{}),{timeoutMs:5})).rejects.toBeInstanceOf(SsrTimeoutError);
 const timedView=runtime.renderToReadableStreamAsync(()=>new Promise(()=>{}),{timeoutMs:5}).getReader();
 const closedFailure=timedView.closed.catch(error=>error);
 await expect(timedView.read()).rejects.toBeInstanceOf(SsrTimeoutError);expect(await closedFailure).toBeInstanceOf(SsrTimeoutError);
 let releaseAfterParent!:()=>void,afterParent!:Promise<void>;
 const afterParentGate=new Promise<void>(resolve=>releaseAfterParent=resolve);
 await runtime.runWithIsolatedContextAsync(async()=>{
  const parentCore=getWasmCore(),foreignCore=createWasmCoreInstance();
  const source=signal(1),captured=captureReactiveContext();onCleanup(source.dispose);let observed=0,release!:()=>void,lateChild!:Promise<void>;
  const gate=new Promise<void>(resolve=>release=resolve);
  await runtime.runWithOwnerScopeAsync([],()=>{effect(()=>{observed=source();});
   afterParent=afterParentGate.then(()=>{
    expect(()=>withWasmCore(parentCore,()=>h('b',null,'expired'))).toThrow(/completed|disposed/);
    withWasmCore(foreignCore,()=>expect(h('b',null,'foreign').textContent).toBe('foreign'));
   });
   lateChild=gate.then(()=>{
   expect(()=>getWasmCore()).toThrow(/already completed/);
   captured.run(()=>expect(h('b',null,'parent').textContent).toBe('parent'));
  });});
  source.set(2);expect(observed).toBe(2);release();await lateChild;captured.dispose();
 });
 releaseAfterParent();await afterParent;
 const setupFailure=Error('owner carrier setup');ownerRunFailure=setupFailure;let calls=0;
 await runtime.runWithIsolatedContextAsync(async()=>{
  await expect(runtime.runWithOwnerScopeAsync([],()=>calls++)).rejects.toBe(setupFailure);
  expect(h('b',null,'parent still alive').textContent).toBe('parent still alive');
 });ownerRunFailure=undefined;expect(calls).toBe(0);
 expect(ownerTokens).toHaveLength(6);
 expect(ownerTokens.every(token=>token.closed&&token.core===undefined&&token.owner===undefined)).toBe(true);
 expect(renderTokens).toHaveLength(7);
 expect(renderTokens.every(token=>token.closed&&token.stream===undefined&&token.pending.size===0&&token.failure===undefined)).toBe(true);
 expect(requestTokens).toHaveLength(11);
 expect(requestTokens.every(token=>token.closed===true&&token.core===undefined)).toBe(true);
 expect(()=>createSsrRuntime({createAsyncContext:<T>()=>new AsyncLocalStorage<T>(),randomUUID})).toThrow(/already installed/);
});
