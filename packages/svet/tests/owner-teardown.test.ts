import {beforeEach, expect, it} from 'vitest';
import {createRoot, runWithOwnerScope, getCurrentOwner, onCleanup, effect, computed, signal, createGpuTensor, getWasmCore, getGraphTelemetry, resetReactiveSystem} from '../src/engine';
import {runWithCapturedOwner, type OwnerScope} from '../src/engine/reactivity';

beforeEach(() => resetReactiveSystem());

it('deep owner-tree teardown releases all scopes and leaf resources without recursive stack growth', () => {
  const core = getWasmCore(), source = signal(0), scopes: OwnerScope[] = [];
  let stop!: () => void, cleaned = 0, runs = 0;
  createRoot(dispose => {
    stop = dispose; let parent = getCurrentOwner()!; scopes.push(parent);
    for (let i = 0; i < 12000; i++) {
      runWithCapturedOwner(core, parent, () => runWithOwnerScope([], () => {
        parent = getCurrentOwner()!; scopes.push(parent); onCleanup(() => { cleaned++; });
      }));
    }
    runWithCapturedOwner(core, parent, () => {
      const value = computed(() => source()); createGpuTensor([1]); effect(() => { value(); runs++; });
    });
  });
  expect(() => stop()).not.toThrow(); expect(cleaned).toBe(12000);
  expect(scopes.every(scope => scope.disposed && scope.parent === null && scope.disposables.length === 0 && !scope.children?.size)).toBe(true);
  source.set(1); expect(runs).toBe(1); expect(getGraphTelemetry().nodes).toBe(1);
  stop(); expect(cleaned).toBe(12000); source.dispose();
});

it('teardown preserves child insertion order and reverse cleanup order within each scope', () => {
  const events: string[] = [];
  createRoot(stop => {
    onCleanup(() => { events.push('root:first'); });
    runWithOwnerScope([], () => { onCleanup(() => { events.push('a:first'); }); onCleanup(() => { events.push('a:last'); }); });
    runWithOwnerScope([], () => { onCleanup(() => { events.push('b:first'); }); onCleanup(() => { events.push('b:last'); }); });
    onCleanup(() => { events.push('root:last'); }); stop();
  });
  expect(events).toEqual(['a:last','a:first','b:last','b:first','root:last','root:first']);
});

it('reentrant sibling disposal during child cleanup remains idempotent', () => {
  const events: string[] = []; let sibling!: ReturnType<typeof effect>;
  createRoot(stop => {
    runWithOwnerScope([], () => { onCleanup(() => { events.push('child'); sibling.dispose(); }); });
    sibling = effect(() => () => { events.push('sibling'); });
    onCleanup(() => { events.push('root'); }); stop(); stop(); sibling.dispose();
  });
  expect(events).toEqual(['child','sibling','root']); expect(getGraphTelemetry().nodes).toBe(0);
});

it('teardown marks ancestors before callbacks but does not pre-dispose later siblings', () => {
  let root!: OwnerScope, later!: OwnerScope; const events: string[] = [];
  createRoot(stop => {
    root = getCurrentOwner()!;
    runWithOwnerScope([], () => { onCleanup(() => {
      expect(root.disposed).toBe(true); expect(later.disposed).toBe(false); events.push('first'); throw new Error('cleanup failed');
    }); });
    runWithOwnerScope([], () => { later = getCurrentOwner()!; onCleanup(() => { events.push('later'); }); });
    onCleanup(() => { events.push('root'); }); stop();
  });
  expect(events).toEqual(['first','later','root']); expect(later.disposed).toBe(true);
});

it('effect rerun cleanup releases a deep scope tree before its next invocation', () => {
  const core = getWasmCore(), source = signal(0); let cleaned = 0, runs = 0;
  const observer = effect(() => {
    runs++; const value = source();
    if (value === 0) {
      let parent = getCurrentOwner()!;
      for (let i = 0; i < 12000; i++) runWithCapturedOwner(core,parent,() => runWithOwnerScope([],() => {
        parent = getCurrentOwner()!; onCleanup(() => { cleaned++; });
      }));
    } else expect(cleaned).toBe(12000);
  });
  expect(() => source.set(1)).not.toThrow(); expect(runs).toBe(2); expect(cleaned).toBe(12000);
  observer.dispose(); source.dispose(); expect(getGraphTelemetry().nodes).toBe(0);
});

it('stopping ancestors reject new child resources and prevent later descendant effect/computed execution', () => {
  const core = getWasmCore(), source = signal(0); let later!: OwnerScope, runs = 0, computes = 0, derived!: () => number, observed = -1, laterWasLive = false, refused = false;
  createRoot(stop => {
    runWithOwnerScope([], () => { onCleanup(() => {
      laterWasLive = !later.disposed;
      source.set(1); observed = derived();
      try { runWithCapturedOwner(core, later, () => effect(() => {})); }
      catch (error) { refused = /disposed/.test(String(error)); }
    }); });
    runWithOwnerScope([], () => { later = getCurrentOwner()!; derived = computed(() => { computes++; return source(); }); effect(() => { derived(); runs++; }); });
    stop();
  });
  expect([runs, computes, observed, laterWasLive, refused]).toEqual([1,1,0,true,true]); source.dispose(); expect(getGraphTelemetry().nodes).toBe(0);
});
it('effect rerun retires descendant execution before cleanup visits later siblings',()=>{
 const core=getWasmCore(),trigger=signal(0),secondary=signal(0);let later!:OwnerScope,runs=0,refused=false,laterWasLive=false,newRuns=0;
 const observer=effect(()=>{trigger();runWithOwnerScope([],()=>{onCleanup(()=>{laterWasLive=!later.disposed;secondary.set(secondary.peek()+1);try{runWithCapturedOwner(core,later,()=>effect(()=>{newRuns++;}));}catch(error){refused=/disposed/.test(String(error));}});});runWithOwnerScope([],()=>{later=getCurrentOwner()!;effect(()=>{secondary();runs++;});});});
 trigger.set(1);expect([runs,refused,laterWasLive,newRuns]).toEqual([2,true,true,0]);observer.dispose();trigger.dispose();secondary.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});
