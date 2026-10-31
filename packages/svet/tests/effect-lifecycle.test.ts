import { beforeEach, expect, it } from 'vitest';
import { runWithOwnerScope, createRoot, effect, computed, batch, createGpuTensor, getCurrentOwner, getWasmCore, getGraphTelemetry, onCleanup, resetReactiveSystem, signal } from '../src/engine';

beforeEach(() => resetReactiveSystem());

it('a disposed handle cannot dispose a later effect that reuses its pooled entry', () => {
  const source = signal(0);
  const first = effect(() => { source(); });
  const firstId = first.id;
  first.dispose();
  const values: number[] = [];
  const second = effect(() => { values.push(source()); });
  expect(second.id).toBe(firstId); // Exercise actual slot reuse.
  first.dispose();
  source.set(1);
  expect(values).toEqual([0, 1]);
  second.dispose();
});

it('owner cleanup cannot dispose an unrelated replacement of a manually disposed effect', () => {
  const source = signal(0);
  let stop!: () => void;
  createRoot(dispose => {
    stop = dispose;
    const handle = effect(() => { source(); });
    handle.dispose();
  });
  const values: number[] = [];
  const replacement = effect(() => { values.push(source()); });
  stop();
  source.set(1);
  expect(values).toEqual([0, 1]);
  replacement.dispose();
});

it('keeps public handles distinct and IDs stable across slot reuse', () => {
  const first = effect(() => {});
  const id = first.id;
  first.dispose();
  const second = effect(() => {});
  expect(first).not.toBe(second);
  expect(first.id).toBe(id);
  expect(second.id).toBe(id);
  second.dispose();
});

it('stale disposal neither runs replacement cleanup nor stops a WASM-linked effect', () => {
  const a = signal(0);
  const b = signal(0);
  const first = effect(() => { a(); b(); });
  first.dispose();
  let cleanups = 0;
  const values: number[] = [];
  const second = effect(() => {
    values.push(a() + b());
    return () => { cleanups++; };
  });
  first.dispose();
  expect(cleanups).toBe(0);
  a.set(1);
  expect(values).toEqual([0, 1]);
  expect(cleanups).toBe(1);
  second.dispose();
  second.dispose();
  expect(cleanups).toBe(2);
  b.set(1);
  expect(values).toEqual([0, 1]);
});

it('cleans up nested effects before a top-level effect reruns or is disposed', () => {
  const trigger = signal(0), source = signal(0);
  const values: number[] = [];
  const outer = effect(() => {
    trigger();
    effect(() => { values.push(source()); });
  });
  trigger.set(1);
  source.set(1);
  expect(values).toEqual([0, 0, 1]);
  outer.dispose();
  source.set(2);
  expect(values).toEqual([0, 0, 1]);
});

it('runs onCleanup for top-level effects on rerun and final disposal', () => {
  const source = signal(0);
  const cleaned: number[] = [];
  const handle = effect(() => {
    const value = source();
    onCleanup(() => { cleaned.push(value); });
  });
  source.set(1);
  expect(cleaned).toEqual([0]);
  handle.dispose();
  expect(cleaned).toEqual([0, 1]);
});

it('releases a failed initial effect and its nested subscriptions', () => {
  const source = signal(0);
  const baseline = getGraphTelemetry().nodes;
  let nestedRuns = 0;
  expect(() => effect(() => {
    effect(() => { source(); nestedRuns++; });
    throw new Error('initial failure');
  })).toThrow('initial failure');
  source.set(1);
  expect(nestedRuns).toBe(1);
  expect(getGraphTelemetry().nodes).toBe(baseline);
});

it('releases graph nodes even if the returned cleanup throws during disposal', () => {
  const source = signal(0);
  const baseline = getGraphTelemetry().nodes;
  const handle = effect(() => {
    source();
    return () => { throw new Error('cleanup failure'); };
  });
  expect(() => handle.dispose()).toThrow('cleanup failure');
  expect(getGraphTelemetry().nodes).toBe(baseline);
  expect(() => handle.dispose()).not.toThrow();
});

