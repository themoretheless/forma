/** @vitest-environment jsdom */
import { beforeEach, expect, it } from 'vitest';
import {
  compileFormaComponent,
  getGraphTelemetry,
  resetReactiveSystem,
  signal,
} from '../src/engine';
beforeEach(() => resetReactiveSystem());
it('drains all owned resources when a computed disposer throws and stays idempotent', () => {
  const inst = compileFormaComponent(
      `component App { state count: Number = 1; prop limit: Number = 5; let doubled = state.count * 2; let tripled = state.count * 3; Text { text:doubled; } }`,
    ),
    container = document.createElement('div'),
    handle = inst.mount(container);
  const comp = inst.computedLets.doubled,
    dispose = comp.dispose.bind(comp),
    failure = Error('computed dispose refused');
  comp.dispose = () => {
    dispose();
    throw failure;
  };
  expect(() => inst.dispose()).toThrow(failure);
  expect(container.childNodes.length).toBe(0);
  expect(getGraphTelemetry().nodes).toBe(0);
  expect(() => inst.dispose()).not.toThrow();
  handle.unmount();
});
it('preserves an external signal even when the options ViewModel map is replaced later', () => {
  const external = signal(7),
    options: { viewModel: Record<string, any> } = { viewModel: { external } };
  const inst = compileFormaComponent(
    `component App { state external: Number = 1; Column {} }`,
    options,
  );
  options.viewModel = {};
  try {
    inst.dispose();
    expect(external()).toBe(7);
    expect(getGraphTelemetry().nodes).toBe(1);
  } finally {
    external.dispose();
  }
  expect(getGraphTelemetry().nodes).toBe(0);
});
it('does not classify a skipped ViewModel entry as an external owned-state binding', () => {
  const inst = compileFormaComponent(`component App { state owned: Number = 1; Column {} }`, {
    viewModel: { owned: {} as any },
  });
  expect(inst.state.owned()).toBe(1);
  inst.dispose();
  expect(getGraphTelemetry().nodes).toBe(0);
});
it('disposes originally allocated state handles once despite aliases in the public state map', () => {
  const inst = compileFormaComponent(
      `component App { state a: Number = 1; state b: Number = 2; Column {} }`,
    ),
    a = inst.state.a,
    b = inst.state.b;
  let aCalls = 0,
    bCalls = 0;
  const da = a.dispose.bind(a),
    db = b.dispose.bind(b);
  a.dispose = () => {
    aCalls++;
    da();
  };
  b.dispose = () => {
    bCalls++;
    db();
  };
  inst.state.b = a;
  inst.state.extra = a;
  inst.dispose();
  expect(aCalls).toBe(1);
  expect(bCalls).toBe(1);
  expect(getGraphTelemetry().nodes).toBe(0);
});
it('does not acquire an external signal assigned over an owned field after construction', () => {
  const external = signal(7),
    inst = compileFormaComponent(`component App { state owned: Number = 1; Column {} }`),
    owned = inst.state.owned;
  let released = 0;
  const stop = owned.dispose.bind(owned);
  owned.dispose = () => {
    released++;
    stop();
  };
  inst.state.owned = external;
  inst.state.alias = external;
  try {
    inst.dispose();
    expect(external()).toBe(7);
    expect(released).toBe(1);
    expect(getGraphTelemetry().nodes).toBe(1);
  } finally {
    external.dispose();
  }
  expect(getGraphTelemetry().nodes).toBe(0);
});
it('rollback releases an allocated candidate state replaced by an external alias without disposing the alias', () => {
  const external = signal(9),
    primary = Error('prepare alias refused');
  let fail = false,
    released = 0;
  const source = `component App { state count: Number = 1; Column { Text { text:state.count; } Child {} } }`;
  const inst = compileFormaComponent(source, {
    components: {
      Child: () => {
        const child = compileFormaComponent(`component Child { Column {} }`);
        if (fail) {
          const allocated = inst.state.added,
            stop = allocated.dispose.bind(allocated);
          allocated.dispose = () => {
            released++;
            stop();
          };
          inst.state.added = external;
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
          'state count: Number = 1;',
          'state count: Number = 1; state added: Number = 2;',
        ),
      );
    } catch (error) {
      caught = error;
    }
    expect(caught).toBe(primary);
    expect(external()).toBe(9);
    expect(released).toBe(1);
    expect(getGraphTelemetry().nodes).toBe(nodes);
    inst.state.count.set(2);
    expect(container.textContent).toBe('2');
  } finally {
    fail = false;
    handle.unmount();
    inst.dispose();
    external.dispose();
  }
  expect(getGraphTelemetry().nodes).toBe(0);
});
it('preserves the preparation failure when owned candidate state cleanup closes an instance with props', () => {
  const primary = Error('prepare state closes');
  let fail = false,
    closed = false,
    closedWrites = 0;
  const source = `component App { state count: Number = 1; prop limit: Number = 5; Column { Child {} } }`;
  const inst = compileFormaComponent(source, {
    components: {
      Child: () => {
        const child = compileFormaComponent(`component Child { Column {} }`);
        if (fail) {
          const added = inst.state.added,
            stop = added.dispose.bind(added);
          added.dispose = () => {
            stop();
            inst.dispose();
            closed = true;
            throw Error('secondary state cleanup');
          };
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
    set = inst.props.limit.set;
  inst.props.limit.set = value => {
    if (closed) closedWrites++;
    return set(value);
  };
  try {
    fail = true;
    let caught;
    try {
      inst.hotReload(
        source.replace(
          'state count: Number = 1;',
          'state count: Number = 1; state added: Number = 2;',
        ),
      );
    } catch (error) {
      caught = error;
    }
    expect(caught).toBe(primary);
    expect(closedWrites).toBe(0);
    expect(container.childNodes.length).toBe(0);
    expect(getGraphTelemetry().nodes).toBe(0);
  } finally {
    fail = false;
    handle.unmount();
    inst.dispose();
  }
});
