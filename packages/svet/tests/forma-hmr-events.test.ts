/** @vitest-environment jsdom */
import { beforeEach, expect, it } from 'vitest';
import { compileFormaComponent, getGraphTelemetry, parseFormaSource, resetReactiveSystem, signal } from '../src/engine';
beforeEach(() => resetReactiveSystem());

it('HMR and individual view unmount retire retained click/input callbacks', () => {
  const source = `component App {
    state count: Number = 0; state name: String = 'seed';
    Column {
      Button { key:'inc'; text:'add'; clicked -> state.increment('count',1); }
      TextInput { key:'name'; value <-> state.name; changed -> state.increment('count',1); }
      Text { text:'v1:\${state.count}'; }
    }
  }`;
  const inst = compileFormaComponent(source), a = document.createElement('div'), b = document.createElement('div');
  const left = inst.mount(a), right = inst.mount(b);
  const oldButton = a.querySelector('button')!, oldInput = a.querySelector('input')!;
  try {
    oldButton.click(); oldInput.value = 'active'; oldInput.dispatchEvent(new Event('input', { bubbles: true }));
    expect(inst.state.count()).toBe(2); expect(inst.state.name()).toBe('active');
    inst.hotReload(source.replace('v1:', 'v2:'));
    oldButton.click(); oldInput.value = 'stale'; oldInput.dispatchEvent(new Event('input', { bubbles: true }));
    expect(inst.state.count()).toBe(2); expect(inst.state.name()).toBe('active');
    const currentButton = a.querySelector('button')!, currentInput = a.querySelector('input')!;
    left.unmount(); left.unmount(); currentButton.click(); currentInput.value = 'unmounted';
    currentInput.dispatchEvent(new Event('input', { bubbles: true }));
    expect(inst.state.count()).toBe(2); expect(inst.state.name()).toBe('active');
    const live = b.querySelector('input')!; live.value = 'live'; live.dispatchEvent(new Event('input', { bubbles: true }));
    b.querySelector('button')!.click(); expect(inst.state.count()).toBe(4); expect(inst.state.name()).toBe('live');
  } finally { left.unmount(); right.unmount(); inst.dispose(); }
  expect(getGraphTelemetry().nodes).toBe(0);
});

it('direct instance disposal retires mounted/rendered views and preserves external signals', () => {
  const external = signal(0);
  const source = `component App { state count: Number = 0; Column { Text { text:state.count; } Button { text:'add'; clicked -> state.increment('count',1); } } }`;
  const inst = compileFormaComponent(source, { viewModel: { count: external } });
  const a = document.createElement('div'), b = document.createElement('div'), c = document.createElement('div');
  const left = inst.mount(a), right = inst.mount(b); c.append(inst.render());
  const buttons = [a, b, c].map(container => container.querySelector('button')!);
  try {
    inst.dispose(); inst.dispose();
    expect(getGraphTelemetry().nodes).toBe(1);
    for (const button of buttons) button.click();
    expect(external()).toBe(0); expect(a.childNodes.length + b.childNodes.length + c.childNodes.length).toBe(0);
    a.textContent = 'replacement'; left.unmount(); right.unmount(); expect(a.textContent).toBe('replacement');
    expect(() => inst.mount(a)).toThrow(/disposed/i);
    expect(() => inst.render()).toThrow(/disposed/i);
    expect(() => inst.hotReload(source)).toThrow(/disposed/i);
  } finally { left.unmount(); right.unmount(); inst.dispose(); external.dispose(); }
  expect(getGraphTelemetry().nodes).toBe(0);
});