it('manual computed/effect churn does not retain owner cleanup callbacks or child scopes', () => {
  createRoot(() => {
    const owner = getCurrentOwner()!, source = signal(1), baseline = owner.disposables.length;
    const live = effect(() => { source(); });
    for (let i = 0; i < 1000; i++) {
      const value = computed(() => source() + i), observer = effect(() => { value(); });
      observer.dispose(); value.dispose(); observer.dispose(); value.dispose();
    }
    expect(owner.disposables).toHaveLength(baseline + 1); expect(owner.children?.size).toBe(1);
    live.dispose(); expect(owner.disposables).toHaveLength(baseline); expect(owner.children?.size).toBe(0);
    source.dispose();
  });
});

it('failed effect initialization releases its parent cleanup registration', () => {
  createRoot(() => {
    const owner = getCurrentOwner()!, baseline = owner.disposables.length, nodes = getGraphTelemetry().nodes;
    for (let i = 0; i < 100; i++) expect(() => effect(() => { throw new Error('init failed'); })).toThrow('init failed');
    expect(owner.disposables).toHaveLength(baseline); expect(owner.children?.size).toBe(0);
    expect(getGraphTelemetry().nodes).toBe(nodes);
  });
});

it('effect cleanup runs once when a callback manually disposes another owned resource', () => {
  const source = signal(0); let cleanups = 0;
  const observer = effect(() => {
    source(); const value = computed(() => 1);
    onCleanup(() => { cleanups++; value.dispose(); });
  });
  source.set(1); expect(cleanups).toBe(1);
  observer.dispose(); expect(cleanups).toBe(2); source.dispose();
});

it('self-disposal cannot let an in-flight callback corrupt a replacement effect', () => {
  const trigger = signal(0), replacementSource = signal(0), tail = signal(0);
  let old!: ReturnType<typeof effect>, replacement!: ReturnType<typeof effect>, stopReplacement!: () => void;
  let lateCleanups = 0; const values: number[] = [];
  old = effect(() => {
    if (trigger() === 1) {
      old.dispose();
      createRoot(dispose => { stopReplacement = dispose; replacement = effect(() => { values.push(replacementSource()); }); });
      tail(); return () => { lateCleanups++; };
    }
  });
  trigger.set(1); expect(lateCleanups).toBe(1);
  replacementSource.set(1); tail.set(1); trigger.set(2);
  expect(values).toEqual([0, 1]); old.dispose(); replacementSource.set(2); expect(values).toEqual([0, 1, 2]);
  replacement.dispose(); stopReplacement(); trigger.dispose(); replacementSource.dispose(); tail.dispose();
});

it('disposal from previous-run cleanup prevents the next effect callback and runs cleanup once', () => {
  const trigger = signal(0); let handle!: ReturnType<typeof effect>, runs = 0, cleanups = 0;
  handle = effect(() => { trigger(); runs++; return () => { cleanups++; handle.dispose(); }; });
  trigger.set(1); expect(runs).toBe(1); expect(cleanups).toBe(1); trigger.set(2); expect(runs).toBe(1);
  trigger.dispose();
});

it('late cleanup registration is tied to its original run and cannot overwrite a reused effect', () => {
  const source = signal(0); let oldRegister!: (fn: () => void) => void, cleanups = 0;
  const old = effect(register => { source(); oldRegister ??= register; });
  source.set(1); oldRegister(() => { cleanups++; }); expect(cleanups).toBe(1);
  old.dispose();
  let replacementCleanups = 0;
  const replacement = effect(() => { source(); return () => { replacementCleanups++; }; });
  expect(replacement.id).toBe(old.id);
  oldRegister(() => { cleanups++; }); expect(cleanups).toBe(2); expect(replacementCleanups).toBe(0);
  source.set(2); expect(replacementCleanups).toBe(1);
  replacement.dispose(); expect(replacementCleanups).toBe(2); source.dispose();
});

it('throwing cleanup returned after self-disposal still releases the executing shell safely', () => {
  const source = signal(0); let old!: ReturnType<typeof effect>, cleanups = 0;
  old = effect(() => {
    if (source() === 1) { old.dispose(); return () => { cleanups++; throw new Error('late cleanup failed'); }; }
  });
  expect(() => source.set(1)).toThrow('late cleanup failed'); expect(cleanups).toBe(1);
  const values: number[] = [], replacement = effect(() => { values.push(source()); });
  old.dispose(); source.set(2); expect(values).toEqual([1, 2]);
  replacement.dispose(); source.dispose(); expect(getGraphTelemetry().nodes).toBe(0);
});

