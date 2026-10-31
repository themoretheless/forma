/** @vitest-environment jsdom */
import {afterEach,beforeEach,expect,it,vi} from 'vitest';
import {createRoot,getGraphTelemetry,getWasmCore,h,onCleanup,renderToString,resetReactiveSystem,signal,type Signal} from '../src/engine';
import {renderToStringAsync} from '../src/engine/server';
import * as wasmBridge from '../src/engine/wasm-bridge';
beforeEach(()=>resetReactiveSystem());
afterEach(()=>vi.restoreAllMocks());
function gate(){let release!:()=>void;const promise=new Promise<void>(resolve=>{release=resolve;});return {promise,release};}

it('sync SSR rejects reset during view evaluation and closes the isolated graph',()=>{
 let core!:ReturnType<typeof getWasmCore>;
 expect(()=>renderToString(()=>{core=getWasmCore();resetReactiveSystem();signal(7);return h('p',null,'stale');})).toThrow('reset');
 expect(core.svet_get_node_count()).toBe(0);expect(getGraphTelemetry().nodes).toBe(0);
});

it('sync SSR rejects a disposed caller before invoking its view',()=>{
 let calls=0;createRoot(dispose=>{dispose();expect(()=>renderToString(()=>{calls++;return h('p',null,'stale');})).toThrow('disposed');});expect(calls).toBe(0);expect(getGraphTelemetry().nodes).toBe(0);
});

it('sync SSR rejects caller teardown from inside its view',()=>{
 let stop!:()=>void;createRoot(dispose=>{stop=dispose;expect(()=>renderToString(()=>{stop();return h('p',null,'stale');})).toThrow('disposed');});expect(getGraphTelemetry().nodes).toBe(0);
});

it('sync SSR rejects reset during DOM serialization',()=>{
 const input=document.createElement('input');input.value='data';const descriptor=Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,'value')!;let core!:ReturnType<typeof getWasmCore>;
 Object.defineProperty(input,'value',{configurable:true,get(){resetReactiveSystem();return descriptor.get!.call(this);}});
 expect(()=>renderToString(()=>{core=getWasmCore();return input;})).toThrow('reset');delete (input as any).value;expect(core.svet_get_node_count()).toBe(0);expect(getGraphTelemetry().nodes).toBe(0);
});

it('sync SSR rejects reset from snapshot JSON serialization',()=>{
 const named:Record<string,Signal<any>>={};let core!:ReturnType<typeof getWasmCore>;
 expect(()=>renderToString(()=>{core=getWasmCore();named.value=signal({toJSON(){resetReactiveSystem();return 'stale';}});return h('p',null,'html');},named)).toThrow('reset');
 expect(core.svet_get_node_count()).toBe(0);expect(getGraphTelemetry().nodes).toBe(0);
});

it('sync SSR retires bare request signals after successful publication preparation',()=>{
 let retained!:Signal<number>,core!:ReturnType<typeof getWasmCore>;const named:Record<string,Signal<any>>={};
 const result=renderToString(()=>{core=getWasmCore();retained=signal(1);named.value=retained;return h('p',null,retained);},named);
 expect(result.html).toContain('<p>1</p>');expect(JSON.parse(result.stateJson).signals.value).toBe(1);expect(core.svet_get_node_count()).toBe(0);retained.set(2);expect(retained()).toBe(1);expect(getGraphTelemetry().nodes).toBe(0);
});

it('sync SSR permits its own teardown reset after output is fully serialized',()=>{
 let core!:ReturnType<typeof getWasmCore>;const result=renderToString(()=>{core=getWasmCore();onCleanup(()=>resetReactiveSystem());return h('p',null,'complete');});expect(result.html).toBe('<p>complete</p>');expect(core.svet_get_node_count()).toBe(0);
});

it('async SSR rejects a view that resets its graph after await',async()=>{
 let core!:ReturnType<typeof getWasmCore>;await expect(renderToStringAsync(async()=>{await Promise.resolve();core=getWasmCore();resetReactiveSystem();signal(7);return {node:h('p',null,'stale')};})).rejects.toThrow('reset');expect(core.svet_get_node_count()).toBe(0);expect(getGraphTelemetry().nodes).toBe(0);
});

it('async SSR snapshots metadata once and rejects reset from a signals getter',async()=>{
 let reads=0;await expect(renderToStringAsync(async()=>({node:h('p',null,'html'),get signals(){reads++;resetReactiveSystem();return {};}}))).rejects.toThrow('reset');expect(reads).toBe(1);expect(getGraphTelemetry().nodes).toBe(0);
});

it('async SSR rejects a caller disposed while the view is awaiting',async()=>{
 const wait=gate();let stop!:()=>void,pending!:ReturnType<typeof renderToStringAsync>;
 createRoot(dispose=>{stop=dispose;pending=renderToStringAsync(async()=>{await wait.promise;return {node:h('p',null,'stale')};});});stop();wait.release();await expect(pending).rejects.toThrow('disposed');expect(getGraphTelemetry().nodes).toBe(0);
});

