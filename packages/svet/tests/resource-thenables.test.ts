/** @vitest-environment jsdom */
import {beforeEach,expect,it} from 'vitest';
import {createResource,getGraphTelemetry,resetReactiveSystem} from '../src/engine';
beforeEach(()=>resetReactiveSystem());
const tick=async()=>{for(let i=0;i<12;i++)await Promise.resolve();};
it('accepts PromiseLike fetchers and assimilates only their first settlement',async()=>{
 const source:PromiseLike<string>={then(resolve,reject){resolve?.('first');resolve?.('second');reject?.(Error('late rejection'));return Promise.resolve(undefined as never);}};
 const resource=createResource<string>(()=>source);await tick();
 const value:string|undefined=resource();expect(value).toBe('first');expect(resource.error()).toBeNull();expect(resource.loading()).toBe(false);
 resource.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});
it('handles a throwing then getter as a resource failure and permits recovery',async()=>{
 const reason=Error('then getter');let fail=true;
 const source:PromiseLike<string>={get then():PromiseLike<string>['then']{throw reason;}};
 const resource=createResource(()=>fail?source:Promise.resolve('recovered'));await tick();
 expect(resource.error()).toBe(reason);expect(resource.loading()).toBe(false);
 fail=false;expect(await resource.refetch()).toBe('recovered');expect(resource()).toBe('recovered');expect(resource.error()).toBeNull();resource.dispose();
});
it('settles cancellation independently of a thenable and ignores its late fulfillment',async()=>{
 let fulfill:((value:string)=>unknown)|undefined;
 const source:PromiseLike<string>={then(resolve){fulfill=resolve??undefined;return Promise.resolve(undefined as never);}};
 const resource=createResource<string>(()=>source);await tick();const job=resource.refetch();await tick();resource.abort();
 expect(await job).toBeUndefined();expect(resource.loading()).toBe(false);fulfill?.('late');await tick();expect(resource()).toBeUndefined();expect(resource.error()).toBeNull();resource.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});
it('refuses publication when the then getter disposes the resource',async()=>{
 let dispose=()=>{};let reenter=false;
 const source:PromiseLike<string>={get then(){if(reenter)dispose();const promise=Promise.resolve('value');return promise.then.bind(promise);}};
 const resource=createResource<string>(()=>source);dispose=resource.dispose;await tick();expect(resource()).toBe('value');reenter=true;
 expect(await resource.refetch()).toBeUndefined();expect(getGraphTelemetry().nodes).toBe(0);
});
