/** @vitest-environment jsdom */
import { beforeEach, expect, it } from 'vitest';
import { mount, ErrorBoundary, createResource, createRoot, resetReactiveSystem, signal, Suspense, effect, getGraphTelemetry, getCurrentOwner } from '../src/engine';

beforeEach(() => resetReactiveSystem());

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}

it('abort settles loading even when the fetcher ignores cancellation', async () => {
  const request = deferred<string>();
  const resource = createResource(() => request.promise);
  expect(resource.loading()).toBe(true);
  resource.abort();
  expect(resource.loading()).toBe(false);
  request.resolve('stale');
  await request.promise;
  expect(resource()).toBeUndefined();
  expect(resource.error()).toBeNull();
});

it('keeps Suspense pending when an obsolete request settles before its replacement', async () => {
  const requests = [deferred<string>(), deferred<string>()];
  const source = signal(0);
  let resource!: ReturnType<typeof createResource<string, number>>;
  const container = document.createElement('div');
  let dispose!: () => void;
  createRoot(stop => {
    dispose = stop;
    container.append(Suspense({
      fallback: 'waiting',
      children: () => {
        resource = createResource(source, value => requests[value].promise);
        return document.createTextNode('ready');
      },
    }));
  });
  const fallback = container.querySelector<HTMLElement>('[data-svet-suspense-fallback]')!;
  source.set(1);
  requests[0].resolve('stale');
  await requests[0].promise;
  expect(resource.loading()).toBe(true);
  expect(fallback.style.display).toBe('contents');
  requests[1].resolve('current');
  await requests[1].promise;
  expect(resource()).toBe('current');
  expect(resource.loading()).toBe(false);
  expect(fallback.style.display).toBe('none');
  dispose();
});

it('disabling the source cancels pending work and ignores its rejection', async () => {
  const source = signal<number | false>(1);
  const request = deferred<string>();
  let requestSignal!: AbortSignal;
  const resource = createResource(source, (_, ctx) => {
    requestSignal = ctx.signal;
    return request.promise;
  });
  source.set(false);
  expect(requestSignal.aborted).toBe(true);
  expect(resource.loading()).toBe(false);
  request.reject(new Error('obsolete'));
  await request.promise.catch(() => {});
  expect(resource.error()).toBeNull();
});

it('refetch after owner disposal does not start another request', async () => {
  let calls = 0;
  let dispose!: () => void;
  const resource = createRoot(stop => {
    dispose = stop;
    return createResource(async () => { calls++; return 'value'; });
  });
  dispose();
  expect(resource.loading()).toBe(false);
  await resource.refetch();
  expect(calls).toBe(1);
  expect(resource()).toBeUndefined();
});


it('owner teardown releases resource signals and its source observer while a request is pending', async () => {
  const source = signal(0), request = deferred<string>(), baseline = getGraphTelemetry().nodes;
  let stop!: () => void, abortSignal!: AbortSignal;
  const resource = createRoot(dispose => { stop = dispose; return createResource(source, (_,ctx) => { abortSignal = ctx.signal; return request.promise; }); });
  stop(); expect(abortSignal.aborted).toBe(true); expect(getGraphTelemetry().nodes).toBe(baseline);
  request.resolve('late'); await request.promise; expect(resource()).toBeUndefined(); expect(resource.loading()).toBe(false);
  source.dispose();
});

it('manual resource disposal releases nodes and unregisters its owner cleanup', async () => {
  const request = deferred<string>(), source = signal(0); let calls = 0;
  createRoot(stop => {
    const owner = getCurrentOwner()!, callbacks = owner.disposables.length, baseline = getGraphTelemetry().nodes;
    const resource = createResource(source, () => { calls++; return request.promise; });
    resource.dispose(); resource.dispose(); expect(getGraphTelemetry().nodes).toBe(baseline);
    expect(owner.disposables).toHaveLength(callbacks); source.set(1); expect(calls).toBe(1); stop();
  });
  request.resolve('late'); await request.promise; source.dispose();
});