it.each(['mount', 'render'] as const)('failed child %s creation retires child signals and allows a retry', mode => {
  const failure = Error('child render failed'); let fail = true;
  const child = `component Child { state value: Number = 2; Text { text:state.value; } }`;
  const inst = compileFormaComponent(`component App { state count: Number = 1; Column { Text { key:'label'; text:state.count; } Child { key:'child'; } } }`, {
    components: { Child: () => {
      const instance = compileFormaComponent(child), render = instance.render.bind(instance);
      instance.render = () => { if (fail) throw failure; return render(); };
      return instance;
    } },
  });
  const baseline = getGraphTelemetry().nodes, container = document.createElement('div');
  let handle: { unmount(): void } | undefined;
  try {
    expect(() => mode === 'mount' ? inst.mount(container) : inst.render()).toThrow(failure);
    expect(getGraphTelemetry().nodes).toBe(baseline);
    expect(container.childNodes.length).toBe(0);
    expect(inst.getElementByKey('label')).toBeUndefined(); expect(inst.getElementByKey('child')).toBeUndefined();
    fail = false; handle = inst.mount(container); expect(container.textContent).toBe('12');
  } finally { handle?.unmount(); inst.dispose(); }
  expect(getGraphTelemetry().nodes).toBe(0);
});


it('a failed extra view preserves the existing live key map and unmount restores sibling keys', () => {
  let fail = false;
  const child = `component Child { Text { text:'child'; } }`;
  const inst = compileFormaComponent(`component App { Column { Text { key:'label'; text:'live'; } Child {} } }`, {
    components: { Child: () => { const childInst = compileFormaComponent(child); if (fail) childInst.render = () => { throw Error('refused'); }; return childInst; } },
  });
  const a = document.createElement('div'), b = document.createElement('div'), c = document.createElement('div');
  const left = inst.mount(a), first = inst.getElementByKey('label'); let right: { unmount(): void } | undefined;
  try {
    const baseline = getGraphTelemetry().nodes; fail = true;
    expect(() => inst.mount(c)).toThrow('refused'); expect(getGraphTelemetry().nodes).toBe(baseline);
    expect(inst.getElementByKey('label')).toBe(first); expect(a.textContent).toBe('livechild');
    fail = false; right = inst.mount(b); expect(inst.getElementByKey('label')).not.toBe(first);
    right.unmount(); expect(inst.getElementByKey('label')).toBe(first);
  } finally { left.unmount(); right?.unmount(); inst.dispose(); }
  expect(getGraphTelemetry().nodes).toBe(0);
});

it.each(['factory', 'child-render'] as const)('parent disposal during %s refuses the unfinished view and drains child resources', phase => {
  const childSource = `component Child { state value: Number = 2; Text { text:state.value; } }`;
  let inst: ReturnType<typeof compileFormaComponent>;
  inst = compileFormaComponent(`component App { state count: Number = 1; Column { Child {} Text { text:state.count; } } }`, {
    components: { Child: () => {
      const child = compileFormaComponent(childSource), render = child.render.bind(child);
      if (phase === 'factory') inst.dispose();
      else child.render = () => { inst.dispose(); return render(); };
      return child;
    } },
  });
  const container = document.createElement('div');
  expect(() => inst.mount(container)).toThrow(/disposed/i);
  expect(container.childNodes.length).toBe(0);
  expect(getGraphTelemetry().nodes).toBe(0);
  inst.dispose();
});

it('disposal during DOM connection refuses the mount and retires registered resources', () => {
  let inst: ReturnType<typeof compileFormaComponent>;
  customElements.define('svet-dispose-on-connect', class extends HTMLElement { connectedCallback() { inst.dispose(); } });
  inst = compileFormaComponent(`component App { Column { Child {} } }`, { components: { Child: () => {
    const child = compileFormaComponent(`component Child { Text { text:'child'; } }`);
    child.render = () => document.createElement('svet-dispose-on-connect') as unknown as DocumentFragment;
    return child;
  } } });
  const container = document.createElement('div'); document.body.append(container);
  try {
    expect(() => inst.mount(container)).toThrow(/disposed/i);
    expect(container.childNodes.length).toBe(0); expect(getGraphTelemetry().nodes).toBe(0);
  } finally { inst.dispose(); container.remove(); }
});

