import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { mkdtemp, mkdir, readFile, readdir, rm, writeFile } from 'node:fs/promises';
import { createRequire } from 'node:module';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const require = createRequire(import.meta.url);
const keep = process.argv.includes('--keep');
const temporary = await mkdtemp(join(tmpdir(), 'svet-consumer-'));
const consumer = join(temporary, 'consumer');
await mkdir(consumer);
function run(command, args, cwd = consumer) {
  try {
    return execFileSync(command, args, { cwd, encoding: 'utf8', maxBuffer: 10 * 1024 * 1024, timeout: 120000 });
  } catch (error) {
    process.stderr.write(String(error.stdout ?? '') + String(error.stderr ?? ''));
    throw error;
  }
}
const node = (args, cwd) => run(process.execPath, args, cwd);
let passed = false;
try {
  const [packed] = JSON.parse(run('npm', ['pack', '--json', '--ignore-scripts', '--loglevel=error', '--pack-destination', temporary], root));
  assert(packed.files.every(file => file.path.startsWith('dist/package/') || ['package.json', 'README.md', 'DEVELOPMENT.md'].includes(file.path)), 'unexpected development files in tarball');
  assert(packed.files.some(file => file.path === 'dist/package/index.js'));
  assert(packed.files.some(file => file.path === 'dist/package/index.d.ts'));
  await writeFile(join(consumer, 'package.json'), JSON.stringify({ name: 'svet-clean-consumer', private: true, type: 'module' }));
  run('npm', ['install', '--no-audit', '--no-fund', '--registry=https://registry.npmjs.org', '--@babel:registry=https://registry.npmjs.org', '--@jridgewell:registry=https://registry.npmjs.org', join(temporary, packed.filename)]);
  const installed = join(consumer, 'node_modules/svet-engine');
  const metadata = JSON.parse(await readFile(join(installed, 'package.json'), 'utf8'));
  for (const entry of Object.values(metadata.exports)) {
    assert(entry.import.endsWith('.js') && entry.types.endsWith('.d.ts'));
    await readFile(join(installed, entry.import));
    await readFile(join(installed, entry.types));
  }
  // No Vite or TypeScript dependency may be required just to install the runtime.
  const consumerRequire = createRequire(join(consumer, 'package.json'));
  assert.throws(() => consumerRequire.resolve('vite'));
  assert.throws(() => consumerRequire.resolve('typescript'));
  await writeFile(join(consumer, 'runtime.mjs'), `
    import assert from 'node:assert/strict';
    import { captureReactiveContext, ReactiveContextDisposedError, Fragment, bindProp, useContext, h, createContext, renderToString, createServerAction, ServerActionDisposedError, For, untrack, Portal, createRouter, Suspense, ErrorBoundary, createResource, initWebGpuHardware, onCleanup, resetReactiveSystem, getCurrentOwner, runWithOwnerScope, signal, effect, computed, batch, createRoot, createWasmCoreInstance, createPerspectiveCamera3D, getGraphTelemetry, compileFormaToJsModule, createGpuTensor, getGpuTelemetry, configureGpuSubsystem, GpuComputeCapacityError, ReactiveStabilizationError, ReactiveCycleError, dispatchGpuComputeKernel, dispatchGpuComputeKernelAsync, Viewport3D, createMesh3D, createTransform3D, pickNearestVertex3D, getWasmCore, withWasmCore } from 'svet-engine';
    import { signal as reactiveSignal, runWithCapturedOwner } from 'svet-engine/reactivity';
    import { renderToStringAsync, runWithIsolatedContextAsync } from 'svet-engine/server';
    import { transformSvetJsx, svetVitePlugin } from 'svet-engine/compiler';
    import { jsx, Fragment as runtimeFragment } from 'svet-engine/jsx-runtime';
    import { jsxDEV } from 'svet-engine/jsx-dev-runtime';
    assert.equal(typeof Viewport3D, 'function');
    const transform = createTransform3D();
    const indexed = createMesh3D({ positions: [0,0,0, 1,0,0, 0,1,0], indices: [0,1,2], indexFormat: 'uint32', transform });
    assert.ok(indexed.indices instanceof Uint32Array); indexed.updateIndices([]); assert.equal(indexed.indices.length, 0); indexed.updateIndices([2,1,0]); assert.equal(indexed.indices[0], 2); indexed.dispose();
    const hidden = createMesh3D({ positions: [-0.8,-0.8,0.2, 0.8,-0.8,0.2, 0,0.8,0.2, 0,0,0.8], indices: [0,1,2], transform });
    assert.equal(pickNearestVertex3D(hidden,100,100,{ viewportWidth:200,viewportHeight:200,maxRadiusPx:1 }), null);
    assert.equal(pickNearestVertex3D(hidden,100,100,{ viewportWidth:200,viewportHeight:200,maxRadiusPx:1,occlusion:'none' }).vertexIndex,3);
    hidden.dispose(); transform.dispose();
    const cleanupOwned = createWasmCoreInstance(), cleanupCaller = createWasmCoreInstance(), cleanupCores = [];
    let stopCleanupRoot, stopCleanupEffect;
    runWithCapturedOwner(cleanupOwned, null, () => {
      createRoot(stop => { stopCleanupRoot = stop; onCleanup(() => cleanupCores.push(getWasmCore())); });
      stopCleanupEffect = effect(() => () => cleanupCores.push(getWasmCore())).dispose;
    });
    runWithCapturedOwner(cleanupCaller, null, () => {
      stopCleanupRoot(); stopCleanupEffect(); assert.equal(getWasmCore(), cleanupCaller);
    });
    assert.deepEqual(cleanupCores, [cleanupOwned, cleanupOwned]);
    assert.equal(cleanupOwned.svet_get_node_count(), 0); assert.equal(cleanupCaller.svet_get_node_count(), 0);
    const cleanupGate = signal(0), cleanupRead = signal(0); let cleanupRuns = 0, stopUntrackedRoot;
    createRoot(stop => { stopUntrackedRoot = stop; onCleanup(() => cleanupRead()); });
    const cleanupObserver = effect(() => { cleanupGate(); cleanupRuns++; stopUntrackedRoot(); });
    cleanupRead.set(1); assert.equal(cleanupRuns, 1); cleanupGate.set(1); assert.equal(cleanupRuns, 2);
    cleanupObserver.dispose(); cleanupGate.dispose(); cleanupRead.dispose();
    const foreignOwned = createWasmCoreInstance(), foreignCaller = createWasmCoreInstance(), foreignSeen = [], foreignValues = [];
    let foreignSource, foreignComputed, foreignObserver;
    runWithCapturedOwner(foreignOwned, null, () => {
      foreignSource = signal(0, {equals: (a,b) => { foreignSeen.push(getWasmCore()); return a === b; }});
      foreignObserver = effect(() => { foreignValues.push(foreignSource()); foreignSeen.push(getWasmCore()); return () => { foreignSeen.push(getWasmCore()); }; });
      foreignComputed = computed(() => { foreignSeen.push(getWasmCore()); return foreignSource() * 2; });
    });
    runWithCapturedOwner(foreignCaller, null, () => {
      const live = signal(17);
      foreignSource.update(value => { foreignSeen.push(getWasmCore()); return value + 1; });
      assert.equal(foreignComputed(), 2); foreignSource.set(2); assert.equal(foreignComputed.peek(), 4);
      assert.equal(getWasmCore(), foreignCaller); live.set(18); assert.equal(live(), 18); live.dispose();
    });
    assert.deepEqual(foreignValues, [0,1,2]); assert.ok(foreignSeen.every(core => core === foreignOwned));
    foreignObserver.dispose(); foreignComputed.dispose(); foreignSource.dispose();
    assert.equal(foreignOwned.svet_get_node_count(), 0); assert.equal(foreignCaller.svet_get_node_count(), 0);
    for (const construct of [options => signal(1, options), options => computed(() => 1, options)]) {
      const before = getGraphTelemetry().nodes, failure = new Error('consumer equals getter failed');
      assert.throws(() => construct({get equals() { assert.equal(getGraphTelemetry().nodes, before); throw failure; }}), error => error === failure);
      assert.equal(getGraphTelemetry().nodes, before);
      assert.throws(() => construct({equals: false}), TypeError); assert.equal(getGraphTelemetry().nodes, before);
      const constructionCore = createWasmCoreInstance();
      runWithCapturedOwner(constructionCore, null, () => {
        let fresh;
        assert.throws(() => construct({get equals() { resetReactiveSystem(); fresh = signal(17); return Object.is; }}), /reset/);
        fresh.set(18); assert.equal(fresh(), 18); assert.equal(getGraphTelemetry().nodes, 1);
        const freshValues = [], freshObserver = effect(() => { freshValues.push(fresh()); });
        fresh.set(19); assert.deepEqual(freshValues, [18,19]); freshObserver.dispose(); fresh.dispose();
      });
      assert.equal(constructionCore.svet_get_node_count(), 0);
    }
    assert.throws(() => computed(1), TypeError);
    const registeredCleanupEvents = [], registeredCleanupSource = signal(0);
    const registeredCleanupEffect = effect(register => {
      const value = registeredCleanupSource();
      register(() => registeredCleanupEvents.push(value + ':first'));
      register(() => registeredCleanupEvents.push(value + ':second'));
      return () => registeredCleanupEvents.push(value + ':returned');
    });
    registeredCleanupSource.set(1); registeredCleanupEffect.dispose(); registeredCleanupSource.dispose();
    assert.deepEqual(registeredCleanupEvents, ['0:returned','0:second','0:first','1:returned','1:second','1:first']);
    assert.throws(() => effect(1), TypeError);
    const lateOwned = createWasmCoreInstance(), lateCaller = createWasmCoreInstance(); let lateRegister;
    runWithCapturedOwner(lateOwned, null, () => { const handle = effect(register => {lateRegister = register;}); handle.dispose(); });
    runWithCapturedOwner(lateCaller, null, () => {
      const gate = signal(0), hidden = signal(0); let runs = 0;
      const observer = effect(() => { gate(); runs++; lateRegister(() => {assert.equal(getWasmCore(), lateOwned); hidden();}); });
      hidden.set(1); assert.equal(runs, 1); gate.set(1); assert.equal(runs, 2);
      observer.dispose(); gate.dispose(); hidden.dispose();
    });
    const retrySource = signal(0), retryComputed = computed(() => retrySource()*2), retryValues = [], retryFailure = new Error('consumer cleanup failed');
    const retryFirst = effect(() => {const value = retryComputed(); retryValues.push(value); return () => {if(value===0)throw retryFailure;};});
    const retrySecond = effect(() => {retrySource();});
    assert.throws(() => retrySource.set(1), error => error === retryFailure); retrySource.set(2); assert.deepEqual(retryValues, [0,4]);
    retryFirst.dispose(); retrySecond.dispose(); retryComputed.dispose(); retrySource.dispose();
    assert.equal(Fragment, runtimeFragment);
    let fragmentMetadataReads = 0;
    createRoot(stop => {
      stop();
      assert.throws(() => Fragment({get children() {fragmentMetadataReads++; return 'stale';}}), /disposed/);
    });
    assert.equal(fragmentMetadataReads, 0);

    const rerunTrigger = signal(0), rerunSource = signal(0);
    let rerunDerived, rerunObserved = -1, rerunComputes = 0, rerunRuns = 0;
    const rerunObserver = effect(() => {
      rerunTrigger();
      runWithOwnerScope([], () => onCleanup(() => {rerunSource.set(rerunSource.peek()+1); rerunObserved = rerunDerived();}));
      runWithOwnerScope([], () => {
        rerunDerived = computed(() => {rerunComputes++; return rerunSource();});
        effect(() => {rerunDerived(); rerunRuns++;});
      });
    });
    rerunTrigger.set(1); assert.deepEqual([rerunRuns,rerunComputes,rerunObserved], [2,2,0]);
    rerunObserver.dispose(); rerunTrigger.dispose(); rerunSource.dispose();

    const capturedProvider = createContext('default'), capturedSource = signal(0);
    let capturedContext, capturedOwnerStop, capturedRuns = 0;
    createRoot(stop => {
      capturedOwnerStop = stop;
      runWithOwnerScope([[capturedProvider.id,'captured']], () => {capturedContext = captureReactiveContext();});
    });
    await Promise.resolve();
    assert.equal(capturedContext.run(() => {
      effect(() => {capturedSource(); capturedRuns++;});
      return useContext(capturedProvider);
    }), 'captured');
    assert.equal(useContext(capturedProvider), 'default');
    capturedContext.dispose(); capturedSource.set(1); assert.equal(capturedRuns, 2);
    capturedOwnerStop(); capturedSource.set(2); assert.equal(capturedRuns, 2);
    assert.equal(capturedContext.active, false);
    assert.throws(() => capturedContext.run(() => {}), error => error instanceof ReactiveContextDisposedError && error.code === 'SVET_CONTEXT_DISPOSED');
    capturedSource.dispose();

    let requestContinuation;
    assert.equal(await runWithIsolatedContextAsync(async () => {
      requestContinuation = captureReactiveContext();
      await Promise.resolve();
      assert.equal(requestContinuation.active, true);
      return requestContinuation.run(() => 23);
    }), 23);
    assert.equal(requestContinuation.active, false);
    assert.throws(() => requestContinuation.run(() => 0), ReactiveContextDisposedError);

    createRoot(stop => {
      const owner = getCurrentOwner(), before = owner.disposables.length;
      const captures = Array.from({length: 1024}, () => captureReactiveContext());
      assert.equal(owner.disposables.length, before+1);
      captures.forEach(context => context.dispose());
      assert.equal(owner.disposables.length, before);
      assert.equal(captures.every(context => !context.active), true);
      stop();
    });

    const eventContext = createContext('default'), eventSource = signal(0), eventTarget = new EventTarget();
    let eventOwnerStop, eventViewRuns = 0, eventCallerRuns = 0, eventCleanups = 0, eventContextValue;
    createRoot(stop => {
      eventOwnerStop = stop;
      runWithOwnerScope([[eventContext.id,'bound']], () => bindProp(eventTarget, 'onFocus', () => {
        eventContextValue = useContext(eventContext); eventSource();
        effect(() => {eventSource(); eventViewRuns++;}); onCleanup(() => {eventCleanups++;});
      }));
    });
    const eventCaller = effect(() => {eventCallerRuns++; eventTarget.dispatchEvent(new Event('focus'));});
    eventSource.set(1);
    assert.deepEqual([eventContextValue,eventViewRuns,eventCallerRuns], ['bound',2,1]);
    eventOwnerStop(); eventSource.set(2);
    assert.deepEqual([eventViewRuns,eventCallerRuns,eventCleanups], [2,1,1]);
    eventCaller.dispose(); eventSource.dispose();

    const ancestorSource = signal(0); let ancestorRuns = 0, ancestorComputes = 0, ancestorObserved = -1, ancestorDerived;
    createRoot(stop => {
      runWithOwnerScope([], () => onCleanup(() => {ancestorSource.set(1); ancestorObserved = ancestorDerived();}));
      runWithOwnerScope([], () => {
        ancestorDerived = computed(() => {ancestorComputes++; return ancestorSource();});
        effect(() => {ancestorDerived(); ancestorRuns++;});
      });
      stop();
    });
    assert.deepEqual([ancestorRuns,ancestorComputes,ancestorObserved], [1,1,0]); ancestorSource.dispose();

    const hMetadataSource = signal(0), hMetadataFailure = new Error('consumer h metadata failed');
    let hMetadataRuns = 0, hMetadataReads = 0;
    createRoot(stop => {
      const before = getGraphTelemetry().nodes;
      assert.throws(() => h('div', {get title() {effect(() => {hMetadataSource(); hMetadataRuns++;}); throw hMetadataFailure;}}), error => error === hMetadataFailure);
      assert.equal(getGraphTelemetry().nodes, before);
      assert.throws(() => h(() => {effect(() => {hMetadataSource(); hMetadataRuns++;}); throw hMetadataFailure;}, null), error => error === hMetadataFailure);
      assert.equal(getGraphTelemetry().nodes, before);
      stop();
    });
    hMetadataSource.set(1); assert.equal(hMetadataRuns, 2); hMetadataSource.dispose();
    createRoot(stop => {
      stop();
      assert.throws(() => h('div', {get title() {hMetadataReads++; return 'stale';}}), /disposed/);
    });
    assert.equal(hMetadataReads, 0);

    const providerMetadataContext = createContext('default'), providerMetadataSource = signal(0), providerMetadataFailure = new Error('consumer Provider metadata failed');
    let providerMetadataRuns = 0, providerMetadataReads = 0;
    createRoot(stop => {
      const before = getGraphTelemetry().nodes;
      assert.throws(() => providerMetadataContext.Provider({get value() {effect(() => {providerMetadataSource(); providerMetadataRuns++;}); throw providerMetadataFailure;},children:null}), error => error === providerMetadataFailure);
      assert.equal(getGraphTelemetry().nodes, before); stop();
    });
    providerMetadataSource.set(1); assert.equal(providerMetadataRuns, 1); providerMetadataSource.dispose();
    createRoot(stop => {
      stop();
      assert.throws(() => providerMetadataContext.Provider({get value() {providerMetadataReads++; return 'stale';},children:null}), /disposed/);
    });
    assert.equal(providerMetadataReads, 0);
    const snapshotCore = getWasmCore();
    const snapshotPtr = snapshotCore.svet_prepare_gpu_pick_vertices(4);
    new Float32Array(snapshotCore.memory.buffer, snapshotPtr, 4).set([0,0,0.5,1]);
    assert.equal(snapshotCore.svet_gpu_pick_snapshot_3d(1,0,0,200,200,100,100,1),0);
    assert.equal(signal, reactiveSignal);
    assert.equal(typeof jsx, 'function'); assert.equal(typeof jsxDEV, 'function');
    const results = await Promise.all([1, 10, 100].map(start => runWithIsolatedContextAsync(async () => {
      const value = signal(start);
      await new Promise(resolve => setTimeout(resolve, 1));
      const values = []; effect(() => values.push(value())); value.set(start + 1);
      return values;
    })));
    assert.deepEqual(results, [[1, 2], [10, 11], [100, 101]]);
    assert.equal(getGraphTelemetry().nodes, 0);
    let snapshotRenderCore, snapshotRenderSignal;
    const renderedSnapshot = renderToString(() => {
      snapshotRenderCore = getWasmCore(); snapshotRenderSignal = signal(3);
      return {nodeType: 3, textContent: String(snapshotRenderSignal())};
    });
    assert.equal(renderedSnapshot.html, '3');
    assert.equal(snapshotRenderCore.svet_get_node_count(), 0);
    snapshotRenderSignal.set(4); assert.equal(snapshotRenderSignal(), 3);
    let finishRenderCaller, notifyRenderStarted, ownedRenderCore;
    const renderStarted = new Promise(resolve => { notifyRenderStarted = resolve; });
    const shutdownRender = createRoot(dispose => {
      finishRenderCaller = dispose;
      return renderToStringAsync(async () => {
        ownedRenderCore = getWasmCore(); signal(7); notifyRenderStarted();
        return await new Promise(() => {});
      });
    });
    await renderStarted; finishRenderCaller();
    await assert.rejects(shutdownRender, /disposed/);
    assert.equal(ownedRenderCore.svet_get_node_count(), 0);
    assert.equal(getGraphTelemetry().nodes, 0);
    const staleSignal = signal(10); let recycledSignal;
    staleSignal.update(previous => { staleSignal.dispose(); recycledSignal = signal(100); return previous + 1; });
    assert.equal(staleSignal.peek(), 10); assert.equal(recycledSignal.peek(), 100); recycledSignal.dispose();
    const computedSource = signal(1); let staleComputed, recycledComputedSlot;
    staleComputed = computed(() => { const value = computedSource();
      if (value === 2) { staleComputed.dispose(); recycledComputedSlot = signal(100); }
      return value;
    });
    assert.equal(staleComputed(), 1); computedSource.set(2); assert.equal(staleComputed(), 1);
    assert.equal(recycledComputedSlot.peek(), 100); recycledComputedSlot.dispose(); computedSource.dispose();
    assert.equal(getGraphTelemetry().nodes, 0);
    const selfTrigger = signal(0), replacementSource = signal(0); let selfEffect, stopReplacement, lateCleanup = 0;
    const replacementValues = [];
    selfEffect = effect(() => {
      if (selfTrigger() === 1) {
        selfEffect.dispose();
        createRoot(dispose => { stopReplacement = dispose; effect(() => { replacementValues.push(replacementSource()); }); });
        return () => { lateCleanup++; };
      }
    });
    selfTrigger.set(1); assert.equal(lateCleanup, 1);
    replacementSource.set(1); assert.deepEqual(replacementValues, [0, 1]);
    stopReplacement(); selfTrigger.dispose(); replacementSource.dispose(); assert.equal(getGraphTelemetry().nodes, 0);
    const queuedTrigger = signal(0), queuedReplacementSource = signal(0), queuedValues = [];
    let queuedLeft, queuedRight, queuedReplacement, queuedStop, replacedQueued = false;
    const runQueuedPeer = other => {
      if (queuedTrigger() === 1 && !replacedQueued) {
        replacedQueued = true; const victim = other(); victim.dispose();
        createRoot(dispose => { queuedStop = dispose; queuedReplacement = effect(() => { queuedValues.push(queuedReplacementSource()); }); });
        assert.equal(queuedReplacement.id, victim.id);
      }
    };
    queuedLeft = effect(() => runQueuedPeer(() => queuedRight)); queuedRight = effect(() => runQueuedPeer(() => queuedLeft));
    batch(() => queuedTrigger.set(1)); assert.deepEqual(queuedValues, [0]);
    queuedReplacementSource.set(1); assert.deepEqual(queuedValues, [0, 1]);
    queuedLeft.dispose(); queuedRight.dispose(); queuedStop(); queuedTrigger.dispose(); queuedReplacementSource.dispose();
    assert.equal(getGraphTelemetry().nodes, 0);
    const stabilizationSource = signal(0), stabilizationDerived = computed(() => stabilizationSource() * 2), stabilizedValues = [];
    const stabilizer = effect(() => { const value = stabilizationDerived() / 2; stabilizedValues.push(value); if (value < 3) stabilizationSource.set(value + 1); });
    assert.deepEqual(stabilizedValues, [0,1,2,3]); stabilizer.dispose(); stabilizationDerived.dispose(); stabilizationSource.dispose();
    const runaway = signal(0);
    assert.throws(() => effect(() => { runaway.set(runaway() + 1); }), error => error instanceof ReactiveStabilizationError && error.iterations === 100);
    assert.equal(getGraphTelemetry().nodes, 1); runaway.dispose();
    const pendingBirthSource = signal(0), pendingBirthGate = signal(0), pendingNext = signal(0);
    const pendingValues = [], pendingPeerValues = [];
    const pendingOld = effect(() => { pendingBirthSource(); pendingBirthGate(); });
    const pendingPeer = effect(() => { pendingPeerValues.push(pendingNext()); }); let pendingFresh;
    batch(() => {
      pendingBirthSource.set(1); pendingOld.dispose();
      pendingFresh = effect(() => { pendingValues.push(pendingNext()); }); assert.equal(pendingFresh.id, pendingOld.id);
      pendingNext.set(1); pendingNext.set(2);
      assert.equal(getWasmCore().svet_pending_effect_count(),2);
    });
    assert.deepEqual(pendingValues,[0,2]); assert.deepEqual(pendingPeerValues,[0,2]);
    pendingNext.set(3); assert.deepEqual(pendingValues,[0,2,3]); assert.deepEqual(pendingPeerValues,[0,2,3]);
    pendingFresh.dispose(); pendingPeer.dispose(); pendingBirthSource.dispose(); pendingBirthGate.dispose(); pendingNext.dispose();
    assert.equal(getGraphTelemetry().nodes,0);
    const cyclicInput = signal(false); let cyclicComputed;
    cyclicComputed = computed(() => cyclicInput() ? cyclicComputed() : 7);
    const cyclicValues = [], cyclicPeerValues = [], cyclicObserver = effect(() => { cyclicValues.push(cyclicComputed()); });
    const cyclicPeer = effect(() => { cyclicPeerValues.push(cyclicComputed()); });
    assert.throws(() => cyclicInput.set(true), error => error instanceof ReactiveCycleError &&
      Object.isFrozen(error.nodeIds) && error.nodeIds.length === 2 && error.nodeIds[0] === cyclicComputed.id);
    cyclicInput.set(false); assert.deepEqual(cyclicValues,[7,7]); assert.deepEqual(cyclicPeerValues,[7,7]);
    cyclicPeer.dispose(); cyclicObserver.dispose(); cyclicComputed.dispose(); cyclicInput.dispose(); assert.equal(getGraphTelemetry().nodes,0);
    const memorySource = signal(0), memoryGate = signal(0);
    batch(() => {
      for (let i = 0; i < 1000; i++) {
        const transient = effect(() => { memorySource(); memoryGate(); });
        memorySource.set(i + 1); transient.dispose();
      }
      const pendingMemory = getGraphTelemetry();
      assert.equal(pendingMemory.nodes,2); assert.equal(pendingMemory.pendingEffectCount,0);
      assert.ok(pendingMemory.pendingEffectEntryCount <= 68);
    });
    assert.equal(getGraphTelemetry().pendingEffectEntryCount,0); memorySource.dispose(); memoryGate.dispose();
    const cursorCore = createWasmCoreInstance(), cursorA = cursorCore.svet_create_node(0), cursorB = cursorCore.svet_create_node(0), cursorObserver = cursorCore.svet_create_node(2);
    cursorCore.svet_begin_tracking(cursorObserver); cursorCore.svet_track_read(cursorA); cursorCore.svet_track_read(cursorB); cursorCore.svet_end_tracking();
    assert.equal(cursorCore.svet_trigger_and_flush(cursorB),cursorObserver+1); cursorCore.svet_set_state(cursorObserver,0);
    cursorCore.svet_begin_tracking(cursorObserver); cursorCore.svet_track_read(cursorB); cursorCore.svet_track_read(cursorA); cursorCore.svet_end_tracking();
    assert.equal(cursorCore.svet_trigger_and_flush(cursorB),cursorObserver+1); cursorCore.svet_set_state(cursorObserver,0);
    cursorCore.svet_begin_tracking(cursorObserver); cursorCore.svet_track_read(cursorObserver); cursorCore.svet_track_read(0xffffffff); cursorCore.svet_track_read(cursorA); cursorCore.svet_end_tracking();
    assert.equal(cursorCore.svet_trigger_and_flush(cursorB),0);
    cursorCore.svet_begin_tracking(cursorObserver); cursorCore.svet_track_read(cursorA); cursorCore.svet_dispose_node(cursorObserver);
    const cursorFresh = cursorCore.svet_create_node(2); assert.equal(cursorFresh,cursorObserver); assert.equal(cursorCore.svet_is_tracking(),0);
    cursorCore.svet_track_read(cursorB); cursorCore.svet_end_tracking(); assert.equal(cursorCore.svet_get_state(cursorFresh),2);
    cursorCore.svet_begin_tracking(cursorFresh); cursorCore.svet_track_read(cursorB); cursorCore.svet_end_tracking();
    assert.equal(cursorCore.svet_trigger_and_flush(cursorA),0); assert.equal(cursorCore.svet_trigger_and_flush(cursorB),cursorFresh+1);
    cursorCore.svet_dispose_node(cursorFresh); cursorCore.svet_dispose_node(cursorA); cursorCore.svet_dispose_node(cursorB); assert.equal(cursorCore.svet_get_node_count(),0);
    await runWithIsolatedContextAsync(async () => {
      createRoot(() => {
        runWithOwnerScope([], () => resetReactiveSystem());
        assert.equal(getCurrentOwner(), null);
      });
      batch(() => resetReactiveSystem());
      const resetSource = signal(0), resetValues = [];
      const resetObserver = effect(() => { resetValues.push(resetSource()); });
      batch(() => { resetSource.set(1); resetSource.set(2); assert.deepEqual(resetValues, [0]); });
      assert.deepEqual(resetValues, [0,2]); resetObserver.dispose(); resetSource.dispose();
    });

    await runWithIsolatedContextAsync(async () => {
      const navigatorDescriptor = Object.getOwnPropertyDescriptor(globalThis, 'navigator');
      let destroyed = 0;
      Object.defineProperty(globalThis, 'navigator', {configurable: true, value: {gpu: {
        requestAdapter: async () => ({info: {}, requestDevice: async () => ({queue: {}, destroy() { destroyed++; throw new Error('consumer teardown'); }})})
      }}});
      try {
        assert.equal((await initWebGpuHardware()).webgpuAvailable, false);
        assert.equal(destroyed, 1);
      } finally {
        if (navigatorDescriptor) Object.defineProperty(globalThis, 'navigator', navigatorDescriptor);
        else delete globalThis.navigator;
      }
    });

    await assert.rejects(createRoot(stop => { stop(); return initWebGpuHardware(); }), /owner disposed/);

    const invalidConfigBefore = getGpuTelemetry();
    assert.throws(() => configureGpuSubsystem({architecture: 'invalid'}), /architecture/);
    assert.throws(() => configureGpuSubsystem({get architecture() { throw new Error('consumer config getter'); }}), /consumer config getter/);
    assert.deepEqual(getGpuTelemetry(), invalidConfigBefore);

    const disposedConfigBefore = getGpuTelemetry();
    createRoot(stop => {
      stop(); let invoked = false;
      assert.throws(() => runWithOwnerScope([], () => { invoked = true; }), /disposed owner/);
      assert.equal(invoked, false);
      assert.throws(() => configureGpuSubsystem({architecture: 'UMA'}), /owner disposed/);
    });
    assert.deepEqual(getGpuTelemetry(), disposedConfigBefore);

    const encodedRoute = {path: '/café/:__proto__', component() { throw new Error('type-only component'); }};
    const encodedRouter = createRouter({routes: [encodedRoute], initialPath: '/'});
    encodedRouter.navigate('/caf%C3%A9/value');
    assert.equal(encodedRouter.match(encodedRouter.path()).route, encodedRoute);
    assert.equal(Object.hasOwn(encodedRouter.params(), '__proto__'), true);
    assert.equal(encodedRouter.params().__proto__, 'value');
    assert.equal(Object.getPrototypeOf(encodedRouter.params()), Object.prototype); encodedRouter.dispose();

    const normalizedRouter = createRouter({routes: [], initialPath: '/dir/page?old=yes'});
    assert.equal(normalizedRouter.navigate('../next?q=value#part'), true);
    assert.equal(normalizedRouter.path(), '/next'); assert.deepEqual(normalizedRouter.query(), {q: 'value'}); assert.equal(normalizedRouter.hash(), 'part');
    assert.equal(normalizedRouter.navigate('https://external.invalid/path'), false);
    normalizedRouter.dispose();

    const routerBaseline = getGraphTelemetry().nodes;
    const lifetimeRouter = createRouter({routes: [], initialPath: '/initial'});
    lifetimeRouter.beforeEach(to => { if (to.path === '/outer') lifetimeRouter.navigate('/newer'); });
    assert.equal(lifetimeRouter.navigate('/outer'), false); assert.equal(lifetimeRouter.path(), '/newer');
    lifetimeRouter.dispose(); lifetimeRouter.dispose();
    assert.equal(lifetimeRouter.navigate('/retired'), false); assert.equal(getGraphTelemetry().nodes, routerBaseline);

    const failedBoundaryBaseline = getGraphTelemetry().nodes;
    assert.throws(() => For({each: [1], children() { throw new Error('unused row'); }}), /document/);
    assert.throws(() => Portal({children: 'content'}), /document/);
    assert.throws(() => Suspense({children: 'content', fallback: 'waiting'}), /document/);
    assert.throws(() => ErrorBoundary({children: 'content', fallback() { throw new Error('unused fallback'); }}), /document/);
    assert.equal(getGraphTelemetry().nodes, failedBoundaryBaseline);

    const cancelledReplies = [];
    const actionNodeBaseline = getGraphTelemetry().nodes;
    let actionAbortSignal;
    const cancellableAction = createServerAction({ rpc: (_args, abort) => {
      actionAbortSignal = abort; return new Promise(() => {});
    }});
    const cancelledActionResult = cancellableAction(); cancellableAction.dispose();
    await assert.rejects(cancelledActionResult, ServerActionDisposedError);
    await assert.rejects(cancellableAction(), ServerActionDisposedError);
    assert.equal(actionAbortSignal.aborted, true);
    assert.equal(cancellableAction.pending(), false);
    assert.equal(getGraphTelemetry().nodes, actionNodeBaseline);
    const cancelledResource = createResource(() => new Promise(resolve => { cancelledReplies.push(resolve); }));
    const cancelledRefetch = cancelledResource.refetch(); cancelledResource.abort();
    assert.equal(await cancelledRefetch, undefined);
    assert.equal(cancelledResource.loading(), false);
    cancelledReplies.forEach(resolve => resolve('obsolete'));
    await Promise.resolve();
    assert.equal(cancelledResource(), undefined); cancelledResource.dispose();
    let rejectResourceSource = false; const refetchSourceFailure = new Error('consumer refetch source');
    const sourceFailureResource = createResource(() => { if (rejectResourceSource) throw refetchSourceFailure; return 1; }, async () => 1);
    rejectResourceSource = true;
    await assert.rejects(sourceFailureResource.refetch(), error => error === refetchSourceFailure);
    sourceFailureResource.dispose();

    const resourceSource = signal(1), resourceBaseline = getGraphTelemetry().nodes; let resourceCalls = 0;
    const ownedResource = createResource(resourceSource, async value => { resourceCalls++; return value; });
    ownedResource.dispose(); ownedResource.dispose();
    assert.equal(getGraphTelemetry().nodes, resourceBaseline);
    assert.equal(await ownedResource.refetch(), undefined);
    resourceSource.set(2); assert.equal(resourceCalls, 1); assert.equal(ownedResource.loading(), false);
    resourceSource.dispose();

    const failedBatchSource = signal(0), failedBatchBody = new Error('consumer batch body');
    const failedBatchObserver = effect(() => { if (failedBatchSource() === 1) throw new Error('consumer observer'); });
    assert.throws(() => batch(() => { failedBatchSource.set(1); throw failedBatchBody; }), error => error === failedBatchBody);
    batch(() => failedBatchSource.set(2)); failedBatchObserver.dispose(); failedBatchSource.dispose();

    let deepScopeCleanup = 0;
    createRoot(stop => {
      const core = getWasmCore(); let parent = getCurrentOwner();
      for (let i = 0; i < 12000; i++) runWithCapturedOwner(core, parent, () => runWithOwnerScope([], () => {
        parent = getCurrentOwner(); onCleanup(() => { deepScopeCleanup++; });
      }));
      stop();
    });
    assert.equal(deepScopeCleanup, 12000);

    const untrackedSource = signal(0), untrackedValues = [];
    const untrackedObserver = effect(() => {
      const value = untrackedSource(); untrackedValues.push(value);
      if (value < 3) untrack(() => untrackedSource.set(value+1));
    });
    assert.deepEqual(untrackedValues, [0,1,2,3]); untrackedObserver.dispose(); untrackedSource.dispose();

    const cascadeSources = Array.from({length: 401}, () => signal(0));
    let cascadeDepth = 0, cascadePeak = 0;
    const cascadeObservers = cascadeSources.slice(0,-1).map((source,i) => effect(() => {
      cascadeDepth++; cascadePeak = Math.max(cascadePeak, cascadeDepth);
      try { const value = source(); if (value > 0) cascadeSources[i+1].set(value+1); }
      finally { cascadeDepth--; }
    }));
    cascadeSources[0].set(1);
    assert.equal(cascadeSources.at(-1).peek(), 401); assert.equal(cascadePeak, 1);
    cascadeObservers.forEach(observer => observer.dispose()); cascadeSources.forEach(source => source.dispose());

    const failedScopeSource = signal(0), failedScopeBaseline = getGraphTelemetry().nodes;
    let failedScopeRuns = 0, failedScopeCleanups = 0;
    assert.throws(() => createRoot(() => {
      onCleanup(() => { failedScopeCleanups++; });
      const failedValue = computed(() => failedScopeSource());
      createGpuTensor([1,2]); effect(() => { failedValue(); failedScopeRuns++; });
      throw new Error('consumer setup failed');
    }), /consumer setup failed/);
    failedScopeSource.set(1);
    assert.equal(failedScopeRuns, 1); assert.equal(failedScopeCleanups, 1);
    assert.equal(getGraphTelemetry().nodes, failedScopeBaseline);
    failedScopeSource.dispose();

    // Recycled numeric source slots must not carry old subscriptions.
    const lifetimeGate = signal(0), retiredSource = signal(5);
    let lifetimeSource = retiredSource;
    const lifetimeValues = [];
    const lifetimeObserver = effect(() => { lifetimeGate(); lifetimeValues.push(lifetimeSource()); });
    retiredSource.dispose(); lifetimeSource = signal(100);
    assert.equal(lifetimeSource.id, retiredSource.id);
    lifetimeGate.set(1); lifetimeSource.set(101);
    assert.deepEqual(lifetimeValues, [5, 100, 101]);
    lifetimeObserver.dispose(); lifetimeSource.dispose(); lifetimeGate.dispose();
    const stagedSource = signal(5); let stagedReplacement, stagedRuns = 0;
    const stagedObserver = effect(() => {
      stagedSource(); stagedRuns++;
      if (!stagedReplacement) { stagedSource.dispose(); stagedReplacement = signal(100); }
    });
    stagedReplacement.set(101); assert.equal(stagedRuns, 1);
    stagedObserver.dispose(); stagedReplacement.dispose();

    const tensor = createGpuTensor([2, 3]), computedTensor = createGpuTensor(2);
    const compute = await dispatchGpuComputeKernelAsync({ wgsl: '', inputs: [tensor], output: computedTensor,
      async fallbackKernel([values], result) { await Promise.resolve(); result.set(values.map(value => value * 2)); }
    });
    assert.equal(compute.backend, 'fallback'); assert.equal(compute.uploadedBytes, 0);
    const gpuStats = getGpuTelemetry();
    assert.equal(gpuStats.pendingComputeCount, 0); assert.equal(gpuStats.pendingComputeSnapshotBytes, 0);
    assert.equal(gpuStats.maxPendingComputeCount, 32); assert.equal(gpuStats.maxPendingComputeBytes, 256 * 1024 * 1024);
    assert.deepEqual(Array.from(computedTensor()), [4, 6]);
    assert.throws(() => dispatchGpuComputeKernel({ wgsl: '', inputs: [tensor], output: computedTensor,
      fallbackKernel(_, values) { values[0] = 99; throw new Error('consumer sync failure'); }
    }), /consumer sync failure/);
    assert.deepEqual(Array.from(computedTensor()), [4, 6]);
    dispatchGpuComputeKernel({ wgsl: '', inputs: [tensor], output: computedTensor,
      fallbackKernel([values], result) { result.set(values); }
    });
    assert.deepEqual(Array.from(computedTensor()), [2, 3]); tensor.dispose(); computedTensor.dispose();
    assert.equal(getGraphTelemetry().nodes, 0);
    let stopScene;
    createRoot(dispose => {
      stopScene = dispose;
      const transform = createTransform3D(), camera = createPerspectiveCamera3D();
      camera.setProjection({ near: 2000, far: 3000 });
      assert.equal(camera.near.peek(), 2000); assert.equal(camera.far.peek(), 3000);
      assert.throws(() => camera.aspect.set(0), /projection parameters/);
      assert.ok(Array.from(camera.projectionMatrix.peek()).every(Number.isFinite));
      createMesh3D({ positions: [1, 2, -3], transform, camera });
    });
    assert.ok(getGraphTelemetry().nodes > 0); stopScene(); assert.equal(getGraphTelemetry().nodes, 0);
    const noAllocation = () => { throw new Error('capacity rejection must precede GPU work'); };
    configureGpuSubsystem({ device: { queue: { writeBuffer: noAllocation, submit: noAllocation },
      createComputePipeline: noAllocation, createShaderModule: noAllocation, createBindGroup: noAllocation,
      createBuffer: noAllocation, createCommandEncoder: noAllocation }, maxPendingComputeBytes: 0 });
    const rejectedInput = createGpuTensor([1]), rejectedOutput = createGpuTensor([0]);
    await assert.rejects(dispatchGpuComputeKernelAsync({ wgsl: '', inputs: [rejectedInput], output: rejectedOutput }),
      error => error instanceof GpuComputeCapacityError && error.reason === 'snapshot-bytes' &&
        error.requestedSnapshotBytes === 8 && error.queue.maxPendingComputeBytes === 0);
    rejectedInput.dispose(); rejectedOutput.dispose(); configureGpuSubsystem({ device: null });
    assert.ok(transformSvetJsx('const node = <p>{count()}</p>').includes('() => (count())'));
    const source = 'component ConsumerCard { state count: Number = 1; Text { text: state.count; } }';
    const compiled = compileFormaToJsModule(source);
    assert.ok(compiled.includes('from "svet-engine"'));
    const { writeFile } = await import('node:fs/promises');
    await writeFile(new URL('./compiled-forma.mjs', import.meta.url), compiled);
    const forma = await import('./compiled-forma.mjs');
    assert.equal(typeof forma.default, 'function');
    assert.equal(forma.uiValidation.valid, true);
    assert.ok(svetVitePlugin().transform(source, '/ConsumerCard.ui').code.includes('from "svet-engine"'));
    const actionCallerCore = createWasmCoreInstance(), actionLocalSource = signal(0);
    const actionForeignSource = withWasmCore(actionCallerCore, () => signal(0));
    let actionOuterRuns = 0, actionInnerRuns = 0, actionInner, isolatedActionWait, isolatedThenReads = 0;
    const isolatedAction = createServerAction({rpc: () => ({get then() {
      isolatedThenReads++; actionLocalSource(); actionForeignSource(); return resolve => resolve(7);
    }})});
    const actionOuter = effect(() => {
      actionOuterRuns++;
      withWasmCore(actionCallerCore, () => { actionInner = effect(() => { actionInnerRuns++; isolatedActionWait = isolatedAction(); }); });
    });
    actionLocalSource.set(1); actionForeignSource.set(1);
    assert.deepEqual([actionOuterRuns, actionInnerRuns], [1, 1]);
    assert.equal(await isolatedActionWait, 7); assert.equal(isolatedThenReads, 1);
    actionOuter.dispose(); actionInner.dispose(); isolatedAction.dispose(); actionLocalSource.dispose(); actionForeignSource.dispose();
    assert.equal(withWasmCore(actionCallerCore, () => getGraphTelemetry().nodes), 0);

    const adoptionCore = createWasmCoreInstance();
    let adoptionGetterCore, adoptionMethodCore, adoptionNestedCore, adoptionReceiver;
    const nestedAdoption = {then(resolve) { adoptionNestedCore = getWasmCore(); resolve(9); }};
    const adoptionOperation = Object.freeze({get then() {
      adoptionGetterCore = getWasmCore();
      return function(resolve) { adoptionReceiver = this; adoptionMethodCore = getWasmCore(); resolve(nestedAdoption); };
    }});
    const adoptionAction = withWasmCore(adoptionCore, () => createServerAction({rpc: () => adoptionOperation}));
    assert.equal(await adoptionAction(), 9);
    assert.equal(adoptionGetterCore, adoptionCore); assert.equal(adoptionMethodCore, adoptionCore);
    assert.equal(adoptionNestedCore, adoptionCore); assert.equal(adoptionReceiver, adoptionOperation);
    adoptionAction.dispose(); assert.equal(withWasmCore(adoptionCore, () => getGraphTelemetry().nodes), 0);

    const resourceCore = createWasmCoreInstance(), resourceCores = [], resourceAbortCores = [];
    let resolveCoreResource;
    const coreResourcePromise = new Promise(resolve => { resolveCoreResource = resolve; });
    const coreResource = withWasmCore(resourceCore, () => createResource(ctx => {
      resourceCores.push(getWasmCore());
      ctx.signal.addEventListener('abort', () => resourceAbortCores.push(getWasmCore()));
      return coreResourcePromise;
    }));
    const coreResourceSeen = [];
    const coreResourceObserver = withWasmCore(resourceCore, () => effect(() => {
      coreResourceSeen.push([coreResource(), coreResource.loading()]);
    }));
    const coreResourceWait = coreResource.refetch(); resolveCoreResource('owned');
    assert.equal(await coreResourceWait, 'owned');
    assert.deepEqual(coreResourceSeen, [[undefined, true], ['owned', false]]);
    assert.equal(resourceCores.every(core => core === resourceCore), true);
    assert.equal(resourceAbortCores.length, 1);
    assert.equal(resourceAbortCores[0], resourceCore);
    coreResourceObserver.dispose(); coreResource.dispose();
    assert.equal(withWasmCore(resourceCore, () => getGraphTelemetry().nodes), 0);

    let resetResourceSignal, resetReplacement;
    const resetResource = createResource(ctx => {
      resetResourceSignal = ctx.signal;
      ctx.signal.addEventListener('abort', () => { resetReplacement = signal(17); });
      return new Promise(() => {});
    });
    const resetResourceWait = resetResource.refetch();
    let resetActionSignal;
    const resetAction = createServerAction({rpc: (_args, signal) => { resetActionSignal = signal; return new Promise(() => {}); }});
    const resetActionWait = resetAction(); void resetActionWait.catch(() => {});
    resetReactiveSystem();
    assert.equal(resetActionSignal.aborted, true);
    await assert.rejects(resetActionWait, ServerActionDisposedError); resetAction.dispose();
    assert.equal(resetResourceSignal.aborted, true);
    assert.equal(await resetResourceWait, undefined);
    assert.equal(resetReplacement(), 17); resetResource.dispose();
    resetReplacement.set(18); assert.equal(resetReplacement(), 18);
    resetReplacement.dispose(); assert.equal(getGraphTelemetry().nodes, 0);
    let constructorReplacement, constructorSourceReads = 0;
    assert.throws(() => createResource(() => { constructorSourceReads++; return 1; }, async () => 'stale', {
      get initialValue() { resetReactiveSystem(); constructorReplacement = signal(31); return 'initial'; }
    }), /Resource graph reset during construction/);
    assert.equal(constructorSourceReads, 0); assert.equal(getGraphTelemetry().nodes, 1);
    constructorReplacement.set(32); assert.equal(constructorReplacement(), 32);
    constructorReplacement.dispose(); assert.equal(getGraphTelemetry().nodes, 0);
    const scalarOld = createGpuTensor([7]); let scalarReplacement;
    assert.throws(() => scalarOld.setAt(0, {valueOf() {
      scalarOld.dispose(); scalarReplacement = createGpuTensor([17]); return 99;
    }}), /disposed|invalidated/);
    assert.deepEqual(Array.from(scalarReplacement.peek()), [17]);
    scalarReplacement.dispose(); assert.equal(getGraphTelemetry().nodes, 0);
    for (const mode of ['sync', 'async']) {
      let computeReplacement, staleComputeCalls = 0;
      const staleComputeOptions = {
        get wgsl() { resetReactiveSystem(); computeReplacement = createGpuTensor([17]); return ''; },
        inputs: [], get output() { throw new Error('later output getter must not execute'); },
        fallbackKernel() { staleComputeCalls++; }
      };
      if (mode === 'sync') assert.throws(() => dispatchGpuComputeKernel(staleComputeOptions), /reset/);
      else await assert.rejects(dispatchGpuComputeKernelAsync(staleComputeOptions), /reset/);
      assert.equal(staleComputeCalls, 0); assert.deepEqual(Array.from(computeReplacement.peek()), [17]);
      computeReplacement.dispose(); assert.equal(getGraphTelemetry().nodes, 0);
    }

    const fallbackCore = createWasmCoreInstance();
    const fallbackOutput = withWasmCore(fallbackCore, () => createGpuTensor([0]));
    const fallbackSource = withWasmCore(fallbackCore, () => signal(0));
    let stopFallback, fallbackSeenCore, fallbackRuns = 0, fallbackCleanups = 0;
    const fallbackJob = withWasmCore(fallbackCore, () => createRoot(stop => {
      stopFallback = stop;
      return dispatchGpuComputeKernelAsync({wgsl: '', inputs: [], output: fallbackOutput, fallbackKernel(_inputs, result) {
        fallbackSeenCore = getWasmCore();
        effect(() => { fallbackSource(); fallbackRuns++; }); onCleanup(() => { fallbackCleanups++; }); result[0] = 7;
      }});
    }));
    assert.equal((await fallbackJob).backend, 'fallback'); assert.equal(fallbackSeenCore, fallbackCore);
    assert.deepEqual(withWasmCore(fallbackCore, () => Array.from(fallbackOutput.peek())), [7]);
    fallbackSource.set(1); assert.equal(fallbackRuns, 2); stopFallback(); fallbackSource.set(2);
    assert.deepEqual([fallbackRuns, fallbackCleanups], [2, 1]);
    fallbackOutput.dispose(); fallbackSource.dispose();
    assert.equal(withWasmCore(fallbackCore, () => getGraphTelemetry().nodes), 0);
    const tupleFailure = new Error('consumer tuple failure');
    assert.throws(() => createTransform3D({position: {[Symbol.iterator]() { throw tupleFailure; }}}), error => error === tupleFailure);
    assert.equal(getGraphTelemetry().nodes, 0);
    let transformReplacement;
    assert.throws(() => createTransform3D({position: [{valueOf() {
      resetReactiveSystem(); transformReplacement = createGpuTensor(new Float32Array(32).fill(17)); return 1;
    }}, 0, 0]}), /reset|invalidated/);
    assert.deepEqual(Array.from(transformReplacement.peek()), new Array(32).fill(17));
    transformReplacement.dispose(); assert.equal(getGraphTelemetry().nodes, 0);
    const meshDependency = createTransform3D(), meshFailure = new Error('consumer mesh matrix failure');
    const meshBaseline = getGraphTelemetry().nodes;
    const rejectedMatrix = new Proxy(meshDependency.worldMatrix, {apply() { throw meshFailure; }});
    assert.throws(() => createMesh3D({positions: [0,0,0], transform: {...meshDependency, worldMatrix: rejectedMatrix}}), error => error === meshFailure);
    assert.equal(getGraphTelemetry().nodes, meshBaseline); meshDependency.dispose();
    const oldMeshDependency = createTransform3D(); let meshReplacement, meshLaterReads = 0;
    assert.throws(() => createMesh3D({transform: oldMeshDependency, positions: {length: 3,
      get 0() { resetReactiveSystem(); meshReplacement = createGpuTensor([17]); return 0; },
      get 1() { meshLaterReads++; return 0; }, 2: 0
    }}), /reset|invalidated/);
    assert.equal(meshLaterReads, 0); oldMeshDependency.dispose();
    assert.deepEqual(Array.from(meshReplacement.peek()), [17]); meshReplacement.dispose();
    const reentryTopologyTransform = createTransform3D();
    const reentryTopologyMesh = createMesh3D({positions: [0,0,0, 1,0,0, 0,1,0], transform: reentryTopologyTransform});
    let reentryTopologyReads = 0;
    assert.throws(() => reentryTopologyMesh.updateIndices({length: 3,
      get 0() { reentryTopologyMesh.updateIndices([2,1,0], 'uint32'); return 0; },
      get 1() { reentryTopologyReads++; return 1; }, 2: 2
    }, 'uint16'), /topology changed/);
    assert.equal(reentryTopologyReads, 0);
    assert.deepEqual(reentryTopologyMesh.indices, new Uint32Array([2,1,0]));
    reentryTopologyMesh.updateIndices([0,1,2]); assert(reentryTopologyMesh.indices instanceof Uint32Array);
    reentryTopologyMesh.dispose(); reentryTopologyTransform.dispose();
    const arenaLimit = getWasmCore().svet_gpu_get_arena_limit_f32(); assert.equal(arenaLimit, 67_108_864);
    let oversizedTensorReads = 0;
    assert.throws(() => createGpuTensor({length: arenaLimit + 1, get 0() { oversizedTensorReads++; return 0; }}), /Invalid GPU tensor length/);
    assert.equal(oversizedTensorReads, 0);
    const capacityTransform = createTransform3D(), capacityNodes = getGraphTelemetry().nodes;
    const capacityArena = getGpuTelemetry().arenaFloatCount, CapacityFloat32Array = Float32Array;
    let largeMeshCopies = 0, capacityIndexReads = 0;
    globalThis.Float32Array = new Proxy(CapacityFloat32Array, {construct(target, args, receiver) {
      if (typeof args[0] === 'number' && args[0] > 1_000_000) { largeMeshCopies++; throw new Error('large mesh copy attempted'); }
      return Reflect.construct(target, args, receiver);
    }});
    try {
      const vertexCount = Math.floor((arenaLimit - 16) / 8) + 1;
      assert.throws(() => createMesh3D({transform: capacityTransform,
        positions: {length: vertexCount * 3, get 0() { throw new Error('coordinate read attempted'); }},
        indices: {get length() { capacityIndexReads++; return 0; }}
      }), /arena capacity/);
      assert.deepEqual([largeMeshCopies, capacityIndexReads], [0, 0]);
      assert.equal(getGraphTelemetry().nodes, capacityNodes); assert.equal(getGpuTelemetry().arenaFloatCount, capacityArena);
    } finally { globalThis.Float32Array = CapacityFloat32Array; capacityTransform.dispose(); }


    assert.equal(getGraphTelemetry().nodes, 0);






  `);
  node(['runtime.mjs']);
  await writeFile(join(consumer, 'consumer.tsx'), `
    import { createServerAction, ServerActionDisposedError, type ServerAction, For, Link, createResource, type Resource, h, signal, getGraphTelemetry, createGpuTensor, getGpuTelemetry, configureGpuSubsystem, GpuComputeCapacityError, ReactiveStabilizationError, ReactiveCycleError, dispatchGpuComputeKernel, dispatchGpuComputeKernelAsync, type GpuComputeResult, type Signal, Viewport3D, type Canvas3DProps, createMesh3D, type WebGpuDeviceLike, type VertexPickFrame3D } from 'svet-engine';
    import { captureReactiveContext, ReactiveContextDisposedError, type CapturedReactiveContext, mount, createPerspectiveCamera3D, type PerspectiveCamera3DOptions } from 'svet-engine';
    const captureFactory: () => CapturedReactiveContext = captureReactiveContext;
    const contextErrorCode: 'SVET_CONTEXT_DISPOSED' = new ReactiveContextDisposedError().code;
    function capturedTypes(context: CapturedReactiveContext): number {
      const active: boolean = context.active; void active;
      // @ts-expect-error captured validity is readonly
      context.active = false;
      return context.run(() => 17);
    }
    void captureFactory; void contextErrorCode; void capturedTypes;
    import { renderToStringAsync } from 'svet-engine/server';
    import { svetVitePlugin } from 'svet-engine/compiler';
    const shadow: ShadowRoot = document.createElement('div').attachShadow({mode: 'closed'});
    mount(() => h('form', null, h('input', {model: signal('')})), shadow).unmount();
    const typedAction: ServerAction<number, string> = createServerAction({
      rpc: async (value, abort) => { const signal: AbortSignal = abort; return String(value) + signal.aborted; },
    }); typedAction.dispose();
    const actionDisposedError: Error = new ServerActionDisposedError(); void actionDisposedError;
    const currentRows: Node = For({each: [{id: 1, label: 'value'}], keyFn: item => item.id,
      children: (_initial, index, current) => {
        const value: {id: number; label: string} = current(); const position: number = index();
        return document.createTextNode(value.label + position);
      }}); void currentRows;
    const linkProps: Parameters<typeof Link>[0] = {to: '/download', target: '_blank', download: 'file.bin', rel: 'noopener'};
    const linkNode: Node = Link(linkProps); void linkNode;
    const resource: Resource<number> = createResource(async () => 1);
    resource.dispose(); const disposedRefetch: Promise<number | undefined> = resource.refetch(); void disposedRefetch;
    const projectionOptions: PerspectiveCamera3DOptions = { near: 2000, far: 3000 };
    createPerspectiveCamera3D().setProjection(projectionOptions);
    const cycleError: Error = new ReactiveCycleError([1,2,1]);
    if (cycleError instanceof ReactiveCycleError) { const nodes: readonly number[] = cycleError.nodeIds; void nodes; }
    const stabilizationError: Error = new ReactiveStabilizationError(null);
    if (stabilizationError instanceof ReactiveStabilizationError) {
      const effectId: number | null = stabilizationError.effectId;
      const iterations: number = stabilizationError.iterations;
      void effectId; void iterations;
    }
    const capacityError: Error = new GpuComputeCapacityError('snapshot-bytes', getGpuTelemetry(), 8);
    if (capacityError instanceof GpuComputeCapacityError) {
      const reason: 'count' | 'snapshot-bytes' = capacityError.reason;
      const requested: number | null = capacityError.requestedSnapshotBytes;
      const pending: number = capacityError.queue.pendingComputeCount;
      void reason; void requested; void pending;
    }
    const nativeQueueStats: number[] = [getGraphTelemetry().pendingEffectCount, getGraphTelemetry().pendingEffectEntryCount];
    void nativeQueueStats;
    const queueStats: number[] = [getGpuTelemetry().pendingComputeCount, getGpuTelemetry().pendingComputeSnapshotBytes,
      getGpuTelemetry().maxPendingComputeCount, getGpuTelemetry().maxPendingComputeBytes];
    void queueStats;
    const count: Signal<number> = signal(1);
    const node: Node = <div title={() => String(count())}>{count()}</div>;
    const result: Promise<{html: string; stateJson: string}> = renderToStringAsync(async () => ({ node, signals: { count } }));
    const plugin = svetVitePlugin();
    const computation: Promise<GpuComputeResult> = dispatchGpuComputeKernelAsync({ wgsl: '', inputs: [], output: createGpuTensor([0]), async fallbackKernel(_, output) { output[0] = 1; } });
    const viewport: (props: Canvas3DProps) => HTMLDivElement = Viewport3D;
    const errorFilter: Parameters<NonNullable<WebGpuDeviceLike['pushErrorScope']>>[0] = 'out-of-memory';
    void errorFilter;
    const limits: NonNullable<WebGpuDeviceLike['limits']> = { maxBufferSize: 1024, maxTextureDimension2D: 256 };
    void limits;
    const indexFormat: NonNullable<Parameters<typeof createMesh3D>[0]['indexFormat']> = 'uint32';
    void indexFormat;
    const meshSource: Canvas3DProps['mesh'] = () => { throw new Error('type-only fixture'); };
    const picker: NonNullable<Canvas3DProps['onVertexPick']> = (_index,_distance,_z,frame: VertexPickFrame3D) => { const id: number = frame.id; void id; };
    const legacyPicker: NonNullable<Canvas3DProps['onVertexPick']> = (_index,_distance,_z) => {};
    void picker; void legacyPicker;
    const renderMode: NonNullable<Canvas3DProps['renderMode']> = 'animation-frame';
    const queueCompletion: NonNullable<WebGpuDeviceLike['queue']['onSubmittedWorkDone']> = async () => {};
    void renderMode; void queueCompletion;
    const pickMode: NonNullable<Canvas3DProps['pickOcclusion']> = 'indexed';
    void pickMode;
    const dimension: NonNullable<Canvas3DProps['width']> = () => 320;
    void h; void result; void plugin; void computation; void viewport; void dimension; void meshSource;
  `);
  await writeFile(join(consumer, 'tsconfig.json'), JSON.stringify({ compilerOptions: { strict: true, noEmit: true, module: 'NodeNext', moduleResolution: 'NodeNext', target: 'ES2022', jsx: 'react-jsx', jsxImportSource: 'svet-engine', skipLibCheck: false, types: [] }, files: ['consumer.tsx'] }));
  node([require.resolve('typescript/bin/tsc'), '-p', 'tsconfig.json']);
  await writeFile(join(consumer, 'negative.ts'), "import { signal } from 'svet-engine'; const value: number = signal('wrong')();");
  const negative = await import('node:child_process').then(({ spawnSync }) => spawnSync(process.execPath, [require.resolve('typescript/bin/tsc'), '--noEmit', '--strict', '--module', 'NodeNext', '--moduleResolution', 'NodeNext', '--target', 'ES2022', 'negative.ts'], { cwd: consumer, encoding: 'utf8' }));
  assert.equal(negative.status, 2);
  assert.match(negative.stdout, /Type 'string' is not assignable to type 'number'/);
  await writeFile(join(consumer, 'index.html'), '<h1>Installed Svet consumer</h1><h2>JSX counter</h2><div id="app"></div><h2>Forma card</h2><div id="forma"></div><script type="module" src="/main.tsx"></script>');
  await writeFile(join(consumer, 'Card.ui'), "component ConsumerCard { state count: Number = 1; Column { Text { text: state.count; } Button { text: 'Forma +1'; clicked -> state.increment('count', 1); } } }");
  await writeFile(join(consumer, 'main.tsx'), `import { mount, signal } from 'svet-engine'; import createCard from './Card.ui'; const count = signal(1); mount(() => <button onClick={() => count.update(n => n + 1)}>Count: {count()}</button>, document.querySelector('#app')); createCard({}).mount(document.querySelector('#forma'));`);
  await writeFile(join(consumer, 'vite.config.mjs'), `import { svetVitePlugin } from 'svet-engine/compiler'; export default {plugins: [svetVitePlugin()], oxc: {jsx: {runtime: 'automatic', importSource: 'svet-engine'}}};`);
  node([resolve(root, 'node_modules/vite/bin/vite.js'), 'build']);
  const assets = await readdir(join(consumer, 'dist/assets'));
  const javascript = await Promise.all(assets.filter(file => file.endsWith('.js')).map(file => readFile(join(consumer, 'dist/assets', file), 'utf8')));
  assert(javascript.length > 0);
  assert(javascript.every(code => !code.includes('node:async_hooks')), 'Node SSR code leaked into browser output');
  passed = true;
  const report = { node: process.version, platform: process.platform, arch: process.arch, completedAt: new Date().toISOString(), packageFiles: packed.entryCount, unpackedBytes: packed.unpackedSize, consumerDirectory: keep ? consumer : undefined, checks: ['archive allowlist', 'isolated installation', 'native Node ESM exports', 'concurrent SSR', 'sync and async compute fallback', 'compiled Forma import', 'strict JSX declarations without Vite', 'negative type fixture', 'external JSX/Forma browser build'] };
  await writeFile(join(root, 'dist/package-verification.json'), JSON.stringify(report, null, 2) + '\n');
  console.log(JSON.stringify(report, null, 2));
} finally {
  if (passed && !keep) await rm(temporary, { recursive: true, force: true });
  else console.error(`Consumer verification evidence retained at ${temporary}`);
}