it('a failed initial source read releases resource state and owner registrations', () => {
  createRoot(stop => {
    const owner = getCurrentOwner()!, callbacks = owner.disposables.length, baseline = getGraphTelemetry().nodes;
    expect(() => createResource(() => { throw new Error('source failed'); }, async () => 'value')).toThrow('source failed');
    expect(getGraphTelemetry().nodes).toBe(baseline); expect(owner.disposables).toHaveLength(callbacks); stop();
  });
});

it('resource construction under a disposed owner fails before allocating or invoking the source', () => {
  createRoot(stop => {
    stop(); let invoked = false; const baseline = getGraphTelemetry().nodes;
    expect(() => createResource(() => { invoked = true; return 1; }, async () => 'value')).toThrow('owner');
    expect(invoked).toBe(false); expect(getGraphTelemetry().nodes).toBe(baseline);
  });
});

it('refetch after disposal or graph reset does not evaluate the source again', async () => {
  let reads = 0, calls = 0;
  const resource = createResource(() => { reads++; return 1; }, async () => { calls++; return 'value'; });
  resetReactiveSystem(); await resource.refetch(); expect(reads).toBe(1); expect(calls).toBe(1);
  resource.dispose(); await resource.refetch(); expect(reads).toBe(1); expect(calls).toBe(1);
});

it('manual disposal still releases every resource node if a loading observer throws', async () => {
  const source = signal(0), request = deferred<string>();
  const resource = createResource(source, () => request.promise);
  const observer = effect(() => { if (!resource.loading()) throw new Error('loading observer failed'); });
  expect(() => resource.dispose()).toThrow('loading observer failed');
  expect(getGraphTelemetry().nodes).toBe(2);
  resource.dispose(); request.resolve('late'); await request.promise;
  observer.dispose(); source.dispose(); expect(getGraphTelemetry().nodes).toBe(0);
});

it('abort remains reusable while dispose permanently freezes retained resource accessors', async () => {
  let calls = 0;
  const resource = createResource(async () => ++calls);
  resource.abort(); expect(await resource.refetch()).toBe(2); expect(resource()).toBe(2);
  resource.dispose(); resource.mutate(99); await resource.refetch();
  expect(calls).toBe(2); expect(resource()).toBe(2); expect(resource.loading()).toBe(false);
  expect(getGraphTelemetry().nodes).toBe(0);
});

it.each(['abort','dispose','reset'] as const)('refetch resolves undefined after %s even if the fetcher returns data', async mode => {
  const requests=[deferred<string>(),deferred<string>()]; let calls=0;
  const resource=createResource(()=>requests[calls++].promise), job=resource.refetch();
  if(mode==='reset') resetReactiveSystem(); else resource[mode]();
  requests[0].resolve('obsolete initial'); requests[1].resolve('obsolete refetch');
  expect(await job).toBeUndefined(); expect(resource()).toBeUndefined(); resource.dispose();
});

it('a superseded refetch returns no data while its replacement remains pending', async () => {
  const requests=[deferred<string>(),deferred<string>(),deferred<string>()]; let calls=0;
  const resource=createResource(()=>requests[calls++].promise), obsolete=resource.refetch(), current=resource.refetch();
  requests[1].resolve('obsolete'); expect(await obsolete).toBeUndefined(); expect(resource.loading()).toBe(true);
  requests[2].resolve('current'); expect(await current).toBe('current'); expect(resource()).toBe('current');
  requests[0].resolve('obsolete initial'); resource.dispose();
});

it('refetch always returns a Promise when the source reader throws', async () => {
  const failure=new Error('refetch source failed'); let fail=false;
  const resource=createResource(()=>{if(fail)throw failure;return 1;},async()=> 'value');
  await Promise.resolve(); fail=true; let job!:Promise<string|undefined>;
  expect(()=>{job=resource.refetch();}).not.toThrow(); await expect(job).rejects.toBe(failure); resource.dispose();
});

