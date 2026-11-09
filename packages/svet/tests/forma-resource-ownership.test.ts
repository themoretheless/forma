/** @vitest-environment jsdom */
import { beforeEach, expect, it } from 'vitest';
import {
  compileFormaComponent,
  computed,
  getGraphTelemetry,
  resetReactiveSystem,
  signal,
} from '../src/engine';
beforeEach(() => resetReactiveSystem());
it('owns original prop handles once despite aliases in the public map', () => {
  const inst = compileFormaComponent(
      `component App { prop a: Number = 1; prop b: Number = 2; Column {} }`,
    ),
    a = inst.props.a,
    b = inst.props.b,
    da = a.dispose.bind(a),
    db = b.dispose.bind(b);
  let ac = 0,
    bc = 0;
  a.dispose = () => {
    ac++;
    da();
  };
  b.dispose = () => {
    bc++;
    db();
  };
  inst.props.b = a;
  inst.props.extra = a;
  inst.dispose();
  expect(ac).toBe(1);
  expect(bc).toBe(1);
  expect(getGraphTelemetry().nodes).toBe(0);
});
it('does not acquire an external prop replacement or lose the original own handle', () => {
  const external = signal(7),
    inst = compileFormaComponent(`component App { prop own: Number = 1; Column {} }`),
    own = inst.props.own,
    stop = own.dispose.bind(own);
  let calls = 0;
  own.dispose = () => {
    calls++;
    stop();
  };
  inst.props.own = external;
  try {
    inst.dispose();
    expect(calls).toBe(1);
    expect(external()).toBe(7);
    expect(getGraphTelemetry().nodes).toBe(1);
  } finally {
    external.dispose();
  }
  expect(getGraphTelemetry().nodes).toBe(0);
});
it('owns original computed handles once despite public aliases', () => {
  const inst = compileFormaComponent(`component App { let a = 1; let b = 2; Column {} }`),
    a = inst.computedLets.a,
    b = inst.computedLets.b,
    da = a.dispose.bind(a),
    db = b.dispose.bind(b);
  let ac = 0,
    bc = 0;
  a.dispose = () => {
    ac++;
    da();
  };
  b.dispose = () => {
    bc++;
    db();
  };
  inst.computedLets.b = a;
  inst.computedLets.extra = a;
  inst.dispose();
  expect(ac).toBe(1);
  expect(bc).toBe(1);
  expect(getGraphTelemetry().nodes).toBe(0);
});
it('HMR retires allocated computed handles and preserves a public foreign replacement', () => {
  const external = signal(7),
    foreign = computed(() => external()),
    source = `component App { let a = 1; let b = 2; Column {} }`,
    inst = compileFormaComponent(source),
    old = inst.computedLets.a,
    stop = old.dispose.bind(old),
    foreignStop = foreign.dispose.bind(foreign);
  let ownCalls = 0,
    foreignCalls = 0;
  old.dispose = () => {
    ownCalls++;
    stop();
  };
  foreign.dispose = () => {
    foreignCalls++;
    foreignStop();
  };
  inst.computedLets.a = foreign;
  try {
    inst.hotReload(source);
    expect(ownCalls).toBe(1);
    expect(foreignCalls).toBe(0);
    expect(foreign()).toBe(7);
    inst.dispose();
    expect(foreignCalls).toBe(0);
  } finally {
    inst.dispose();
    foreign.dispose();
    external.dispose();
  }
  expect(getGraphTelemetry().nodes).toBe(0);
});
for (const kind of ['prop', 'computed'])
  it(`rollback releases the allocated candidate ${kind} rather than its foreign public replacement`, () => {
    const external = signal(7),
      primary = Error('prepare ownership refused');
    let fail = false,
      released = 0;
    const source = `component App { prop base: Number = 1; let value = 2; Column { Child {} } }`;
    const inst = compileFormaComponent(source, {
      components: {
        Child: () => {
          const child = compileFormaComponent(`component Child { Column {} }`);
          if (fail) {
            const map = kind === 'prop' ? inst.props : inst.computedLets,
              key = kind === 'prop' ? 'added' : 'value',
              own = map[key],
              stop = own.dispose.bind(own);
            own.dispose = () => {
              released++;
              stop();
            };
            map[key] = external;
            child.render = () => {
              throw primary;
            };
          }
          return child;
        },
      },
    });
    const container = document.createElement('div'),
      handle = inst.mount(container),
      nodes = getGraphTelemetry().nodes;
    try {
      fail = true;
      let caught;
      try {
        inst.hotReload(
          source.replace(
            'prop base: Number = 1;',
            'prop base: Number = 1; prop added: Number = 3;',
          ),
        );
      } catch (error) {
        caught = error;
      }
      expect(caught).toBe(primary);
      expect(released).toBe(1);
      expect(external()).toBe(7);
      expect(getGraphTelemetry().nodes).toBe(nodes);
    } finally {
      fail = false;
      handle.unmount();
      inst.dispose();
      external.dispose();
    }
    expect(getGraphTelemetry().nodes).toBe(0);
  });
