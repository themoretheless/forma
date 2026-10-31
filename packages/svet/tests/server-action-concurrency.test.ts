import {beforeEach,expect,it} from 'vitest';
import {createServerAction,effect,getGraphTelemetry,resetReactiveSystem,type ServerAction} from '../src/engine';
beforeEach(()=>resetReactiveSystem());
function deferred<T>(){let resolve!:(value:T)=>void,reject!:(reason:unknown)=>void;const promise=new Promise<T>((yes,no)=>{resolve=yes;reject=no;});return {promise,resolve,reject};}
function dispose(action:ServerAction<any,any>){action.pending.dispose();action.error.dispose();expect(getGraphTelemetry().nodes).toBe(0);}

it('pending stays true until every overlapping invocation settles',async()=>{
 const a=deferred<number>(),b=deferred<number>(),action=createServerAction<number,number>({rpc:id=>id===1?a.promise:b.promise});
 const first=action(1),second=action(2);expect(action.pending()).toBe(true);a.resolve(1);expect(await first).toBe(1);expect(action.pending()).toBe(true);
 b.resolve(2);expect(await second).toBe(2);expect(action.pending()).toBe(false);dispose(action);
});

it('an older failure cannot overwrite a newer successful invocation state',async()=>{
 const a=deferred<number>(),b=deferred<number>(),action=createServerAction<number,number>({rpc:id=>id===1?a.promise:b.promise}),failure=new Error('old failure');
 const first=action(1),second=action(2);b.resolve(2);await second;a.reject(failure);await expect(first).rejects.toBe(failure);expect(action.error()).toBeNull();expect(action.pending()).toBe(false);dispose(action);
});

it('a newer failure remains visible while an older invocation completes',async()=>{
 const a=deferred<number>(),b=deferred<number>(),action=createServerAction<number,number>({rpc:id=>id===1?a.promise:b.promise}),failure=new Error('new failure');
 const first=action(1),second=action(2);b.reject(failure);await expect(second).rejects.toBe(failure);expect(action.pending()).toBe(true);expect(action.error()).toBe(failure);
 a.resolve(1);await first;expect(action.pending()).toBe(false);expect(action.error()).toBe(failure);dispose(action);
});

it('optimistic preparation failures publish an error and finish pending without starting RPC',async()=>{
 const failure=new Error('optimistic failure');let calls=0;
 const action=createServerAction<number,number>({rpc:async()=>{calls++;return 1;},optimisticUpdate:()=>{throw failure;}});
 await expect(action(1)).rejects.toBe(failure);expect(calls).toBe(0);expect(action.pending()).toBe(false);expect(action.error()).toBe(failure);dispose(action);
});

it('RPC and rollback failures are both retained in an AggregateError',async()=>{
 const rpcFailure=new Error('rpc failed'),rollbackFailure=new Error('rollback failed'),action=createServerAction<void,void>({rpc:async()=>{throw rpcFailure;},optimisticUpdate:()=>()=>{throw rollbackFailure;}});
 let failure:unknown;try{await action();}catch(error){failure=error;}
 expect(failure).toBeInstanceOf(AggregateError);expect((failure as AggregateError).errors).toEqual([rpcFailure,rollbackFailure]);expect((failure as Error).cause).toBe(rpcFailure);expect(action.error()).toBe(failure);expect(action.pending()).toBe(false);dispose(action);
});

it('a reentrant newer invocation started by rollback owns error publication',async()=>{
 const next=deferred<number>(),failure=new Error('old failure');let newer!:Promise<number>,action!:ServerAction<number,number>;
 action=createServerAction({rpc:(id:number)=>id===1?Promise.reject(failure):next.promise,optimisticUpdate:id=>()=>{if(id===1)newer=action(2);}});
 await expect(action(1)).rejects.toBe(failure);expect(action.pending()).toBe(true);expect(action.error()).toBeNull();next.resolve(2);await newer;expect(action.pending()).toBe(false);dispose(action);
});

it('error observer failures do not mask the original RPC rejection or leave pending active',async()=>{
 const failure=new Error('rpc failure'),observerFailure=new Error('observer failure'),action=createServerAction<void,void>({rpc:async()=>{throw failure;}});
 const stop=effect(()=>{if(action.error())throw observerFailure;});
 await expect(action()).rejects.toBe(failure);expect(action.error()).toBe(failure);expect(action.pending()).toBe(false);stop.dispose();dispose(action);
});

it('pending observers may start a second invocation without losing its pending lifetime',async()=>{
 const firstResult=deferred<number>(),nextResult=deferred<number>(),action=createServerAction<number,number>({rpc:id=>id===1?firstResult.promise:nextResult.promise});let nested:Promise<number>|undefined;
 const stop=effect(()=>{if(action.pending()&&!nested)nested=action(2);});
 const first=action(1);expect(nested).toBeDefined();firstResult.resolve(1);await first;expect(action.pending()).toBe(true);
 nextResult.resolve(2);await nested;expect(action.pending()).toBe(false);stop.dispose();dispose(action);
});

it('unprintable rejection values still publish an Error and release pending',async()=>{
 const reason={toString(){throw Error('string conversion failed');}},action=createServerAction<void,void>({rpc:async()=>{throw reason;}});
 let failure:unknown;try{await action();}catch(error){failure=error;}
 expect(failure).toBeInstanceOf(Error);expect((failure as Error).cause).toBe(reason);expect(action.error()).toBe(failure);expect(action.pending()).toBe(false);dispose(action);
});
