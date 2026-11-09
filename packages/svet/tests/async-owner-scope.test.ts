/** @vitest-environment jsdom */
import { beforeEach, expect, it } from 'vitest';
import {
  captureReactiveContext,
  computed,
  createResource,
  createRoot,
  effect,
  getCurrentOwner,
  getGraphTelemetry,
  getWasmCore,
  h,
  lookupOwnerContext,
  onCleanup,
  resetReactiveSystem,
  runWithOwnerScope,
  signal,
} from '../src/engine';
import {
  renderToStringAsync,
  runWithIsolatedContextAsync,
  runWithOwnerScopeAsync,
} from '../src/engine/server';

beforeEach(() => resetReactiveSystem());
function gate() {
  let release!: () => void;
  const promise = new Promise<void>(resolve => {
    release = resolve;
  });
  return { promise, release };
}
const key = Symbol('async subtree');

it('keeps interleaved sibling owners and inherited context independent after await', async () => {
  const a = gate(),
    b = gate(),
    cleanups: string[] = [];
  let core!: ReturnType<typeof getWasmCore>;
  await runWithIsolatedContextAsync(async () => {
    core = getWasmCore();
    const parent = getCurrentOwner();
    const run = (name: string, wait: Promise<void>) =>
      runWithOwnerScopeAsync([[key, name]], async () => {
        const owner = getCurrentOwner();
        expect(owner).not.toBe(parent);
        expect(owner?.parent).toBe(parent);
        await wait;
        expect(getWasmCore()).toBe(core);
        expect(getCurrentOwner()).toBe(owner);
        expect(lookupOwnerContext(key, 'missing')).toBe(name);
        onCleanup(() => cleanups.push(name));
        const source = signal(1),
          memo = computed(() => source() + 1);
        let observed = 0;
        effect(() => {
          observed = memo();
        });
        source.set(3);
        expect(observed).toBe(4);
        await runWithOwnerScopeAsync([], async () => {
          await Promise.resolve();
          expect(lookupOwnerContext(key, '')).toBe(name);
          expect(getCurrentOwner()?.parent).toBe(owner);
        });
        expect(getCurrentOwner()).toBe(owner);
        return owner;
      });
    const first = run('a', a.promise),
      second = run('b', b.promise);
    b.release();
    const secondOwner = await second;
    expect(getCurrentOwner()).toBe(parent);
    expect(cleanups).toEqual([]);
    a.release();
    const firstOwner = await first;
    expect(firstOwner).not.toBe(secondOwner);
    expect(firstOwner?.disposed).toBe(false);
    expect(secondOwner?.disposed).toBe(false);
    expect(lookupOwnerContext(key, 'root')).toBe('root');
  });
  expect(cleanups.sort()).toEqual(['a', 'b']);
  expect(core.svet_get_node_count()).toBe(0);
  expect(getGraphTelemetry().nodes).toBe(0);
});

it('disposes a rejected subtree immediately without stopping its sibling or request', async () => {
  let stopped = 0;
  let resourceSignal!: AbortSignal;
  await runWithIsolatedContextAsync(async () => {
    const parent = getCurrentOwner(),
      source = signal(1);
    let seen = 0;
    effect(() => {
      seen = source();
    });
    const baseline = getGraphTelemetry().nodes;
    const failure = Error('component failed');
    await expect(
      runWithOwnerScopeAsync([], async () => {
        await Promise.resolve();
        onCleanup(() => stopped++);
        createResource(ctx => {
          resourceSignal = ctx.signal;
          return new Promise(() => {});
        });
        computed(() => source());
        effect(() => {
          source();
        });
        throw failure;
      }),
    ).rejects.toBe(failure);
    expect(stopped).toBe(1);
    expect(resourceSignal.aborted).toBe(true);
    expect(getGraphTelemetry().nodes).toBe(baseline);
    expect(getCurrentOwner()).toBe(parent);
    source.set(2);
    expect(seen).toBe(2);
    expect(await runWithOwnerScopeAsync([], async () => 42)).toBe(42);
  });
  expect(stopped).toBe(1);
});

it('cancels a pending child when its synchronous parent is disposed', async () => {
  await runWithIsolatedContextAsync(async () => {
    const started = gate(),
      wait = gate();
    let dispose!: () => void;
    let late!: Promise<unknown>,
      pending!: Promise<unknown>,
      stopped = 0;
    runWithOwnerScope([], stop => {
      dispose = stop;
      pending = runWithOwnerScopeAsync([], async () => {
        onCleanup(() => stopped++);
        computed(() => 1);
        late = wait.promise.then(() => getCurrentOwner());
        started.release();
        return late;
      });
    });
    await started.promise;
    const assertion = expect(pending).rejects.toThrow('disposed');
    dispose();
    await assertion;
    expect(stopped).toBe(1);
    expect(getGraphTelemetry().nodes).toBe(0);
    const lateAssertion = expect(late).rejects.toThrow(/disposed|completed/);
    wait.release();
    await lateAssertion;
    expect(await runWithOwnerScopeAsync([], async () => 'request alive')).toBe('request alive');
  });
});