it('cannot attach new effects or computed nodes to an owner disposed during its callback', () => {
  const source = signal(0); let handle!: ReturnType<typeof effect>;
  handle = effect(() => {
    if (source() === 1) {
      handle.dispose();
      expect(() => effect(() => {})).toThrow('disposed owner');
      expect(() => computed(() => 1)).toThrow('disposed owner');
    }
  });
  source.set(1); source.dispose(); expect(getGraphTelemetry().nodes).toBe(0);
});

it.each(['scalar', 'batch', 'tensor'] as const)('%s flush does not execute a replacement using an already queued effect ID', mode => {
  const source = mode === 'tensor' ? createGpuTensor([0]) : signal(0), replacementSource = signal(0);
  const read = () => mode === 'tensor' ? (source as ReturnType<typeof createGpuTensor>)()[0] : (source as ReturnType<typeof signal<number>>)();
  let first!: ReturnType<typeof effect>, second!: ReturnType<typeof effect>, replacement!: ReturnType<typeof effect>, stopReplacement!: () => void;
  let replaced = false; const values: number[] = [];
  const run = (other: () => ReturnType<typeof effect>) => {
    if (read() === 1 && !replaced) {
      replaced = true; const victim = other(); victim.dispose();
      createRoot(dispose => { stopReplacement = dispose; replacement = effect(() => { values.push(replacementSource()); }); });
      expect(replacement.id).toBe(victim.id);
    }
  };
  first = effect(() => run(() => second)); second = effect(() => run(() => first));
  const write = () => mode === 'tensor' ? (source as ReturnType<typeof createGpuTensor>).setAt(0, 1) : (source as ReturnType<typeof signal<number>>).set(1);
  if (mode === 'batch') batch(write); else write();
  expect(replaced).toBe(true); expect(values).toEqual([0]);
  replacementSource.set(1); expect(values).toEqual([0, 1]);
  first.dispose(); second.dispose(); replacement.dispose(); stopReplacement(); source.dispose(); replacementSource.dispose();
  expect(getGraphTelemetry().nodes).toBe(0);
});

it('a replacement still receives genuine writes created during the old queue flush', () => {
  const source = signal(0), nextSource = signal(0); let first!: ReturnType<typeof effect>, second!: ReturnType<typeof effect>, replacement!: ReturnType<typeof effect>, stop!: () => void;
  let changed = false; const values: number[] = [];
  const run = (other: () => ReturnType<typeof effect>) => {
    if (source() === 1 && !changed) {
      changed = true; other().dispose();
      createRoot(dispose => { stop = dispose; replacement = effect(() => { values.push(nextSource()); }); });
      nextSource.set(1);
    }
  };
  first = effect(() => run(() => second)); second = effect(() => run(() => first));
  batch(() => source.set(1)); expect(values).toEqual([0, 1]);
  nextSource.set(2); expect(values).toEqual([0, 1, 2]);
  first.dispose(); second.dispose(); replacement.dispose(); stop(); source.dispose(); nextSource.dispose();
});

it('reset during a queued callback cannot execute new-generation effects using old queue slots', () => {
  const source = signal(0); let first!: ReturnType<typeof effect>, second!: ReturnType<typeof effect>, nextSource!: ReturnType<typeof signal<number>>, replacement!: ReturnType<typeof effect>;
  let reset = false; const values: number[] = [], padding: Array<ReturnType<typeof signal<number>>> = [];
  const run = (other: () => ReturnType<typeof effect>) => {
    if (source() === 1 && !reset) {
      reset = true; const oldQueuedId = other().id; resetReactiveSystem();
      nextSource = signal(0);
      for (let i = 1; i < oldQueuedId; i++) padding.push(signal(0));
      replacement = effect(() => { values.push(nextSource()); }); expect(replacement.id).toBe(oldQueuedId);
    }
  };
  first = effect(() => run(() => second)); second = effect(() => run(() => first));
  batch(() => source.set(1)); expect(values).toEqual([0]);
  first.dispose(); second.dispose(); nextSource.set(1); expect(values).toEqual([0, 1]);
  replacement.dispose(); source.dispose(); nextSource.dispose(); padding.forEach(item => item.dispose());
  expect(getGraphTelemetry().nodes).toBe(0);
});