it('cancellation by a loading observer stops before invoking the fetcher', async () => {
  let calls=0;
  const resource=createResource(()=>false,async()=>{calls++;return 'value';});
  const observer=effect(()=>{if(resource.loading())resource.abort();});
  expect(await resource.refetch()).toBeUndefined(); expect(calls).toBe(0); expect(resource.loading()).toBe(false);
  observer.dispose(); resource.dispose();
});

it('refetch from an abort listener supersedes the outer refetch without an extra request', async () => {
  const requests=[deferred<string>(),deferred<string>(),deferred<string>()]; let calls=0, armed=false, nested!:Promise<string|undefined>;
  const resource=createResource(ctx=>{
    const request=requests[calls++]; ctx.signal.addEventListener('abort',()=>{if(armed){armed=false;nested=resource.refetch();}}); return request.promise;
  });
  armed=true; const outer=resource.refetch(); requests.forEach((request,i)=>request.resolve('value'+i));
  const result=await outer, newer=await nested;
  expect(calls).toBe(2); expect(result).toBeUndefined(); expect(newer).toBe('value1'); expect(resource()).toBe('value1'); resource.dispose();
});

it('abort does not clear loading for a replacement started by its abort listener', async () => {
  const requests=[deferred<string>(),deferred<string>()]; let calls=0, armed=false, nested!:Promise<string|undefined>;
  const resource=createResource(ctx=>{
    const request=requests[calls++]; ctx.signal.addEventListener('abort',()=>{if(armed){armed=false;nested=resource.refetch();}}); return request.promise;
  });
  armed=true; resource.abort(); expect(resource.loading()).toBe(true); expect(calls).toBe(2);
  requests[0].resolve('obsolete'); requests[1].resolve('current'); expect(await nested).toBe('current'); resource.dispose();
});


it('an error observer that starts recovery prevents stale boundary error forwarding', async () => {
  const requests=[deferred<string>(),deferred<string>()]; let calls=0, resource!:ReturnType<typeof createResource<string>>, recovered!:Promise<string|undefined>;
  const app=mount(()=>ErrorBoundary({children:()=>{
    resource=createResource(()=>requests[calls++].promise);
    effect(()=>{if(resource.error())recovered=resource.refetch();});
    return document.createTextNode('content');
  },fallback:()=>document.createTextNode('fallback')}),document.body);
  requests[0].reject(new Error('first request failed')); await requests[0].promise.catch(()=>{});
  expect(document.body.textContent).toBe('content'); expect(resource.loading()).toBe(true); expect(calls).toBe(2);
  requests[1].resolve('recovered'); expect(await recovered).toBe('recovered'); expect(resource()).toBe('recovered');
  app.unmount(); expect(getGraphTelemetry().nodes).toBe(0);
});


it.each(['root stop', 'effect rerun'] as const)('retiring ancestors prevent resource refetch and mutation during %s', async mode => {
  const { runWithOwnerScope, onCleanup } = await import('../src/engine');
  const trigger = signal(0), request = deferred<string>();
  let reads = 0, calls = 0, stop!: () => void;
  let resource!: ReturnType<typeof createResource<string, number>>;
  let attempted: Promise<string | undefined> | undefined, observed: string | undefined;
  let requestSignal!: AbortSignal;
  const children = () => {
    runWithOwnerScope([], () => onCleanup(() => {
      attempted = resource.refetch();
      resource.mutate('resurrected');
      observed = resource();
    }));
    runWithOwnerScope([], () => {
      resource = createResource(() => { reads++; return 1; }, (_, ctx) => {
        calls++; requestSignal = ctx.signal; return request.promise;
      }, { initialValue: 'initial' });
    });
  };
  createRoot(dispose => {
    stop = dispose;
    if (mode === 'root stop') children();
    else effect(() => { if (trigger() === 0) children(); });
  });
  if (mode === 'root stop') stop(); else trigger.set(1);
  expect([reads, calls, observed]).toEqual([1, 1, 'initial']);
  expect(requestSignal.aborted).toBe(true);
  expect(await attempted).toBeUndefined();
  request.resolve('late'); await request.promise;
  expect(resource()).toBe('initial');
  stop(); trigger.dispose(); expect(getGraphTelemetry().nodes).toBe(0);
});