it('honors child abort without cancelling its parent request', async () => {
  await runWithIsolatedContextAsync(async () => {
    const wait = gate(),
      started = gate(),
      abort = new AbortController();
    let stopped = 0;
    const reason = Error('child cancelled');
    const pending = runWithOwnerScopeAsync(
      [],
      async () => {
        onCleanup(() => stopped++);
        effect(() => {});
        started.release();
        await wait.promise;
      },
      { signal: abort.signal },
    );
    await started.promise;
    const assertion = expect(pending).rejects.toBe(reason);
    abort.abort(reason);
    await assertion;
    expect(stopped).toBe(1);
    expect(getGraphTelemetry().nodes).toBe(0);
    wait.release();
    expect(await runWithOwnerScopeAsync([], () => 7)).toBe(7);
  });
});

it('gives synchronous owners and explicit captured root reentry priority over async owners', async () => {
  await runWithIsolatedContextAsync(async () => {
    const root = getCurrentOwner(),
      captured = captureReactiveContext();
    await runWithOwnerScopeAsync([[key, 'async']], async () => {
      await Promise.resolve();
      const owner = getCurrentOwner();
      runWithOwnerScope([[key, 'sync']], () => expect(lookupOwnerContext(key, '')).toBe('sync'));
      captured.run(() => {
        expect(getCurrentOwner()).toBe(root);
        expect(lookupOwnerContext(key, 'root')).toBe('root');
      });
      expect(getCurrentOwner()).toBe(owner);
      createRoot(dispose => {
        expect(getCurrentOwner()).not.toBe(owner);
        dispose();
      });
      effect(() => {
        expect(getCurrentOwner()?.parent).toBe(owner);
      });
    });
    captured.dispose();
  });
});

it('closes detached continuations when a child settles while the request stays open', async () => {
  await runWithIsolatedContextAsync(async () => {
    const wait = gate();
    let late!: Promise<unknown>;
    await runWithOwnerScopeAsync([], () => {
      late = wait.promise.then(() => signal(1));
      return 1;
    });
    const assertion = expect(late).rejects.toThrow('already completed');
    wait.release();
    await assertion;
    expect(getGraphTelemetry().nodes).toBe(0);
  });
});

it('rejects reset during a child await and does not reopen the default graph', async () => {
  await runWithIsolatedContextAsync(async () => {
    await expect(
      runWithOwnerScopeAsync([], async () => {
        await Promise.resolve();
        resetReactiveSystem();
        return 'stale';
      }),
    ).rejects.toThrow('reset');
  });
  expect(getGraphTelemetry().nodes).toBe(0);
});

it('rejects outside a request and skips an already-aborted callback', async () => {
  await expect(runWithOwnerScopeAsync([], () => 1)).rejects.toThrow('active isolated SSR request');
  await runWithIsolatedContextAsync(async () => {
    const abort = new AbortController();
    abort.abort(Error('preflight'));
    let calls = 0;
    await expect(
      runWithOwnerScopeAsync([], () => calls++, { signal: abort.signal }),
    ).rejects.toThrow('preflight');
    expect(calls).toBe(0);
  });
});

it('serializes successful sibling subtrees and keeps their effects live until request teardown', async () => {
  let stopped = 0,
    core!: ReturnType<typeof getWasmCore>;
  const rendered = await renderToStringAsync(async () => {
    core = getWasmCore();
    const value = signal(1);
    const nodes = await Promise.all(
      ['a', 'b'].map(name =>
        runWithOwnerScopeAsync([[key, name]], async () => {
          await Promise.resolve();
          onCleanup(() => stopped++);
          return h('p', null, () => lookupOwnerContext(key, '') + ':' + value());
        }),
      ),
    );
    expect(stopped).toBe(0);
    value.set(2);
    expect(nodes.map(node => node.textContent)).toEqual(['a:2', 'b:2']);
    return { node: h('section', null, ...nodes), signals: { value } };
  });
  expect(rendered.html).toContain('<section><p>a:2</p><p>b:2</p></section>');
  expect(JSON.parse(rendered.stateJson).signals.value).toBe(2);
  expect(stopped).toBe(2);
  expect(core.svet_get_node_count()).toBe(0);
});

it('tears down waiting child scopes when the whole request aborts', async () => {
  const abort = new AbortController(),
    started = gate(),
    wait = gate();
  let stopped = 0;
  let child!: Promise<unknown>;
  const pending = runWithIsolatedContextAsync(
    async () => {
      child = runWithOwnerScopeAsync([], async () => {
        onCleanup(() => stopped++);
        effect(() => {});
        started.release();
        await wait.promise;
      });
      return child;
    },
    { signal: abort.signal },
  );
  await started.promise;
  const assertion = expect(pending).rejects.toThrow('request cancelled');
  const childAssertion = expect(child).rejects.toThrow('disposed');
  abort.abort(Error('request cancelled'));
  await assertion;
  await childAssertion;
  expect(stopped).toBe(1);
  expect(getGraphTelemetry().nodes).toBe(0);
  wait.release();
});

it('rechecks request lifetime after option getters before invoking a child', async () => {
  let calls = 0;
  await runWithIsolatedContextAsync(async () => {
    await expect(
      runWithOwnerScopeAsync([], () => calls++, {
        get signal() {
          resetReactiveSystem();
          return undefined;
        },
      }),
    ).rejects.toThrow('reset');
  });
  expect(calls).toBe(0);
  expect(getGraphTelemetry().nodes).toBe(0);
});