it('disposal inside a mounted view update drains the child returned after scope retirement', () => {
  let retire = false, inst: ReturnType<typeof compileFormaComponent>;
  const source = `component App { Column { Child {} } }`;
  inst = compileFormaComponent(source, { components: { Child: () => {
    const child = compileFormaComponent(`component Child { state value: Number = 2; Text { text:state.value; } }`);
    if (retire) inst.dispose();
    return child;
  } } });
  const container = document.createElement('div'), handle = inst.mount(container);
  try {
    retire = true; expect(() => inst.hotReload(source)).toThrow(/disposed/i);
    expect(container.childNodes.length).toBe(0); expect(getGraphTelemetry().nodes).toBe(0);
  } finally { handle.unmount(); inst.dispose(); }
});

it('unmount inside a child factory during update releases the returned child and keeps instance reusable', () => {
  let retire = false, handle: { unmount(): void };
  const source = `component App { Column { Child {} } }`;
  const inst = compileFormaComponent(source, { components: { Child: () => {
    const child = compileFormaComponent(`component Child { state value: Number = 2; Text { text:state.value; } }`);
    if (retire) handle.unmount();
    return child;
  } } });
  const baseline = getGraphTelemetry().nodes, container = document.createElement('div'); handle = inst.mount(container);
  try {
    retire = true; expect(() => inst.hotReload(source)).toThrow(/disposed/i);
    expect(container.childNodes.length).toBe(0); expect(getGraphTelemetry().nodes).toBe(baseline);
    retire = false; handle = inst.mount(container); expect(container.textContent).toBe('2');
  } finally { handle.unmount(); inst.dispose(); }
  expect(getGraphTelemetry().nodes).toBe(0);
});

it('failed HMR child rendering preserves the mounted DOM, callbacks and key lookup until retry', () => {
  let fail = false;
  const source = `component App { state count: Number = 0; Column { Button { key:'inc'; text:'old'; clicked -> state.increment('count',1); } Child {} } }`;
  const inst = compileFormaComponent(source, { components: { Child: () => {
    const child = compileFormaComponent(`component Child { state value: Number = 2; Text { text:state.value; } }`);
    const render = child.render.bind(child); child.render = () => { if (fail) throw Error('stage refused'); return render(); }; return child;
  } } });
  const container = document.createElement('div'), handle = inst.mount(container);
  const button = container.querySelector('button')!, baseline = getGraphTelemetry().nodes;
  try {
    fail = true; expect(() => inst.hotReload(source.replace("text:'old'", "text:'new'"))).toThrow('stage refused');
    expect(container.querySelector('button')).toBe(button); expect(container.textContent).toBe('old2');
    expect(inst.getElementByKey('inc')).toBe(button); expect(getGraphTelemetry().nodes).toBe(baseline);
    button.click(); expect(inst.state.count()).toBe(1);
    fail = false; inst.hotReload(source.replace("text:'old'", "text:'recovered'"));
    expect(container.textContent).toBe('recovered2'); button.click(); expect(inst.state.count()).toBe(1);
    expect(getGraphTelemetry().nodes).toBe(baseline);
  } finally { handle.unmount(); inst.dispose(); }
  expect(getGraphTelemetry().nodes).toBe(0);
});

it('failed staged DOM insertion removes candidate nodes and preserves the previous view', () => {
  const source = `component App { state count: Number = 0; Button { key:'inc'; text:'old'; clicked -> state.increment('count',1); } Text { text:'tail'; } }`;
  const inst = compileFormaComponent(source), container = document.createElement('div'), handle = inst.mount(container);
  const original = container.insertBefore.bind(container), button = container.querySelector('button')!, baseline = getGraphTelemetry().nodes;
  let inserts = 0;
  container.insertBefore = ((node: Node, anchor: Node | null) => { if (++inserts === 2) throw Error('insertion refused'); return original(node, anchor); }) as typeof container.insertBefore;
  try {
    expect(() => inst.hotReload(source.replace("text:'old'", "text:'new'"))).toThrow('insertion refused');
    expect(container.textContent).toBe('oldtail'); expect(container.querySelector('button')).toBe(button);
    expect(inst.getElementByKey('inc')).toBe(button); expect(getGraphTelemetry().nodes).toBe(baseline);
    button.click(); expect(inst.state.count()).toBe(1);
    container.insertBefore = original; inst.hotReload(source.replace("text:'old'", "text:'retry'"));
    expect(container.textContent).toBe('retrytail'); expect(getGraphTelemetry().nodes).toBe(baseline);
  } finally { container.insertBefore = original; handle.unmount(); inst.dispose(); }
  expect(getGraphTelemetry().nodes).toBe(0);
});

