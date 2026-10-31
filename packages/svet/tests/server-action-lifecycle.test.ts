import {beforeEach,expect,it} from 'vitest';
import {createRoot,runWithOwnerScope,onCleanup,getCurrentOwner,createServerAction,effect,getGraphTelemetry,resetReactiveSystem,signal,type ServerAction} from '../src/engine';
beforeEach(()=>resetReactiveSystem());
function deferred<T>(){let resolve!:(value:T)=>void,reject!:(reason:unknown)=>void;const promise=new Promise<T>((yes,no)=>{resolve=yes;reject=no;});return {promise,resolve,reject};}

it('manual disposal is idempotent and rejects new calls before reading callback options',async()=>{
 let reads=0,optimistic=0;const action=createServerAction<void,number>({get rpc(){reads++;return async()=>1;},optimisticUpdate:()=>{optimistic++;return ()=>{};}});
 expect(getGraphTelemetry().nodes).toBe(2);action.dispose();action.dispose();expect(getGraphTelemetry().nodes).toBe(0);
 await expect(action()).rejects.toMatchObject({name:'ServerActionDisposedError'});expect(reads).toBe(0);expect(optimistic).toBe(0);
});

it('owner disposal releases action signals and prevents later invocations',async()=>{
 let action!:ServerAction<void,number>,stop!:()=>void;createRoot(dispose=>{stop=dispose;action=createServerAction({rpc:async()=>1});});
 stop();expect(getGraphTelemetry().nodes).toBe(0);await expect(action()).rejects.toMatchObject({name:'ServerActionDisposedError'});
});

it('disposal aborts the RPC signal and promptly rejects an RPC that ignores cancellation',async()=>{
 let rpcSignal:AbortSignal|undefined;const never=deferred<number>(),action=createServerAction<void,number>({rpc:(_args,abort)=>{rpcSignal=abort;return never.promise;}}),result=action();
 expect(action.pending()).toBe(true);action.dispose();expect(rpcSignal?.aborted).toBe(true);expect(action.pending()).toBe(false);
 await expect(result).rejects.toMatchObject({name:'ServerActionDisposedError'});expect(getGraphTelemetry().nodes).toBe(0);
 never.resolve(1);await Promise.resolve();expect(getGraphTelemetry().nodes).toBe(0);
});