it('async SSR rechecks the caller after request cleanup',async()=>{
 let stop!:()=>void,pending!:ReturnType<typeof renderToStringAsync>;
 createRoot(dispose=>{stop=dispose;pending=renderToStringAsync(async()=>{onCleanup(()=>stop());return {node:h('p',null,'stale')};});});await expect(pending).rejects.toThrow('disposed');expect(getGraphTelemetry().nodes).toBe(0);
});

it('sync request finalization leaves the caller graph and live signals untouched',()=>{
 const caller=signal(5);renderToString(()=>{signal(99);return h('p',null,'request');});caller.set(6);expect(caller()).toBe(6);expect(getGraphTelemetry().nodes).toBe(1);caller.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('sync view failure remains primary when request cleanup also resets its graph',()=>{
 const failure=Error('view failure');expect(()=>renderToString(()=>{onCleanup(()=>resetReactiveSystem());throw failure;})).toThrow(failure);expect(getGraphTelemetry().nodes).toBe(0);
});

it('async disposed-caller preflight always returns a rejected promise without invoking the view',async()=>{
 let pending!:ReturnType<typeof renderToStringAsync>,calls=0;
 createRoot(dispose=>{dispose();expect(()=>{pending=renderToStringAsync(async()=>{calls++;return {node:h('p',null,'stale')};});}).not.toThrow();});
 await expect(pending).rejects.toThrow('disposed');expect(calls).toBe(0);expect(getGraphTelemetry().nodes).toBe(0);
});

it('async SSR reads successful signal metadata only once',async()=>{
 let reads=0;const result=await renderToStringAsync(async()=>{const value=signal(2);return {node:h('p',null,'html'),get signals(){reads++;return {value};}};});
 expect(reads).toBe(1);expect(JSON.parse(result.stateJson).signals.value).toBe(2);expect(getGraphTelemetry().nodes).toBe(0);
});


it('async SSR permits its own teardown reset after output is fully serialized',async()=>{
 let core!:ReturnType<typeof getWasmCore>;const result=await renderToStringAsync(async()=>{core=getWasmCore();const value=signal(2);onCleanup(()=>resetReactiveSystem());return {node:h('p',null,value),signals:{value}};});
 expect(result.html).toContain('<p>2</p>');expect(JSON.parse(result.stateJson).signals.value).toBe(2);expect(core.svet_get_node_count()).toBe(0);expect(getGraphTelemetry().nodes).toBe(0);
});


it('caller teardown promptly cancels an active async render even if its view keeps awaiting',async()=>{
 const wait=gate(),started=gate();let stop!:()=>void,pending!:ReturnType<typeof renderToStringAsync>,core!:ReturnType<typeof getWasmCore>;
 createRoot(dispose=>{stop=dispose;pending=renderToStringAsync(async()=>{core=getWasmCore();signal(7);started.release();await wait.promise;return {node:h('p',null,'late')};});});
 await started.promise;stop();await expect(pending).rejects.toThrow('disposed');expect(core.svet_get_node_count()).toBe(0);expect(getGraphTelemetry().nodes).toBe(0);wait.release();await Promise.resolve();await Promise.resolve();
});

it('an explicit abort signal remains effective for caller-owned async renders',async()=>{
 const wait=gate(),started=gate(),controller=new AbortController();let stop!:()=>void,pending!:ReturnType<typeof renderToStringAsync>;
 createRoot(dispose=>{stop=dispose;pending=renderToStringAsync(async()=>{started.release();await wait.promise;return {node:h('p',null,'late')};},{signal:controller.signal});});
 await started.promise;const reason=Error('explicit disconnect');controller.abort(reason);await expect(pending).rejects.toBe(reason);stop();expect(getGraphTelemetry().nodes).toBe(0);wait.release();await Promise.resolve();await Promise.resolve();
});


it('request-core construction failure rejects asynchronously and unlinks the caller cancellation hook',async()=>{
 getWasmCore();const failure=Error('request instance failed'),abort=vi.spyOn(AbortController.prototype,'abort');
 vi.spyOn(wasmBridge,'createWasmCoreInstance').mockImplementationOnce(()=>{throw failure;});let stop!:()=>void,pending!:ReturnType<typeof renderToStringAsync>,calls=0;
 createRoot(dispose=>{stop=dispose;expect(()=>{pending=renderToStringAsync(async()=>{calls++;return {node:h('p',null,'never')};});}).not.toThrow();});
 await expect(pending).rejects.toBe(failure);stop();expect(abort).not.toHaveBeenCalled();expect(calls).toBe(0);expect(getGraphTelemetry().nodes).toBe(0);
});