it('prepares every mounted view before retiring any previous view on HMR', () => {
  let update = false, calls = 0;
  const source = `component App { Button { key:'button'; text:'old'; } Child {} }`;
  const inst = compileFormaComponent(source, { components: { Child: () => {
    const child = compileFormaComponent(`component Child { state value: Number = 2; Text { text:state.value; } }`), render = child.render.bind(child);
    child.render = () => { if (update && ++calls === 2) throw Error('second view refused'); return render(); }; return child;
  } } });
  const a = document.createElement('div'), b = document.createElement('div'), left = inst.mount(a), right = inst.mount(b);
  const oldA = a.querySelector('button'), oldB = b.querySelector('button'), baseline = getGraphTelemetry().nodes;
  try {
    update = true; expect(() => inst.hotReload(source.replace("text:'old'", "text:'new'"))).toThrow('second view refused');
    expect(a.querySelector('button')).toBe(oldA); expect(b.querySelector('button')).toBe(oldB);
    expect(a.textContent).toBe('old2'); expect(b.textContent).toBe('old2'); expect(getGraphTelemetry().nodes).toBe(baseline);
    update = false; inst.hotReload(source.replace("text:'old'", "text:'retry'"));
    expect(a.textContent).toBe('retry2'); expect(b.textContent).toBe('retry2'); expect(getGraphTelemetry().nodes).toBe(baseline);
  } finally { left.unmount(); right.unmount(); inst.dispose(); }
  expect(getGraphTelemetry().nodes).toBe(0);
});

it('failed preparation restores document data, original computed handles and reactive old DOM', () => {
  let fail = false;
  const source = `component App { state count: Number = 2; #[range(0,10)] prop limit: Number = 5; let doubled = state.count * 2; Column { Text { text:doubled; } Child {} } }`;
  const inst = compileFormaComponent(source, { components: { Child: () => {
    const child = compileFormaComponent(`component Child { Text { text:'child'; } }`), render = child.render.bind(child);
    child.render = () => { if (fail) throw Error('document refused'); return render(); }; return child;
  } } });
  const container = document.createElement('div'), handle = inst.mount(container), oldLet = inst.computedLets.doubled, baseline = getGraphTelemetry().nodes;
  try {
    fail = true;
    const candidate = source.replace('state count: Number = 2;', 'state count: Number = 2; state added: Number = 9; prop extra: String = \'candidate\';').replace('range(0,10)', 'range(0,1)').replace('state.count * 2', 'state.count * 3');
    expect(() => inst.hotReload(candidate)).toThrow('document refused');
    expect(inst.computedLets.doubled).toBe(oldLet); expect(oldLet()).toBe(4); expect(inst.isValid()).toBe(true);
    expect('added' in inst.state).toBe(false); expect('extra' in inst.props).toBe(false); expect(getGraphTelemetry().nodes).toBe(baseline);
    inst.state.count.set(3); expect(oldLet()).toBe(6); expect(container.textContent).toBe('6child');
    fail = false; inst.hotReload(source.replace('state.count * 2', 'state.count * 4'));
    expect(container.textContent).toBe('12child'); expect(getGraphTelemetry().nodes).toBe(baseline);
  } finally { handle.unmount(); inst.dispose(); }
  expect(getGraphTelemetry().nodes).toBe(0);
});