it.each(['abort', 'dispose', 'owner stop', 'supersede', 'disable source'] as const)(
  '%s settles refetch without waiting for an uncooperative fetcher', async mode => {
    const source = signal<number | false>(1);
    const requests: ReturnType<typeof deferred<string>>[] = [];
    const signals: AbortSignal[] = [];
    let stop!: () => void;
    const resource = createRoot(dispose => {
      stop = dispose;
      return createResource(source, (_, ctx) => {
        signals.push(ctx.signal);
        const request = deferred<string>(); requests.push(request); return request.promise;
      });
    });
    const job = resource.refetch();
    let settled = false, value: string | undefined = 'unsettled';
    void job.then(result => { settled = true; value = result; });
    let replacement: Promise<string | undefined> | undefined;
    if (mode === 'owner stop') stop();
    else if (mode === 'supersede') replacement = resource.refetch();
    else if (mode === 'disable source') source.set(false);
    else resource[mode]();
    // Observe Promise jobs without resolving or rejecting the fetcher's Promise.
    for (let i = 0; i < 12; i++) await Promise.resolve();
    expect(settled).toBe(true); expect(value).toBeUndefined();
    expect(signals[1].aborted).toBe(true);
    expect(resource.loading()).toBe(mode === 'supersede');
    expect(resource.error()).toBeNull(); expect(resource()).toBeUndefined();
    if (replacement) {
      requests[2].resolve('current'); expect(await replacement).toBe('current');
      expect(resource()).toBe('current');
    }
    // Late cancellation failures must stay observed and cannot alter current state.
    requests[0].reject(new Error('late initial')); requests[1].reject(new Error('late refetch'));
    await Promise.allSettled(requests.map(request => request.promise));
    expect(resource.error()).toBeNull();
    expect(resource()).toBe(replacement ? 'current' : undefined);
    resource.dispose(); stop(); source.dispose(); expect(getGraphTelemetry().nodes).toBe(0);
  }
);


it('graph reset aborts pending fetchers and settles their refetch waits', async () => {
  const signals: AbortSignal[] = [];
  const resource = createResource(ctx => { signals.push(ctx.signal); return new Promise<string>(() => {}); });
  const job = resource.refetch(); let settled = false;
  void job.then(value => { expect(value).toBeUndefined(); settled = true; });
  resetReactiveSystem();
  for (let i = 0; i < 12; i++) await Promise.resolve();
  expect(signals.every(value => value.aborted)).toBe(true); expect(settled).toBe(true);
  expect(getGraphTelemetry().nodes).toBe(0); resource.dispose();
});

it('reset abort listeners may create replacement graph state that survives reset completion', async () => {
  let fresh!: ReturnType<typeof signal<number>>, freshResource!: ReturnType<typeof createResource<string>>;
  let freshSignal!: AbortSignal, oldCalls = 0;
  const resource = createResource(ctx => {
    oldCalls++;
    ctx.signal.addEventListener('abort', () => {
      if (oldCalls !== 2) return;
      oldCalls++; fresh = signal(17);
      freshResource = createResource(next => { freshSignal = next.signal; return new Promise<string>(() => {}); });
    });
    return new Promise<string>(() => {});
  });
  const job = resource.refetch();
  resetReactiveSystem();
  expect(fresh?.()).toBe(17); expect(freshSignal?.aborted).toBe(false);
  expect(await job).toBeUndefined(); resource.dispose();
  fresh.set(18); expect(fresh()).toBe(18);
  freshResource.abort(); freshResource.dispose(); fresh.dispose(); expect(getGraphTelemetry().nodes).toBe(0);
});


