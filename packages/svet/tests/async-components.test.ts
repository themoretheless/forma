/** @vitest-environment jsdom */
import { beforeEach, expect, it } from 'vitest';
import {
  createContext,
  effect,
  getCurrentOwner,
  getGraphTelemetry,
  getWasmCore,
  h,
  onCleanup,
  resetReactiveSystem,
  signal,
  useContext,
} from '../src/engine';
import { renderToStringAsync } from '../src/engine/server';
beforeEach(() => resetReactiveSystem());
const deferred = () => {
  let release!: () => void;
  const promise = new Promise<void>(r => (release = r));
  return { promise, release };
};
it('awaits sibling and nested h components while preserving provider scopes after await', async () => {
  const context = createContext('missing'),
    left = deferred(),
    right = deferred(),
    cleanups: string[] = [],
    owners: unknown[] = [];
  let core!: ReturnType<typeof getWasmCore>;
  const Child = async ({ wait }: { wait: Promise<void> }) => {
    const owner = getCurrentOwner();
    owners.push(owner);
    await wait;
    expect(getCurrentOwner()).toBe(owner);
    const name = useContext(context);
    onCleanup(() => cleanups.push(name));
    const value = signal(name);
    onCleanup(value.dispose);
    effect(() => {
      value();
    });
    return h(
      'p',
      null,
      h(async () => {
        await Promise.resolve();
        expect(useContext(context)).toBe(name);
        return h('b', null, value());
      }, null),
    );
  };
  const rendering = renderToStringAsync(() => {
    core = getWasmCore();
    return {
      node: h(
        'main',
        null,
        h(context.Provider, { value: 'left', children: () => h(Child, { wait: left.promise }) }),
        h(context.Provider, { value: 'right', children: () => h(Child, { wait: right.promise }) }),
      ),
    };
  });
  right.release();
  left.release();
  const result = await rendering;
  expect(result.html).toBe('<main><p><b>left</b></p><p><b>right</b></p></main>');
  expect(owners[0]).not.toBe(owners[1]);
  expect(cleanups.sort()).toEqual(['left', 'right']);
  expect(core.svet_get_node_count()).toBe(0);
});
it('rejects a component failure and releases successful sibling resources', async () => {
  const failure = Error('async component failed');
  let cleaned = 0,
    core!: ReturnType<typeof getWasmCore>;
  await expect(
    renderToStringAsync(() => {
      core = getWasmCore();
      return {
        node: h(
          'main',
          null,
          h(async () => {
            const s = signal(1);
            onCleanup(() => {
              cleaned++;
              s.dispose();
            });
            await Promise.resolve();
            return h('p', null, 'ok');
          }, null),
          h(async () => {
            await Promise.resolve();
            throw failure;
          }, null),
        ),
      };
    }),
  ).rejects.toBe(failure);
  expect(cleaned).toBe(1);
  expect(core.svet_get_node_count()).toBe(0);
});
it('aborts pending components and refuses late resource creation after request teardown', async () => {
  const wait = deferred(),
    entered = deferred(),
    abort = new AbortController(),
    reason = Error('disconnect');
  let late: unknown,
    cleaned = 0,
    core!: ReturnType<typeof getWasmCore>;
  const rendering = renderToStringAsync(
    () => {
      core = getWasmCore();
      return {
        node: h(async () => {
          onCleanup(() => cleaned++);
          entered.release();
          await wait.promise;
          try {
            signal(1);
          } catch (error) {
            late = error;
          }
          return document.createElement('p');
        }, null),
      };
    },
    { signal: abort.signal },
  );
  await entered.promise;
  abort.abort(reason);
  await expect(rendering).rejects.toBe(reason);
  wait.release();
  for (let i = 0; i < 30; i++) await Promise.resolve();
  expect(late).toBeInstanceOf(Error);
  expect(cleaned).toBe(1);
  expect(core.svet_get_node_count()).toBe(0);
  expect(getGraphTelemetry().nodes).toBe(0);
});
it('preserves immediate DOM access for synchronous components inside async SSR', async () => {
  const result = await renderToStringAsync(() => {
    const node = h(() => h('span', null, 'ready'), null);
    expect(node.textContent).toBe('ready');
    return { node };
  });
  expect(result.html).toBe('<span>ready</span>');
});
it('settles async components created by the result node getter', async () => {
  const result = await renderToStringAsync(() => ({
    get node() {
      return h(
        'main',
        null,
        h(async () => {
          await Promise.resolve();
          return h('p', null, 'getter');
        }, null),
      );
    },
  }));
  expect(result.html).toBe('<main><p>getter</p></main>');
});
it('fails without waiting for an uncooperative sibling component', async () => {
  const failure = Error('sibling failed');
  let cleaned = 0;
  await expect(
    renderToStringAsync(() => ({
      node: h(
        'main',
        null,
        h(async () => {
          onCleanup(() => cleaned++);
          await new Promise(() => {});
          return document.createElement('i');
        }, null),
        h(async () => {
          await Promise.resolve();
          throw failure;
        }, null),
      ),
    })),
  ).rejects.toBe(failure);
  expect(cleaned).toBe(1);
});
it('keeps concurrent render sessions independent while one tree remains suspended', async () => {
  const wait = deferred(),
    entered = deferred();
  let coreA!: ReturnType<typeof getWasmCore>, coreB!: ReturnType<typeof getWasmCore>;
  const a = renderToStringAsync(() => ({
    node: h(async () => {
      coreA = getWasmCore();
      entered.release();
      await wait.promise;
      expect(getWasmCore()).toBe(coreA);
      return h('p', null, 'A');
    }, null),
  }));
  await entered.promise;
  const b = await renderToStringAsync(() => ({
    node: h(async () => {
      coreB = getWasmCore();
      await Promise.resolve();
      return h('p', null, 'B');
    }, null),
  }));
  expect(b.html).toBe('<p>B</p>');
  expect(coreA).not.toBe(coreB);
  wait.release();
  expect((await a).html).toBe('<p>A</p>');
  expect(coreA.svet_get_node_count()).toBe(0);
  expect(coreB.svet_get_node_count()).toBe(0);
});
it('rejects async components outside the Node async renderer without unhandled rejections', async () => {
  expect(() =>
    h(async () => {
      throw Error('unsupported async component');
    }, null),
  ).toThrow(/require renderToStringAsync/);
  for (let i = 0; i < 10; i++) await Promise.resolve();
  expect(getGraphTelemetry().nodes).toBe(0);
});