it('a batch does not execute an unrelated replacement from a stale native pending job', () => {
  const source = signal(0), gate = signal(0), nextSource = signal(0); const values: number[] = [];
  const old = effect(() => { source(); gate(); }); let fresh!: ReturnType<typeof effect>;
  batch(() => {
    source.set(1); old.dispose(); fresh = effect(() => { values.push(nextSource()); });
    expect(fresh.id).toBe(old.id);
  });
  expect(values).toEqual([0]); nextSource.set(1); expect(values).toEqual([0, 1]);
  fresh.dispose(); source.dispose(); gate.dispose(); nextSource.dispose();
});

it('an effect created inside a batch receives subsequent writes before the batch ends', () => {
  const source = signal(0), values: number[] = []; let observer!: ReturnType<typeof effect>;
  batch(() => { observer = effect(() => { values.push(source()); }); source.set(1); });
  expect(values).toEqual([0, 1]); observer.dispose(); source.dispose();
});

it('replacement writes in the same batch retain real scheduling order and coalesce once', () => {
  const source = signal(0), gate = signal(0), peerSource = signal(0), nextSource = signal(0); const values: string[] = [];
  const old = effect(() => { source(); gate(); });
  const peer = effect(() => { values.push('peer:' + peerSource()); gate(); }); values.length = 0;
  let fresh!: ReturnType<typeof effect>;
  batch(() => {
    source.set(1); peerSource.set(1); old.dispose();
    fresh = effect(() => { values.push('fresh:' + nextSource()); }); expect(fresh.id).toBe(old.id);
    nextSource.set(1); nextSource.set(2);
    expect(getWasmCore().svet_pending_effect_count()).toBe(2);
  });
  expect(values).toEqual(['fresh:0', 'peer:1', 'fresh:2']); expect(getWasmCore().svet_pending_effect_count()).toBe(0);
  peer.dispose(); fresh.dispose(); source.dispose(); gate.dispose(); peerSource.dispose(); nextSource.dispose();
  expect(getGraphTelemetry().nodes).toBe(0);
});

it('batch-created peers invalidate an older direct single-subscriber cache', () => {
  const source = signal(0), left: number[] = [], right: number[] = [];
  const first = effect(() => { left.push(source()); }); let second!: ReturnType<typeof effect>;
  batch(() => { second = effect(() => { right.push(source()); }); });
  source.set(1); expect(left).toEqual([0, 1]); expect(right).toEqual([0, 1]);
  first.dispose(); second.dispose(); source.dispose();
});

it('a multi-effect flush finishes surviving callbacks and propagates its first failure', () => {
  const source = signal(0), failures: Error[] = [], values: string[] = [];
  const run = (name: string) => { const value = source(); values.push(name + value); if (value === 1) { const error = new Error(name); failures.push(error); throw error; } };
  const a = effect(() => run('a')), b = effect(() => run('b')); let caught: unknown;
  try { source.set(1); } catch (error) { caught = error; }
  expect(failures).toHaveLength(2); expect(caught).toBe(failures[0]);
  source.set(2); expect(values.slice(-2).sort()).toEqual(['a2','b2']); a.dispose(); b.dispose(); source.dispose();
});

it('multi-effect error handling preserves thrown undefined while completing both callbacks', () => {
  const source = signal(0); let failures = 0;
  const run = () => { if (source() === 1) { failures++; throw undefined; } };
  const a = effect(run), b = effect(run); let caught = false;
  try { source.set(1); } catch (error) { caught = true; expect(error).toBeUndefined(); }
  expect(caught).toBe(true); expect(failures).toBe(2); a.dispose(); b.dispose(); source.dispose();
});