it('reset cancellation is confined to its WASM core', async () => {
  const { createWasmCoreInstance, withWasmCore } = await import('../src/engine/wasm-bridge');
  const other = createWasmCoreInstance(), requests = [deferred<string>(), deferred<string>()];
  let localSignal!: AbortSignal, foreignSignal!: AbortSignal;
  const local = createResource(ctx => { localSignal = ctx.signal; return requests[0].promise; });
  const foreign = withWasmCore(other, () => createResource(ctx => { foreignSignal = ctx.signal; return requests[1].promise; }));
  const localJob = local.refetch(), foreignJob = withWasmCore(other, () => foreign.refetch());
  resetReactiveSystem(); expect(localSignal.aborted).toBe(true); expect(foreignSignal.aborted).toBe(false);
  expect(await localJob).toBeUndefined();
  requests[0].resolve('obsolete'); requests[1].resolve('foreign');
  expect(await foreignJob).toBe('foreign'); expect(foreign()).toBe('foreign');
  local.dispose(); foreign.dispose();
  expect(getGraphTelemetry().nodes).toBe(0);
  expect(withWasmCore(other, () => getGraphTelemetry().nodes)).toBe(0);
});

it('recursive reset drains its own requests without losing remaining outer notifications or fresh state', async () => {
  let armed = false, nestedSignal!: AbortSignal, secondSignal!: AbortSignal;
  let nested!: ReturnType<typeof createResource<string>>, fresh!: ReturnType<typeof signal<number>>;
  const first = createResource(ctx => {
    ctx.signal.addEventListener('abort', () => {
      if (!armed) return; armed = false;
      nested = createResource(inner => { nestedSignal = inner.signal; return new Promise<string>(() => {}); });
      resetReactiveSystem(); fresh = signal(23);
    });
    return new Promise<string>(() => {});
  });
  const second = createResource(ctx => { secondSignal = ctx.signal; return new Promise<string>(() => {}); });
  const firstJob = first.refetch(), secondJob = second.refetch(); armed = true;
  resetReactiveSystem();
  expect(nestedSignal.aborted).toBe(true); expect(secondSignal.aborted).toBe(true);
  expect(await firstJob).toBeUndefined(); expect(await secondJob).toBeUndefined();
  expect(fresh()).toBe(23); fresh.set(24); expect(fresh()).toBe(24);
  first.dispose(); second.dispose(); nested.dispose(); expect(getGraphTelemetry().nodes).toBe(1);
  fresh.dispose(); expect(getGraphTelemetry().nodes).toBe(0);
});


it('foreign refetch runs source, fetcher and abort listeners in its resource core without caller dependencies', async () => {
  const { createWasmCoreInstance, getWasmCore, withWasmCore } = await import('../src/engine/wasm-bridge');
  const own = getWasmCore(), foreign = createWasmCoreInstance();
  const foreignSource = withWasmCore(foreign, () => signal(0));
  const sourceCores: unknown[] = [], fetcherCores: unknown[] = [], abortCores: unknown[] = [];
  const resource = createResource(() => { sourceCores.push(getWasmCore()); return 1; }, (_, ctx) => {
    fetcherCores.push(getWasmCore()); foreignSource();
    ctx.signal.addEventListener('abort', () => { abortCores.push(getWasmCore()); foreignSource(); });
    return new Promise<string>(() => {});
  });
  let runs = 0, job!: Promise<string | undefined>;
  const caller = withWasmCore(foreign, () => effect(() => { runs++; job = resource.refetch(); }));
  foreignSource.set(1);
  withWasmCore(foreign, () => resource.abort());
  expect(runs).toBe(1);
  expect(sourceCores.every(core => core === own)).toBe(true);
  expect(fetcherCores.every(core => core === own)).toBe(true);
  expect(abortCores.every(core => core === own)).toBe(true);
  expect(await job).toBeUndefined();
  caller.dispose(); resource.dispose(); foreignSource.dispose();
  expect(getGraphTelemetry().nodes).toBe(0); expect(withWasmCore(foreign, () => getGraphTelemetry().nodes)).toBe(0);
});