it('late rejection after disposal cannot rollback or overwrite fresh reused signal slots',async()=>{
 const request=deferred<number>();let rollbacks=0;const action=createServerAction<void,number>({rpc:()=>request.promise,optimisticUpdate:()=>()=>{rollbacks++;}}),result=action();
 action.dispose();const fresh=signal(7);await expect(result).rejects.toMatchObject({name:'ServerActionDisposedError'});
 request.reject(Error('late RPC failure'));await Promise.resolve();await Promise.resolve();expect(rollbacks).toBe(0);expect(fresh()).toBe(7);expect(action.error()).toBeNull();fresh.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('pending observer disposal prevents optimistic work and RPC startup',async()=>{
 let calls=0,optimistic=0;const action=createServerAction<void,number>({rpc:async()=>{calls++;return 1;},optimisticUpdate:()=>{optimistic++;return ()=>{};}});
 const observer=effect(()=>{if(action.pending())action.dispose();});await expect(action()).rejects.toMatchObject({name:'ServerActionDisposedError'});
 expect(calls).toBe(0);expect(optimistic).toBe(0);observer.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('reset invalidates completion without disposing fresh graph nodes',async()=>{
 const request=deferred<number>(),action=createServerAction<void,number>({rpc:()=>request.promise}),result=action();resetReactiveSystem();const fresh=signal(10);
 request.resolve(1);await expect(result).rejects.toMatchObject({name:'ServerActionDisposedError'});action.dispose();expect(fresh()).toBe(10);fresh.set(11);expect(fresh()).toBe(11);fresh.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('disposal during optimistic preparation skips RPC and post-disposal rollback',async()=>{
 let action!:ServerAction<void,number>,calls=0,rollbacks=0;action=createServerAction({rpc:async()=>{calls++;return 1;},optimisticUpdate:()=>{action.dispose();return ()=>{rollbacks++;};}});
 await expect(action()).rejects.toMatchObject({name:'ServerActionDisposedError'});expect(calls).toBe(0);expect(rollbacks).toBe(0);expect(getGraphTelemetry().nodes).toBe(0);
});

it('construction inside a disposed owner fails before allocating signals',()=>{
 createRoot(dispose=>{dispose();expect(()=>createServerAction({rpc:async()=>1})).toThrow('disposed');});expect(getGraphTelemetry().nodes).toBe(0);
});

it('disposal cancels every overlapping invocation and abort-listener reentry is rejected',async()=>{
 let action!:ServerAction<number,number>,nested:Promise<number>|undefined,calls=0;const signals:AbortSignal[]=[];
 action=createServerAction({rpc:(_id:number,abort)=>{calls++;signals.push(abort);abort.addEventListener('abort',()=>{nested=action(3);void nested.catch(()=>{});});return new Promise<number>(()=>{});}});
 const a=action(1),b=action(2);action.dispose();await expect(a).rejects.toMatchObject({name:'ServerActionDisposedError'});await expect(b).rejects.toMatchObject({name:'ServerActionDisposedError'});await expect(nested).rejects.toMatchObject({name:'ServerActionDisposedError'});
 expect(signals.every(signal=>signal.aborted)).toBe(true);expect(calls).toBe(2);expect(getGraphTelemetry().nodes).toBe(0);
});

it('a callback-option getter may dispose the action without starting its returned RPC',async()=>{
 let action!:ServerAction<void,number>,calls=0;
 action=createServerAction({get rpc(){action.dispose();return async()=>{calls++;return 1;};}});
 await expect(action()).rejects.toMatchObject({name:'ServerActionDisposedError'});expect(calls).toBe(0);expect(getGraphTelemetry().nodes).toBe(0);
});


it('callback option snapshots preserve their original options receiver',async()=>{
 const options={marker:'value',optimisticCalls:0,
   optimisticUpdate(){this.optimisticCalls++;return ()=>{};},
   async rpc(_args:void,abort:AbortSignal){expect(abort.aborted).toBe(false);return this.marker;},
 };
 const action=createServerAction(options);expect(await action()).toBe('value');expect(options.optimisticCalls).toBe(1);action.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});


it('reset aborts and rejects all overlapping calls without waiting for an uncooperative RPC', async () => {
  const requests = [deferred<number>(), deferred<number>()], signals: AbortSignal[] = [];
  let calls = 0, rollbacks = 0;
  const action = createServerAction<void, number>({
    rpc: (_, abort) => { signals.push(abort); return requests[calls++].promise; },
    optimisticUpdate: () => () => { rollbacks++; },
  });
  let results: PromiseSettledResult<number>[] | undefined;
  void Promise.allSettled([action(), action()]).then(value => { results = value; });
  resetReactiveSystem();
  for (let i = 0; i < 12; i++) await Promise.resolve();
  expect(signals.every(signal => signal.aborted)).toBe(true);
  expect(results).toHaveLength(2);
  for (const result of results!) expect(result).toMatchObject({status: 'rejected', reason: {name: 'ServerActionDisposedError'}});
  requests.forEach(request => request.reject(new Error('late transport error')));
  await Promise.allSettled(requests.map(request => request.promise));
  expect(rollbacks).toBe(0); expect(getGraphTelemetry().nodes).toBe(0); action.dispose();
});

it('reset abort listeners can start replacement actions that survive old-owner cleanup', async () => {
  let old!: ServerAction<void, number>, stop!: () => void, fresh!: ReturnType<typeof signal<number>>;
  let replacement!: ServerAction<void, number>, replacementJob!: Promise<number>, replacementSignal!: AbortSignal;
  const request = deferred<number>();
  createRoot(dispose => {
    stop = dispose;
    old = createServerAction({rpc: (_, abort) => {
      abort.addEventListener('abort', () => {
        fresh = signal(17);
        replacement = createServerAction({rpc: (_, signal) => { replacementSignal = signal; return request.promise; }});
        replacementJob = replacement();
      });
      return new Promise<number>(() => {});
    }});
  });
  const oldJob = old(); void oldJob.catch(() => {}); resetReactiveSystem();
  expect(fresh?.()).toBe(17); expect(replacementSignal?.aborted).toBe(false);
  await expect(oldJob).rejects.toMatchObject({name: 'ServerActionDisposedError'}); stop(); old.dispose();
  fresh.set(18); expect(fresh()).toBe(18); expect(replacementSignal.aborted).toBe(false);
  request.resolve(42); expect(await replacementJob).toBe(42);
  replacement.dispose(); fresh.dispose(); expect(getGraphTelemetry().nodes).toBe(0);
});

it('reset leaves another WASM core action running', async () => {
  const {createWasmCoreInstance, withWasmCore} = await import('../src/engine/wasm-bridge');
  const other = createWasmCoreInstance(), request = deferred<number>();
  let localSignal!: AbortSignal, foreignSignal!: AbortSignal;
  const local = createServerAction<void, number>({rpc: (_, signal) => { localSignal = signal; return new Promise(() => {}); }});
  const foreign = withWasmCore(other, () => createServerAction<void, number>({rpc: (_, signal) => { foreignSignal = signal; return request.promise; }}));
  const localJob = local(); void localJob.catch(() => {});
  const foreignJob = withWasmCore(other, () => foreign()); resetReactiveSystem();
  expect(localSignal.aborted).toBe(true); expect(foreignSignal.aborted).toBe(false);
  await expect(localJob).rejects.toMatchObject({name: 'ServerActionDisposedError'});
  request.resolve(9); expect(await foreignJob).toBe(9);
  local.dispose(); foreign.dispose(); expect(getGraphTelemetry().nodes).toBe(0);
  expect(withWasmCore(other, () => getGraphTelemetry().nodes)).toBe(0);
});


it.each(['root stop', 'effect rerun'] as const)('ancestor %s rejects retained actions before reading options or starting work', async mode => {
  const trigger = signal(0); let stop!: () => void, calls = 0, optionReads = 0, optimistic = 0;
  let action!: ServerAction<void, number>, job!: Promise<number>, later!: NonNullable<ReturnType<typeof getCurrentOwner>>;
  let unvisited = false;
  createRoot(dispose => {
    stop = dispose;
    const children = () => {
      runWithOwnerScope([], () => onCleanup(() => {
        unvisited = !later.disposed; job = action(); void job.catch(() => {});
      }));
      runWithOwnerScope([], () => {
        later = getCurrentOwner()!;
        action = createServerAction({
          get optimisticUpdate() { optionReads++; return () => { optimistic++; return () => {}; }; },
          get rpc() { optionReads++; return async () => { calls++; return 1; }; },
        });
      });
    };
    if (mode === 'root stop') children(); else effect(() => { if (trigger() === 0) children(); });
  });
  if (mode === 'root stop') stop(); else trigger.set(1);
  expect(unvisited).toBe(true); expect([optionReads, optimistic, calls]).toEqual([0, 0, 0]);
  await expect(job).rejects.toMatchObject({name: 'ServerActionDisposedError'});
  stop(); trigger.dispose(); expect(getGraphTelemetry().nodes).toBe(0);
});


it.each(['optimistic getter', 'RPC getter', 'optimistic update', 'RPC', 'rollback', 'abort', 'error normalization'] as const)(
  'action %s does not collect dependencies in either suspended caller core', async stage => {
    const {createWasmCoreInstance, getWasmCore, withWasmCore} = await import('../src/engine/wasm-bridge');
    const own = getWasmCore(), other = createWasmCoreInstance();
    const local = signal(0), foreign = withWasmCore(other, () => signal(0));
    const callbackCores: unknown[] = [];
    const read = () => { callbackCores.push(getWasmCore()); local(); foreign(); };
    const failure = new Error('RPC failed');
    const action = createServerAction<void, number>({
      get optimisticUpdate() {
        if (stage === 'optimistic getter') read();
        return () => { if (stage === 'optimistic update') read(); return () => { if (stage === 'rollback') read(); }; };
      },
      get rpc() {
        if (stage === 'RPC getter') read();
        return (_args: void, abort: AbortSignal) => {
          if (stage === 'RPC') read();
          if (stage === 'rollback') throw failure;
          if (stage === 'error normalization') throw {toString() { read(); return 'normalized RPC failure'; }};
          if (stage === 'abort') { abort.addEventListener('abort', read); return new Promise<number>(() => {}); }
          return Promise.resolve(1);
        };
      },
    });
    let outerRuns = 0, innerRuns = 0, job!: Promise<number>;
    const inners: ReturnType<typeof effect>[] = [];
    const outer = effect(() => {
      outerRuns++; withWasmCore(other, () => {
        inners.push(effect(() => {
          innerRuns++; job = action(); void job.catch(() => {});
          if (stage === 'abort') action.dispose();
        }));
      });
    });
    local.set(1); foreign.set(1);
    expect([outerRuns, innerRuns]).toEqual([1, 1]);
    expect(callbackCores.length).toBeGreaterThan(0); expect(callbackCores.every(core => core === own)).toBe(true);
    if (stage === 'abort') await expect(job).rejects.toMatchObject({name: 'ServerActionDisposedError'});
    else if (stage === 'rollback') await expect(job).rejects.toBe(failure);
    else if (stage === 'error normalization') await expect(job).rejects.toThrow('normalized RPC failure');
    else expect(await job).toBe(1);
    outer.dispose(); inners.forEach(inner => inner.dispose()); action.dispose(); local.dispose(); foreign.dispose();
    expect(getGraphTelemetry().nodes).toBe(0); expect(withWasmCore(other, () => getGraphTelemetry().nodes)).toBe(0);
  }
);


it.each(['dispose', 'reset'] as const)('error normalization %s prevents obsolete rollback', async mode => {
  let action!: ServerAction<void, number>, rollbacks = 0, fresh: ReturnType<typeof signal<number>> | undefined;
  const failure = {toString() {
    if (mode === 'reset') { resetReactiveSystem(); fresh = signal(17); } else action.dispose();
    return 'obsolete transport error';
  }};
  action = createServerAction({rpc: async () => { throw failure; }, optimisticUpdate: () => () => { rollbacks++; }});
  await expect(action()).rejects.toMatchObject({name: 'ServerActionDisposedError'});
  expect(rollbacks).toBe(0); action.dispose();
  if (fresh) { fresh.set(18); expect(fresh()).toBe(18); fresh.dispose(); }
  expect(getGraphTelemetry().nodes).toBe(0);
});


it('RPC then getter runs once in the action core without collecting either caller dependencies', async () => {
  const {createWasmCoreInstance, getWasmCore, withWasmCore} = await import('../src/engine/wasm-bridge');
  const own = getWasmCore(), other = createWasmCoreInstance(), local = signal(0);
  const foreign = withWasmCore(other, () => signal(0));
  let reads = 0, getterCore: unknown, outerRuns = 0, innerRuns = 0;
  const operation = {get then() {
    reads++; getterCore = getWasmCore(); local(); foreign();
    return (resolve: (value: number) => void) => resolve(7);
  }};
  const action = createServerAction<void, number>({rpc: () => operation as unknown as Promise<number>});
  let job!: Promise<number>; const inners: ReturnType<typeof effect>[] = [];
  const outer = effect(() => { outerRuns++; withWasmCore(other, () => {
    inners.push(effect(() => { innerRuns++; job = action(); void job.catch(() => {}); }));
  }); });
  local.set(1); foreign.set(1);
  expect([outerRuns, innerRuns, reads]).toEqual([1, 1, 1]); expect(getterCore).toBe(own);
  expect(await job).toBe(7); outer.dispose(); inners.forEach(inner => inner.dispose());
  action.dispose(); local.dispose(); foreign.dispose(); expect(getGraphTelemetry().nodes).toBe(0);
  expect(withWasmCore(other, () => getGraphTelemetry().nodes)).toBe(0);
});

it('RPC disposal still observes an already-rejected returned Promise', async () => {
  let action!: ServerAction<void, number>;
  action = createServerAction({rpc: () => { action.dispose(); return Promise.reject(new Error('obsolete RPC rejection')); }});
  await expect(action()).rejects.toMatchObject({name: 'ServerActionDisposedError'});
  await Promise.resolve(); expect(getGraphTelemetry().nodes).toBe(0);
});


it.each(['throw', 'dispose', 'reset'] as const)('RPC then getter %s preserves cancellation and rollback rules', async mode => {
  let action!: ServerAction<void, number>, reads = 0, rollbacks = 0, fresh: ReturnType<typeof signal<number>> | undefined;
  const failure = new Error('RPC then getter failed');
  const operation = {get then() {
    reads++;
    if (mode === 'throw') throw failure;
    if (mode === 'dispose') action.dispose();
    else { resetReactiveSystem(); fresh = signal(17); }
    return (resolve: (value: number) => void) => resolve(7);
  }};
  action = createServerAction({rpc: () => operation as unknown as Promise<number>, optimisticUpdate: () => () => { rollbacks++; }});
  const job = action();
  if (mode === 'throw') { await expect(job).rejects.toBe(failure); expect(action.error()).toBe(failure); }
  else await expect(job).rejects.toMatchObject({name: 'ServerActionDisposedError'});
  expect(reads).toBe(1); expect(rollbacks).toBe(mode === 'throw' ? 1 : 0); action.dispose();
  if (fresh) { fresh.set(18); expect(fresh()).toBe(18); fresh.dispose(); }
  expect(getGraphTelemetry().nodes).toBe(0);
});


it('RPC then method reenters its core asynchronously with the original receiver and first-settlement semantics', async () => {
  const {createWasmCoreInstance, getWasmCore, withWasmCore} = await import('../src/engine/wasm-bridge');
  const own = createWasmCoreInstance(); let reads = 0, calls = 0, seenCore: unknown, receiver: unknown;
  const order: string[] = [];
  const operation = {get then() {
    reads++; order.push('get');
    return function(this: unknown, resolve: (value: number) => void, reject: (error: Error) => void) {
      calls++; receiver = this; seenCore = getWasmCore(); order.push('then');
      resolve(7); reject(new Error('ignored rejection')); throw new Error('ignored throw');
    };
  }};
  const action = withWasmCore(own, () => createServerAction<void, number>({rpc: () => operation as unknown as Promise<number>}));
  Object.freeze(operation);
  const job = action(); order.push('returned'); expect(order).toEqual(['get', 'returned']);
  expect(await job).toBe(7); expect(order).toEqual(['get', 'returned', 'then']);
  expect([reads, calls]).toEqual([1, 1]); expect(receiver).toBe(operation); expect(seenCore).toBe(own);
  action.dispose(); expect(withWasmCore(own, () => getGraphTelemetry().nodes)).toBe(0);
});

it.each(['throw', 'dispose', 'reset'] as const)('RPC then method %s retains owned-core lifetime rules', async mode => {
  const {createWasmCoreInstance, getWasmCore, withWasmCore} = await import('../src/engine/wasm-bridge');
  const own = createWasmCoreInstance(), failure = new Error('then failed');
  let action!: ServerAction<void, number>, rollbacks = 0, seenCore: unknown, fresh: ReturnType<typeof signal<number>> | undefined;
  const operation = {then(resolve: (value: number) => void) {
    seenCore = getWasmCore();
    if (mode === 'throw') throw failure;
    if (mode === 'dispose') action.dispose(); else { resetReactiveSystem(); fresh = signal(17); }
    resolve(7);
  }};
  action = withWasmCore(own, () => createServerAction({rpc: () => operation as unknown as Promise<number>, optimisticUpdate: () => () => { rollbacks++; }}));
  if (mode === 'throw') await expect(action()).rejects.toBe(failure);
  else await expect(action()).rejects.toMatchObject({name: 'ServerActionDisposedError'});
  expect(seenCore).toBe(own); expect(rollbacks).toBe(mode === 'throw' ? 1 : 0); action.dispose();
  if (fresh) { fresh.set(18); expect(fresh()).toBe(18); fresh.dispose(); }
  expect(getGraphTelemetry().nodes).toBe(0); expect(withWasmCore(own, () => getGraphTelemetry().nodes)).toBe(0);
});

it('plain RPC result identity and noncallable then getter reads remain compatible', async () => {
  let reads = 0; const value = {get then() { reads++; return undefined; }};
  const action = createServerAction<void, typeof value>({rpc: () => value as unknown as Promise<typeof value>});
  expect(await action()).toBe(value); expect(reads).toBe(3); action.dispose();
  expect(getGraphTelemetry().nodes).toBe(0);
});


it('nested RPC thenables keep their core and terminal identity without recursive stack growth', async () => {
  const {createWasmCoreInstance, getWasmCore, withWasmCore} = await import('../src/engine/wasm-bridge');
  const own = createWasmCoreInstance(), terminal = Object.freeze({answer: 42});
  let value: unknown = terminal, reads = 0, calls = 0, correctCore = true;
  for (let i = 0; i < 1500; i++) {
    const next = value;
    value = Object.freeze({get then() {
      reads++; correctCore &&= getWasmCore() === own;
      return (resolve: (value: unknown) => void) => { calls++; correctCore &&= getWasmCore() === own; resolve(next); };
    }});
  }
  const action = withWasmCore(own, () => createServerAction<void, typeof terminal>({rpc: () => value as Promise<typeof terminal>}));
  expect(await action()).toBe(terminal); expect([reads, calls]).toEqual([1500, 1500]); expect(correctCore).toBe(true);
  action.dispose(); expect(withWasmCore(own, () => getGraphTelemetry().nodes)).toBe(0);
});


it('a self-resolving thenable that becomes a plain object retains its terminal identity', async () => {
  let reads = 0;
  const value = Object.freeze({get then() {
    reads++; return reads === 1 ? (resolve: (value: unknown) => void) => resolve(value) : undefined;
  }});
  const action = createServerAction<void, unknown>({rpc: () => value as unknown as Promise<unknown>});
  expect(await action()).toBe(value); expect(reads).toBe(4); expect(action.pending()).toBe(false);
  action.dispose(); expect(getGraphTelemetry().nodes).toBe(0);
});
