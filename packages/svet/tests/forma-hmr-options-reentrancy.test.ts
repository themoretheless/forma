/** @vitest-environment jsdom */
import { beforeEach, expect, it } from 'vitest';
import { compileFormaComponent, getGraphTelemetry, resetReactiveSystem } from '../src/engine';
beforeEach(() => resetReactiveSystem());
for (const kind of ['new-prop', 'existing-prop', 'existing-prop-no-lets'])
  it(`stops HMR allocations after an options getter closes the instance: ${kind}`, () => {
    let close = false,
      inst: ReturnType<typeof compileFormaComponent>;
    const props = {
      get value() {
        if (close) inst.dispose();
        return 2;
      },
    };
    const declarations =
      (kind === 'new-prop' ? '' : 'prop value: Number = 1; ') +
      (kind === 'existing-prop-no-lets' ? '' : 'let doubled = 2; ');
    const source = `component App { ${declarations} Column {} }`;
    inst = compileFormaComponent(source, { props });
    const container = document.createElement('div'),
      handle = inst.mount(container);
    let oldCalls = 0;
    if (inst.computedLets.doubled) {
      const old = inst.computedLets.doubled,
        stop = old.dispose.bind(old);
      old.dispose = () => {
        oldCalls++;
        stop();
      };
    }
    const next =
      kind === 'new-prop'
        ? source.replace('component App {', 'component App { prop value: Number = 1;')
        : source;
    try {
      close = true;
      expect(() => inst.hotReload(next)).toThrow(/Forma instance is disposed/);
      expect(container.childNodes.length).toBe(0);
      expect(getGraphTelemetry().nodes).toBe(0);
      expect(oldCalls).toBe(kind === 'existing-prop-no-lets' ? 0 : 1);
    } finally {
      close = false;
      handle.unmount();
      inst.dispose();
    }
  });
for (const kind of ['state', 'prop', 'computed'])
  it(`registers allocated ${kind} ownership before a public-map setter closes the instance`, () => {
    const source = `component App { let doubled = 2; Column {} }`,
      inst = compileFormaComponent(source),
      container = document.createElement('div'),
      handle = inst.mount(container),
      old = inst.computedLets.doubled,
      stopOld = old.dispose.bind(old);
    let oldCalls = 0,
      released = 0;
    old.dispose = () => {
      oldCalls++;
      stopOld();
    };
    const map = kind === 'state' ? inst.state : kind === 'prop' ? inst.props : inst.computedLets;
    Object.defineProperty(map, 'added', {
      configurable: true,
      get() {
        return undefined;
      },
      set(resource) {
        const stop = resource.dispose.bind(resource);
        resource.dispose = () => {
          released++;
          stop();
        };
        inst.dispose();
      },
    });
    const declaration =
      kind === 'state'
        ? 'state added: Number = 1;'
        : kind === 'prop'
          ? 'prop added: Number = 1;'
          : 'let added = 1;';
    try {
      expect(() =>
        inst.hotReload(source.replace('component App {', 'component App { ' + declaration)),
      ).toThrow(/Forma instance is disposed/);
      expect(released).toBe(1);
      expect(oldCalls).toBe(1);
      expect(container.childNodes.length).toBe(0);
      expect(getGraphTelemetry().nodes).toBe(0);
    } finally {
      handle.unmount();
      inst.dispose();
    }
  });
it('restores captured metadata without rereading options getters during rollback', () => {
  let reads = 0,
    fail = false,
    closed = false,
    lateLocaleWrites = 0,
    inst: ReturnType<typeof compileFormaComponent>;
  const primary = Error('prepare metadata refused');
  const props = {
    get value() {
      if (++reads === 3) {
        inst.dispose();
        closed = true;
      }
      return 2;
    },
  };
  const source = `component App { prop value: Number = 1; let doubled = 2; Column { Child {} } }`;
  inst = compileFormaComponent(source, {
    props,
    components: {
      Child: () => {
        const child = compileFormaComponent(`component Child { Column {} }`);
        if (fail)
          child.render = () => {
            throw primary;
          };
        return child;
      },
    },
  });
  const container = document.createElement('div'),
    handle = inst.mount(container),
    old = inst.computedLets.doubled,
    stop = old.dispose.bind(old),
    set = inst.locale.set,
    nodes = getGraphTelemetry().nodes;
  let oldCalls = 0;
  old.dispose = () => {
    oldCalls++;
    stop();
  };
  inst.locale.set = value => {
    if (closed) lateLocaleWrites++;
    return set(value);
  };
  try {
    fail = true;
    let caught;
    try {
      inst.hotReload(source);
    } catch (error) {
      caught = error;
    }
    expect(caught).toBe(primary);
    expect(reads).toBe(2);
    expect(closed).toBe(false);
    expect(inst.computedLets.doubled).toBe(old);
    expect(oldCalls).toBe(0);
    expect(lateLocaleWrites).toBe(0);
    expect(getGraphTelemetry().nodes).toBe(nodes);
  } finally {
    fail = false;
    handle.unmount();
    inst.dispose();
  }
});