it('foreign refetch pauses a suspended observer in the resource core as well', async () => {
  const { createWasmCoreInstance, withWasmCore } = await import('../src/engine/wasm-bridge');
  const foreign = createWasmCoreInstance(), source = signal(0);
  const resource = createResource(() => { source(); return new Promise<string>(() => {}); });
  let runs = 0, job!: Promise<string | undefined>;
  const caller = effect(() => { runs++; withWasmCore(foreign, () => { job = resource.refetch(); }); });
  source.set(1); expect(runs).toBe(1);
  resource.abort(); expect(await job).toBeUndefined();
  caller.dispose(); resource.dispose(); source.dispose(); expect(getGraphTelemetry().nodes).toBe(0);
});


it.each(['success', 'failure'] as const)('foreign resource publishes async %s atomically in its own core', async mode => {
  const { createWasmCoreInstance, withWasmCore } = await import('../src/engine/wasm-bridge');
  const foreign = createWasmCoreInstance(), request = deferred<string>();
  const resource = withWasmCore(foreign, () => createResource(() => request.promise));
  const seen: unknown[] = [];
  const observer = withWasmCore(foreign, () => effect(() => {
    seen.push([resource(), resource.error()?.message ?? null, resource.loading()]);
  }));
  if (mode === 'success') request.resolve('ready'); else request.reject(new Error('failed'));
  await request.promise.catch(() => {});
  expect(seen).toEqual([[undefined, null, true], mode === 'success' ? ['ready', null, false] : [undefined, 'failed', false]]);
  observer.dispose(); resource.dispose(); expect(getGraphTelemetry().nodes).toBe(0);
  expect(withWasmCore(foreign, () => getGraphTelemetry().nodes)).toBe(0);
});


it.each(['options', 'source', 'fetcher'] as const)('resource construction rejects reset during initial %s without touching replacement nodes', stage => {
  let fresh!: ReturnType<typeof signal<number>>, stop!: () => void, reads = 0, fetches = 0, optionReads = 0;
  const invalidate = () => { resetReactiveSystem(); fresh = signal(17); };
  createRoot(dispose => {
    stop = dispose;
    expect(() => createResource(() => {
      reads++; if (stage === 'source') invalidate(); return 1;
    }, () => {
      fetches++; if (stage === 'fetcher') invalidate(); return new Promise<string>(() => {});
    }, { get initialValue() { optionReads++; if (stage === 'options') invalidate(); return 'initial'; } })).toThrow(/Resource.*(disposed|reset|invalidated)/);
  });
  expect(optionReads).toBe(1);
  expect(reads).toBe(stage === 'options' ? 0 : 1); expect(fetches).toBe(stage === 'fetcher' ? 1 : 0);
  expect(fresh()).toBe(17); stop(); fresh.set(18); expect(fresh()).toBe(18);
  expect(getGraphTelemetry().nodes).toBe(1); fresh.dispose();
});

it.each(['options', 'source', 'fetcher'] as const)('resource construction rejects owner stop during initial %s', stage => {
  let reads = 0, fetches = 0;
  createRoot(stop => {
    expect(() => createResource(() => {
      reads++; if (stage === 'source') stop(); return 1;
    }, () => {
      fetches++; if (stage === 'fetcher') stop(); return new Promise<string>(() => {});
    }, { get initialValue() { if (stage === 'options') stop(); return 'initial'; } })).toThrow(/(Resource|owner).*(disposed|reset|invalidated)/i);
  });
  expect(reads).toBe(stage === 'options' ? 0 : 1); expect(fetches).toBe(stage === 'fetcher' ? 1 : 0);
  expect(getGraphTelemetry().nodes).toBe(0);
});