it('continuing after a queue failure still rejects the removed peer lifetime', () => {
  const source = signal(0), nextSource = signal(0), values: number[] = [];
  let a!: ReturnType<typeof effect>, b!: ReturnType<typeof effect>, stop!: () => void, changed = false;
  const run = (other: () => ReturnType<typeof effect>) => {
    if (source() === 1 && !changed) {
      changed = true; other().dispose();
      createRoot(dispose => { stop = dispose; effect(() => { values.push(nextSource()); }); });
      throw new Error('queue callback failed');
    }
  };
  a = effect(() => run(() => b)); b = effect(() => run(() => a));
  expect(() => source.set(1)).toThrow('queue callback failed'); expect(values).toEqual([0]);
  nextSource.set(1); expect(values).toEqual([0,1]);
  a.dispose(); b.dispose(); stop(); source.dispose(); nextSource.dispose(); expect(getGraphTelemetry().nodes).toBe(0);
});


it('failed root creation releases owned computed, effect and tensor resources', () => {
  const source = signal(0), baseline = getGraphTelemetry().nodes;
  let runs = 0, cleanups = 0;
  const failure = new Error('root setup failed');
  expect(() => createRoot(() => {
    onCleanup(() => { cleanups++; throw new Error('cleanup failed'); });
    const value = computed(() => source() + 1);
    createGpuTensor([1,2,3]);
    effect(() => { value(); runs++; });
    throw failure;
  })).toThrow(failure);
  expect(cleanups).toBe(1); expect(getGraphTelemetry().nodes).toBe(baseline);
  source.set(1); expect(runs).toBe(1); source.dispose();
});

it('failed child scopes detach immediately from their surviving owner', () => {
  createRoot(stop => {
    const owner = getCurrentOwner()!, source = signal(0), baseline = getGraphTelemetry().nodes;
    let runs = 0, cleanups = 0;
    for (let i = 0; i < 20; i++) {
      expect(() => runWithOwnerScope([], () => {
        effect(() => { source(); runs++; });
        onCleanup(() => { cleanups++; });
        throw new Error('scope setup failed');
      })).toThrow('scope setup failed');
    }
    expect(owner.children?.size ?? 0).toBe(0);
    expect(cleanups).toBe(20); expect(getGraphTelemetry().nodes).toBe(baseline);
    source.set(1); expect(runs).toBe(20); stop(); source.dispose();
  });
});

it('failed scopes preserve thrown undefined and restore the caller owner', () => {
  createRoot(stop => {
    const owner = getCurrentOwner(); let caught = false, cleaned = false;
    try { runWithOwnerScope([], () => { onCleanup(() => { cleaned = true; }); throw undefined; }); }
    catch (error) { caught = true; expect(error).toBeUndefined(); }
    expect(caught).toBe(true); expect(cleaned).toBe(true); expect(getCurrentOwner()).toBe(owner); stop();
  });
});


it('a disposer captured by a failed root remains idempotent', () => {
  let stop!: () => void, cleanups = 0;
  expect(() => createRoot(dispose => { stop = dispose; onCleanup(() => { cleanups++; }); throw new Error('failed'); })).toThrow('failed');
  stop(); stop(); expect(cleanups).toBe(1);
});

it('failed old-root cleanup cannot dispose fresh resources created after reset', () => {
  let fresh!: ReturnType<typeof signal<number>>, observer!: ReturnType<typeof effect>;
  const values: number[] = []; let cleaned = 0;
  expect(() => createRoot(() => {
    onCleanup(() => { cleaned++; }); createGpuTensor([1]);
    resetReactiveSystem(); fresh = signal(10); observer = effect(() => { values.push(fresh()); });
    throw new Error('old root failed');
  })).toThrow('old root failed');
  expect(cleaned).toBe(1); fresh.set(11); expect(values).toEqual([10,11]);
  observer.dispose(); fresh.dispose(); expect(getGraphTelemetry().nodes).toBe(0);
});

it('child scope creation cannot bypass a disposed parent and create orphan effects', () => {
  const source = signal(0); let invoked = false;
  createRoot(stop => {
    const owner = getCurrentOwner()!; stop();
    expect(() => runWithOwnerScope([], () => { invoked = true; effect(() => { source(); }); })).toThrow('disposed owner');
    expect(invoked).toBe(false); expect(owner.children?.size ?? 0).toBe(0);
  });
  expect(getGraphTelemetry().nodes).toBe(1); source.dispose();
});