it('refuses a nested document update during preparation and leaves the original view usable', () => {
  let nested = false;
  const source = `component App { state count: Number = 1; Text { text:state.count; } Child {} }`;
  const inst = compileFormaComponent(source, { components: { Child: () => {
    const child = compileFormaComponent(`component Child { Text { text:'child'; } }`);
    if (nested) { child.dispose(); inst.hotReload(source); } return child;
  } } });
  const container = document.createElement('div'), handle = inst.mount(container), baseline = getGraphTelemetry().nodes;
  try {
    nested = true; expect(() => inst.hotReload(source)).toThrow(/already in progress/);
    expect(container.textContent).toBe('1child'); expect(getGraphTelemetry().nodes).toBe(baseline);
    inst.state.count.set(2); expect(container.textContent).toBe('2child');
    nested = false; inst.hotReload(source); expect(container.textContent).toBe('2child');
  } finally { handle.unmount(); inst.dispose(); }
  expect(getGraphTelemetry().nodes).toBe(0);
});

it.each(['dispose', 'unmount'] as const)('retirement during old child cleanup at commit %s releases the candidate view', mode => {
  let retire = false, handle: { unmount(): void };
  const source = `component App { Column { Child {} } }`;
  const inst = compileFormaComponent(source, { components: { Child: () => {
    const child = compileFormaComponent(`component Child { state value: Number = 2; Text { text:state.value; } }`), dispose = child.dispose.bind(child);
    child.dispose = () => { dispose(); if (retire) { if (mode === 'dispose') inst.dispose(); else handle.unmount(); } }; return child;
  } } });
  const baseline = getGraphTelemetry().nodes, container = document.createElement('div'); handle = inst.mount(container);
  try {
    retire = true; expect(() => inst.hotReload(source)).toThrow(/disposed/i);
    expect(container.childNodes.length).toBe(0); expect(getGraphTelemetry().nodes).toBe(mode === 'dispose' ? 0 : baseline);
    if (mode === 'unmount') { retire = false; handle = inst.mount(container); expect(container.textContent).toBe('2'); }
  } finally { retire = false; handle.unmount(); inst.dispose(); }
  expect(getGraphTelemetry().nodes).toBe(0);
});

it('failed child factory disposes Forma instances created before it throws and permits retry', () => {
  let fail = true;
  const inst = compileFormaComponent(`component App { Column { Child {} } }`, { components: { Child: () => {
    const child = compileFormaComponent(`component Child { state value: Number = 2; Text { text:state.value; } }`);
    if (fail) throw Error('factory refused before return'); return child;
  } } });
  const baseline = getGraphTelemetry().nodes, container = document.createElement('div'); let handle: { unmount(): void } | undefined;
  try {
    expect(() => inst.mount(container)).toThrow('factory refused before return');
    expect(getGraphTelemetry().nodes).toBe(baseline); expect(container.childNodes.length).toBe(0);
    fail = false; handle = inst.mount(container); expect(container.textContent).toBe('2');
  } finally { handle?.unmount(); inst.dispose(); }
  expect(getGraphTelemetry().nodes).toBe(0);
});

it.each(['props', 'locale', 'lets'] as const)('constructor refusal in %s releases owned allocations and preserves external signals', phase => {
  const external = signal(7), baseline = getGraphTelemetry().nodes;
  const ast = parseFormaSource(`component App { prop first: Number = 1; prop second: Number = 2; state own: Number = 3; let doubled = state.own * 2; Text { text:doubled; } }`);
  const reason = Error('constructor refused');
  const props = phase === 'props' ? { first: 1, get second() { throw reason; } } : undefined;
  const options = { viewModel: { external }, props, get locale() { if (phase === 'locale') throw reason; return 'ru'; } };
  if (phase === 'lets') ast.letExpressions.push({ get name(): string { throw reason; }, expr: '1' });
  try { expect(() => compileFormaComponent(ast, options)).toThrow(reason); expect(getGraphTelemetry().nodes).toBe(baseline); external.set(9); expect(external()).toBe(9); }
  finally { external.dispose(); }
  expect(getGraphTelemetry().nodes).toBe(0);
});
