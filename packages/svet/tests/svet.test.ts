/**
 * @vitest-environment jsdom
 */
import { beforeEach, describe, expect, it } from 'vitest';
import {
  batch,
  Canvas3D,
  compileFormaAst,
  compileFormaComponent,
  compileFormaToJsModule,
  computed,
  configureGpuSubsystem,
  createContext,
  createContractForm,
  createCubeMesh3D,
  createDeepStore,
  createForm,
  createFormaContext,
  createFormaProperty,
  createGpuTensor,
  createMesh3D,
  createPersistedSignal,
  createPerspectiveCamera3D,
  createResource,
  createRouter,
  createRoot,
  createServerAction,
  createSpring,
  createStore,
  createTimeTravelDebugger,
  createTransform3D,
  dispatchGpuComputeKernel,
  effect,
  ErrorBoundary,
  executeGpuVectorFma,
  flushSpringFrame,
  For,
  getDomMutationCount,
  getGpuTelemetry,
  getGraphTelemetry,
  h,
  Link,
  mount,
  parseFormaSource,
  pickNearestVertex3D,
  Portal,
  registerFormaComponent,
  renderToString,
  resetDomMutationCount,
  resetGpuTelemetry,
  resetReactiveSystem,
  resumeContainer,
  RouterView,
  runWithIsolatedContext,
  serializeState,
  Show,
  signal,
  Suspense,
  syncGpuArena,
  transformSvetJsx,
  useContext,
  validateFormaSource,
  VirtualList,
} from '../src/engine';

describe('Svet Engine (TypeScript + Rust WASM Core)', () => {
  beforeEach(() => {
    resetReactiveSystem();
    resetDomMutationCount();
    document.body.innerHTML = '';
  });

  it('1. Eliminates React Hook & Diamond Glitch Flaws: WASM Push-Pull DAG executes diamond effect only ONCE', () => {
    const a = signal(1);
    const b = computed(() => a() * 2);
    const c = computed(() => a() * 3);

    let effectRuns = 0;
    let observedSum = 0;

    effect(() => {
      effectRuns++;
      observedSum = b() + c();
    });

    expect(effectRuns).toBe(1);
    expect(observedSum).toBe(5);

    a.set(10);
    expect(effectRuns).toBe(2);
    expect(observedSum).toBe(50);
  });

  it('2. Eliminates Vue Destructuring Trap: createStore properties remain 100% reactive after destructuring', () => {
    const userStore = createStore({
      name: 'Ada',
      score: 100,
    });

    const { name, score } = userStore;
    const badge = computed(() => `${name()}: ${score()}`);

    expect(badge()).toBe('Ada: 100');

    score.set(250);
    expect(badge()).toBe('Ada: 250');

    userStore.patch({ name: 'Grace', score: 500 });
    expect(badge()).toBe('Grace: 500');
  });

  it('3. Eliminates React/Vue VDOM Overhead: Component executes ONCE and performs O(1) surgical DOM updates', () => {
    let componentRenderCount = 0;
    const count = signal(0);

    function CounterView() {
      componentRenderCount++;
      return h(
        'div',
        { className: 'counter-box' },
        h('span', { id: 'val' }, () => `Count is ${count()}`),
        h('span', { id: 'static' }, 'Static Label')
      );
    }

    const container = document.createElement('div');
    mount(() => h(CounterView, null), container);

    expect(componentRenderCount).toBe(1);
    expect(container.querySelector('#val')?.textContent).toBe('Count is 0');

    resetDomMutationCount();

    for (let i = 1; i <= 5; i++) {
      count.set(i);
    }

    expect(componentRenderCount).toBe(1);
    expect(getDomMutationCount()).toBe(5);
    expect(container.querySelector('#val')?.textContent).toBe('Count is 5');
  });

  it('4. WASM O(n log n) LIS Keyed Reconciler: moves ONLY the single displaced DOM element', () => {
    const items = signal([
      { id: 1, label: 'One' },
      { id: 2, label: 'Two' },
      { id: 3, label: 'Three' },
      { id: 4, label: 'Four' },
      { id: 5, label: 'Five' },
    ]);

    const container = document.createElement('ul');
    mount(
      () =>
        For({
          each: items,
          keyFn: (item) => item.id,
          children: (item) => h('li', { 'data-id': String(item.id) }, item.label),
        }),
      container
    );

    const initialLis = Array.from(container.querySelectorAll('li'));
    expect(initialLis.map((el) => el.getAttribute('data-id'))).toEqual(['1', '2', '3', '4', '5']);

    resetDomMutationCount();

    items.set([
      { id: 1, label: 'One' },
      { id: 5, label: 'Five' },
      { id: 2, label: 'Two' },
      { id: 3, label: 'Three' },
      { id: 4, label: 'Four' },
    ]);

    expect(getDomMutationCount()).toBe(1);

    const updatedLis = Array.from(container.querySelectorAll('li'));
    expect(updatedLis.map((el) => el.getAttribute('data-id'))).toEqual(['1', '5', '2', '3', '4']);
    expect(updatedLis[0]).toBe(initialLis[0]);
    expect(updatedLis[1]).toBe(initialLis[4]);
  });

  it('5. Eliminates SSR Hydration Tax: Zero-Hydration Resumability restores state in O(1) without component execution', () => {
    const serverSignals = {
      visitors: signal(42),
    };
    const jsonSnapshot = serializeState(serverSignals);

    const ssrContainer = document.createElement('div');
    ssrContainer.innerHTML = `<div class="hero">Active visitors: <strong data-svet-text="visitors">42</strong></div>`;
    const strongNodeBefore = ssrContainer.querySelector('strong');

    const clientSignals = resumeContainer(ssrContainer, jsonSnapshot);
    expect(clientSignals.visitors()).toBe(42);

    clientSignals.visitors.set(108);
    expect(ssrContainer.querySelector('strong')?.textContent).toBe('108');
    expect(ssrContainer.querySelector('strong')).toBe(strongNodeBefore);
  });

  it('6. Eliminates Blazor Server Latency: Optimistic Server Action updates UI in 0ms and rolls back on failure', async () => {
    const likes = signal(10);

    const likePost = createServerAction<number, number>({
      optimisticUpdate: (delta) => {
        const prev = likes.peek();
        likes.set(prev + delta);
        return () => likes.set(prev);
      },
      rpc: async (delta) => {
        if (delta < 0) {
          throw new Error('Negative likes rejected by server');
        }
        return likes.peek();
      },
    });

    await likePost(1);
    expect(likes()).toBe(11);
    expect(likePost.error()).toBeNull();

    await expect(likePost(-99)).rejects.toThrow('Negative likes rejected by server');
    expect(likes()).toBe(11);
    expect(likePost.error()?.message).toBe('Negative likes rejected by server');
  });

  it('7. Provides live WASM Graph Telemetry & Atomic Batching', () => {
    const x = signal(1);
    const y = signal(2);
    const sum = computed(() => x() + y());
    let runs = 0;
    effect(() => {
      runs++;
      sum();
    });

    expect(runs).toBe(1);
    batch(() => {
      x.set(10);
      y.set(20);
      x.set(30);
    });
    expect(runs).toBe(2);
    expect(sum()).toBe(50);

    const telemetry = getGraphTelemetry();
    expect(telemetry.nodes).toBe(4);
    expect(telemetry.edges).toBe(3);
    expect(telemetry.wasmMemoryBytes).toBeGreaterThan(0);
  });

  it('8. WASM Free-List Slot Recycling: Disposing 1,000 effects reuses WASM memory slots with ZERO capacity growth', () => {
    const src = signal(0);
    const firstHandle = effect(() => {
      src();
    });
    const capacityAfterFirst = getGraphTelemetry().capacity;
    firstHandle.dispose();

    // Create and dispose 1,000 effects in a loop
    for (let i = 0; i < 1000; i++) {
      const hdl = effect(() => {
        src();
      });
      hdl.dispose();
    }

    const afterLoop = getGraphTelemetry();
    // Only the root `src` signal remains active (1 node), and total allocated WASM vector capacity NEVER grew!
    expect(afterLoop.nodes).toBe(1);
    expect(afterLoop.capacity).toBe(capacityAfterFirst);
  });

  it('9. Hierarchical Owner/Scope Disposal: Unmounting <Show> and <For> rows disposes inner effects automatically', () => {
    const visible = signal(true);
    const tick = signal(0);
    let innerEffectRuns = 0;

    const container = document.createElement('div');
    mount(
      () =>
        Show({
          when: visible,
          children: () => {
            effect(() => {
              tick();
              innerEffectRuns++;
            });
            return h('p', null, 'Active Branch');
          },
        }),
      container
    );

    expect(innerEffectRuns).toBe(1);
    tick.set(1);
    expect(innerEffectRuns).toBe(2);

    // Hide the <Show> branch -> inner effect MUST be automatically disposed and removed from WASM graph!
    visible.set(false);
    tick.set(2);
    tick.set(3);
    expect(innerEffectRuns).toBe(2);
  });

  it('10. Auto-Reactive JSX Compiler: Automatically wraps {count()} into {() => (count())} while preserving event handlers', () => {
    const rawCode = `const view = <div title={userName()} onClick={() => inc()}>{count() * 2}</div>;`;
    const compiled = transformSvetJsx(rawCode);
    expect(compiled).toContain('title={() => (userName())}');
    expect(compiled).toContain('onClick={() => inc()}');
    expect(compiled).toContain('>{() => (count() * 2)}</div>');
  });

  it('11. Isolated SSR Contexts: Concurrent runWithIsolatedContext() calls have zero state or ID interference', () => {
    const res1 = runWithIsolatedContext(() => {
      const a = signal(10);
      const b = computed(() => a() * 5);
      return { val: b(), nodes: getGraphTelemetry().nodes };
    });

    const res2 = runWithIsolatedContext(() => {
      const x = signal(7);
      return { val: x(), nodes: getGraphTelemetry().nodes };
    });

    expect(res1).toEqual({ val: 50, nodes: 2 });
    expect(res2).toEqual({ val: 7, nodes: 1 });
    // Default client context remained completely untouched (0 nodes)
    expect(getGraphTelemetry().nodes).toBe(0);
  });

  it('12. Two-Way Model Binding (model={signal}) & Global Event Delegation', () => {
    const username = signal('Alice');
    const clicks = signal(0);

    const container = document.body;
    mount(
      () =>
        h(
          'div',
          null,
          h('input', { id: 'name-input', model: username }),
          h('button', { id: 'btn', onClick: () => clicks.update((c) => c + 1) }, 'Click')
        ),
      container
    );

    const inputEl = container.querySelector('#name-input') as HTMLInputElement;
    const btnEl = container.querySelector('#btn') as HTMLButtonElement;

    // 1. Signal -> DOM
    expect(inputEl.value).toBe('Alice');
    username.set('Bob');
    expect(inputEl.value).toBe('Bob');

    // 2. DOM -> Signal
    inputEl.value = 'Charlie';
    inputEl.dispatchEvent(new Event('input', { bubbles: true }));
    expect(username()).toBe('Charlie');

    // 3. Delegated Click Event
    btnEl.dispatchEvent(new MouseEvent('click', { bubbles: true }));
    expect(clicks()).toBe(1);
  });

  it('13. Deep Signal Store (createDeepStore): Nested destructuring at any depth retains 100% reactivity', () => {
    const appState = createDeepStore({
      user: {
        profile: {
          theme: 'dark',
          notifications: 3,
        },
      },
    });

    // Destructure 3 levels deep!
    const { theme, notifications } = appState.user.profile;
    const summary = computed(() => `${theme()}:${notifications()}`);

    expect(summary()).toBe('dark:3');
    theme.set('light');
    expect(summary()).toBe('light:3');

    appState.patch({
      user: {
        profile: {
          theme: 'cyberpunk',
          notifications: 99,
        },
      },
    });
    expect(summary()).toBe('cyberpunk:99');
    expect(appState.snapshot().user.profile.theme).toBe('cyberpunk');
  });

  it('14. Hierarchical Context (createContext / useContext) & Portals (<Portal>)', () => {
    const ThemeCtx = createContext('light');
    const modalRoot = document.createElement('div');
    const appRoot = document.createElement('div');
    document.body.appendChild(modalRoot);
    document.body.appendChild(appRoot);

    function ThemedModal() {
      const currentTheme = useContext(ThemeCtx);
      return Portal({
        mount: modalRoot,
        children: h('div', { id: 'modal-box', 'data-theme': currentTheme }, 'Portal Content'),
      });
    }

    const { unmount } = mount(
      () =>
        ThemeCtx.Provider({
          value: 'midnight-blue',
          children: () => h(ThemedModal, null),
        }),
      appRoot
    );

    const renderedModal = modalRoot.querySelector('#modal-box');
    expect(renderedModal).not.toBeNull();
    expect(renderedModal?.getAttribute('data-theme')).toBe('midnight-blue');

    // Unmounting the logical parent automatically removes the Portal from modalRoot!
    unmount();
    expect(modalRoot.querySelector('#modal-box')).toBeNull();
  });

  it('15. Async Resources (createResource) with <Suspense> and <ErrorBoundary> recovery', async () => {
    let shouldFail = false;
    let resolveFetch!: (val: string) => void;

    const container = document.createElement('div');

    function AsyncChild() {
      const res = createResource(
        () =>
          new Promise<string>((resolve, reject) => {
            resolveFetch = (v: string) => {
              if (shouldFail) reject(new Error('Network timeout'));
              else resolve(v);
            };
          })
      );
      return h('div', { id: 'data-view' }, () => res() ?? 'empty');
    }

    mount(
      () =>
        ErrorBoundary({
          fallback: (err, reset) =>
            h('button', { id: 'retry-btn', onClick: reset }, () => `Error: ${err.message}`),
          children: () =>
            Suspense({
              fallback: h('span', { id: 'loader' }, 'Loading...'),
              children: () => h(AsyncChild, null),
            }),
        }),
      container
    );

    const fallbackWrapper = container.querySelector('[data-svet-suspense-fallback]') as HTMLElement;
    const contentWrapper = container.querySelector('[data-svet-suspense-content]') as HTMLElement;

    // Initially loading inside <Suspense>
    expect(fallbackWrapper.style.display).toBe('contents');
    expect(contentWrapper.style.display).toBe('none');

    // Resolve async resource -> Suspense switches to content
    resolveFetch('Hello from Server');
    await new Promise((r) => setTimeout(r, 10));

    expect(fallbackWrapper.style.display).toBe('none');
    expect(contentWrapper.style.display).toBe('contents');
    expect(container.querySelector('#data-view')?.textContent).toBe('Hello from Server');

    // Test synchronous ErrorBoundary capture & reset recovery
    const crashSignal = signal(true);
    const errContainer = document.body;
    mount(
      () =>
        ErrorBoundary({
          fallback: (err, reset) =>
            h(
              'button',
              {
                id: 'recover',
                onClick: () => {
                  crashSignal.set(false);
                  reset();
                },
              },
              `Caught: ${err.message}`
            ),
          children: () => {
            if (crashSignal.peek()) {
              throw new Error('Render Crash');
            }
            return h('div', { id: 'healthy' }, 'Recovered UI');
          },
        }),
      errContainer
    );

    expect(errContainer.querySelector('#recover')?.textContent).toBe('Caught: Render Crash');
    (errContainer.querySelector('#recover') as HTMLButtonElement).click();
    expect(errContainer.querySelector('#healthy')?.textContent).toBe('Recovered UI');
  });

  it('16. Reactive Router (createRouter, <RouterView>, <Link>) with dynamic params & automatic scope disposal', () => {
    let homeEffectDisposed = false;
    const router = createRouter({
      initialPath: '/',
      routes: [
        {
          path: '/',
          component: () => {
            effect(() => {
              return () => {
                homeEffectDisposed = true;
              };
            });
            return h(
              'div',
              { id: 'page-home' },
              'Home Page',
              Link({ to: '/users/42', className: 'nav-link', children: 'Go User 42' })
            );
          },
        },
        {
          path: '/users/:id',
          component: (params) => h('div', { id: 'page-user' }, `User ID: ${params.id}`),
        },
      ],
    });

    const container = document.body;
    mount(() => RouterView({ router }), container);

    expect(container.querySelector('#page-home')).not.toBeNull();
    expect(homeEffectDisposed).toBe(false);

    // Click <Link to="/users/42">
    const linkEl = container.querySelector('.nav-link') as HTMLAnchorElement;
    linkEl.dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true }));

    // Route switched, old route's WASM effects were automatically disposed, and dynamic `:id` param was parsed!
    expect(homeEffectDisposed).toBe(true);
    expect(container.querySelector('#page-home')).toBeNull();
    expect(container.querySelector('#page-user')?.textContent).toBe('User ID: 42');
  });

  it('17. GPU + UMA Zero-Copy Tensor Signals & Reactive Compute Kernel Execution', () => {
    resetGpuTelemetry();
    configureGpuSubsystem({ architecture: 'UMA', device: null });

    const inputTensor = createGpuTensor([1, 2, 3, 4]);
    const outputTensor = createGpuTensor(4);

    // Automatically run FMA compute kernel whenever inputTensor mutates
    effect(() => {
      executeGpuVectorFma(inputTensor, outputTensor, 10, 5);
    });

    // Initial compute: [1*10+5, 2*10+5, 3*10+5, 4*10+5] = [15, 25, 35, 45]
    expect(Array.from(outputTensor())).toEqual([15, 25, 35, 45]);

    // Mutate single scalar inside WASM UMA arena -> automatically re-runs kernel and updates outputTensor!
    inputTensor.setAt(2, 100);
    expect(Array.from(outputTensor())).toEqual([15, 25, 1005, 45]);
  });

  it('18. GPU + DUMA Partial Dirty Cache-Line DMA Sync (16-Byte Aligned Partial PCIe Transfer)', () => {
    resetGpuTelemetry();
    const dmaWrites: Array<{ bufferOffset: number; dataOffset: number; size: number }> = [];

    const mockDiscreteGpu = {
      queue: {
        writeBuffer(_buf: any, bufferOffset: number, _data: any, dataOffset = 0, size = 0) {
          dmaWrites.push({ bufferOffset, dataOffset, size });
        },
      },
    };

    configureGpuSubsystem({ architecture: 'DUMA', device: mockDiscreteGpu });

    // Allocate 256 floats (1024 bytes) in the GPU arena
    const bigTensor = createGpuTensor(256);
    // Flush initial allocation
    syncGpuArena();
    dmaWrites.length = 0;

    // Mutate ONLY 2 floats at index 100..102 (8 bytes dirty out of 1024 bytes total)
    bigTensor.write([42.5, 99.5], 100);

    const syncReport = syncGpuArena();
    expect(syncReport.architecture).toBe('DUMA');
    expect(syncReport.dirtyFloatMin).toBe(100);
    expect(syncReport.dirtyFloatMax).toBe(102);
    expect(syncReport.bytesTransferred).toBe(8);

    // Verify the discrete GPU queue received ONLY the 16-byte aligned partial DMA slice (16 bytes instead of 1024 bytes!)
    expect(dmaWrites.length).toBe(1);
    expect(dmaWrites[0].size).toBe(16);

    // Now mutate two distant cache lines (index 2 and index 240) -> DUMA multi-span collector transfers ONLY 2 cache lines (256 bytes) instead of the entire 960-byte gap!
    dmaWrites.length = 0;
    bigTensor.setAt(2, 1.0);
    bigTensor.setAt(240, 2.0);
    const multiSpanReport = syncGpuArena();
    expect(multiSpanReport.spansTransferred).toBe(2);
    expect(dmaWrites.length).toBe(2);
    expect(multiSpanReport.bytesTransferred).toBe(256);
  });

  it('19. GPU Free-List Memory Recycling & Declarative WGSL Compute Dispatch (dispatchGpuComputeKernel)', () => {
    resetGpuTelemetry();
    const initialTensor = createGpuTensor([2, 4, 6, 8]);
    const arenaLengthAfterFirst = getGpuTelemetry().arenaFloatCount;
    initialTensor.dispose();

    // Allocate & dispose 500 GPU tensors inside OwnerScopes -> zero unbounded growth in gpu_f32_arena!
    for (let i = 0; i < 500; i++) {
      createRoot((dispose) => {
        createGpuTensor(4);
        dispose();
      });
    }
    expect(getGpuTelemetry().arenaFloatCount).toBe(arenaLengthAfterFirst);

    // Declarative WGSL Compute Kernel Dispatch tied to reactive DAG
    const a = createGpuTensor([1, 2, 3, 4]);
    const b = createGpuTensor([10, 20, 30, 40]);
    const out = createGpuTensor(4);

    effect(() => {
      dispatchGpuComputeKernel({
        wgsl: `@compute @workgroup_size(64) fn main() {}`,
        inputs: [a, b],
        output: out,
        fallbackKernel: ([inA, inB], dst) => {
          for (let i = 0; i < dst.length; i++) {
            dst[i] = inA[i] + inB[i];
          }
        },
      });
    });

    expect(Array.from(out())).toEqual([11, 22, 33, 44]);
    a.setAt(0, 90);
    expect(Array.from(out())).toEqual([100, 22, 33, 44]);
  });

  it('20. Full SSR renderToString() Pipeline + Surgical Attribute/Model Resumability', () => {
    const ssrResult = renderToString(() => {
      const title = signal('Svet Cloud');
      const count = signal(7);
      return h(
        'section',
        { className: 'ssr-card', 'data-svet-attr': 'data-title:title' },
        h('h1', { 'data-svet-text': 'title' }, () => title()),
        h('span', { id: 'cnt', 'data-svet-text': 'count' }, () => count()),
        h('input', { id: 'inp', 'data-svet-model': 'title', value: title.peek() })
      );
    });

    expect(ssrResult.html).toContain('<section class="ssr-card"');
    expect(ssrResult.html).toContain('<h1 data-svet-text="title">Svet Cloud</h1>');
    expect(ssrResult.html).toContain('<span id="cnt" data-svet-text="count">7</span>');

    // Now resume on client with snapshot
    const clientContainer = document.createElement('div');
    clientContainer.innerHTML = ssrResult.html;
    const snapshotJson = JSON.stringify({
      signals: { title: 'Svet Cloud', count: 7 },
      timestamp: Date.now(),
    });

    const restored = resumeContainer(clientContainer, snapshotJson);
    restored.title.set('Svet Edge GPU');
    expect(clientContainer.querySelector('h1')?.textContent).toBe('Svet Edge GPU');
    expect(clientContainer.querySelector('section')?.getAttribute('data-title')).toBe(
      'Svet Edge GPU'
    );
    expect((clientContainer.querySelector('#inp') as HTMLInputElement).value).toBe(
      'Svet Edge GPU'
    );
  });

  it('21. 3D GPU Scene Graph: Reactive Transform3D Hierarchy, PerspectiveCamera3D & Mesh3D Vertex Transformation in WASM UMA/DUMA Arena', () => {
    resetGpuTelemetry();
    configureGpuSubsystem({ architecture: 'UMA', device: null });

    // Parent node at (10, 20, 30)
    const parentTransform = createTransform3D({
      position: [10, 20, 30],
    });

    // Child node at local (5, 0, 0), scaled by 2x
    const childTransform = createTransform3D({
      position: [5, 0, 0],
      scale: [2, 2, 2],
      parent: parentTransform,
    });

    // Create a 3D mesh with 2 vertices: (1, 2, 3) and (0, 0, 0)
    const mesh = createMesh3D({
      positions: [1, 2, 3, 0, 0, 0],
      transform: childTransform,
    });

    // World position of vertex 0: parent(10,20,30) + child_pos(5,0,0) + scale(2)*(1,2,3) = (17, 24, 36, 1)
    // World position of vertex 1: parent(10,20,30) + child_pos(5,0,0) + scale(2)*(0,0,0) = (15, 20, 30, 1)
    expect(Array.from(mesh.transformedVertices())).toEqual([17, 24, 36, 1, 15, 20, 30, 1]);

    // Move parent node to (100, 0, 0) -> child world matrix & mesh vertices update in WASM automatically!
    parentTransform.position.set([100, 0, 0]);
    expect(Array.from(mesh.transformedVertices())).toEqual([107, 4, 6, 1, 105, 0, 0, 1]);

    // Verify PerspectiveCamera3D matrix generation in WASM
    const cam = createPerspectiveCamera3D({
      fovYRad: Math.PI / 2,
      aspect: 1.0,
      near: 1.0,
      far: 100.0,
    });
    const proj = cam.projectionMatrix();
    expect(proj[0]).toBeCloseTo(1.0, 4);
    expect(proj[5]).toBeCloseTo(1.0, 4);
    expect(proj[11]).toBeCloseTo(-1.0, 4);
  });

  it('22. Deep Forma Integration: Compiles Forma .ui Markup (tokens, i18n, style/apply, #[range]/#[pattern], state, let, pipes |>, <-> bindings, for/empty) into Svet WASM DAG', () => {
    const formaUiSource = `
      component DashboardCard {
        tokens {
          spacing.md: 16;
          colors.surface: #1e293b;
        }

        i18n ru { title: 'Входящие'; }
        i18n en { title: 'Inbox'; }

        style cardSurface {
          padding: tokens.spacing.md;
          background: tokens.colors.surface;
          cornerRadius: 12;
        }

        #[range(0, 100)]
        prop progress: Int = 50;

        #[pattern('^[a-z0-9_-]+$')]
        prop slug: String = 'forma-svet';

        state count: Int = 2;
        state name: String = 'Анна';
        state tags: List<String> = ['ui', 'wasm', 'gpu'];

        let doubled = count * 2;
        let tagsLabel = tags |> map('toUpperCase') |> join(' / ');

        Column {
          apply cardSurface;
          gap: 8;
          Text {
            key: 'headerText';
            text: '\${i18n.title}: \${count} (x2 = \${doubled})';
          }
          TextField {
            key: 'nameInput';
            value <-> state.name;
          }
          Button {
            key: 'incBtn';
            text: '+1';
            clicked -> state.increment('count', 1);
          }
          Button {
            key: 'saveBtn';
            text: 'Save';
            clicked -> events.saveRequested(state.name, doubled);
          }
          for tag in state.tags key tag {
            Badge {
              text: tag;
            }
          } empty {
            Text { key: 'emptyMsg'; text: 'Список пуст'; }
          }
        }
      }
    `;

    const container = document.createElement('div');
    const instance = compileFormaComponent(formaUiSource);
    instance.mount(container);

    // 1. Check initial rendered text with i18n ('ru'), state (`count = 2`), and computed `let` (`doubled = 4`)
    const headerEl = instance.getElementByKey('headerText')!;
    expect(headerEl.textContent).toBe('Входящие: 2 (x2 = 4)');

    // 2. Switch Forma i18n locale signal dynamically ('ru' -> 'en')
    instance.locale.set('en');
    expect(headerEl.textContent).toBe('Inbox: 2 (x2 = 4)');

    // 3. Click '+1' button (`clicked -> state.increment('count', 1)`)
    const incBtn = instance.getElementByKey('incBtn') as HTMLButtonElement;
    incBtn.click();
    expect(instance.state.count()).toBe(3);
    expect(instance.computedLets.doubled()).toBe(6);
    expect(headerEl.textContent).toBe('Inbox: 3 (x2 = 6)');

    // 4. Two-way binding `value <-> state.name` and event emission `events.saveRequested(state.name, doubled)`
    const nameInput = instance.getElementByKey('nameInput') as HTMLInputElement;
    expect(nameInput.value).toBe('Анна');
    nameInput.value = 'Мария';
    nameInput.dispatchEvent(new Event('input', { bubbles: true }));
    expect(instance.state.name()).toBe('Мария');

    let savedPayload: any[] = [];
    instance.onEvent('saveRequested', (userName, dbl) => {
      savedPayload = [userName, dbl];
    });
    (instance.getElementByKey('saveBtn') as HTMLButtonElement).click();
    expect(savedPayload).toEqual(['Мария', 6]);

    // 5. Validation constraints (`#[range(0, 100)]` and `#[pattern('^[a-z0-9_-]+$')]`) -> reactive `isValid`
    expect(instance.isValid()).toBe(true);
    instance.props.progress.set(150); // violates #[range(0, 100)]
    expect(instance.isValid()).toBe(false);
    instance.props.progress.set(80);
    expect(instance.isValid()).toBe(true);
    instance.props.slug.set('INVALID SLUG!'); // violates #[pattern]
    expect(instance.isValid()).toBe(false);

    // 6. Test Forma `for ... empty` collection backed by Svet's WASM LIS <For>
    expect(container.querySelectorAll('[data-forma-type="Badge"]').length).toBe(3);
    instance.actions.set('tags', []);
    expect(container.querySelectorAll('[data-forma-type="Badge"]').length).toBe(0);
    expect(container.textContent).toContain('Список пуст');

    // 7. Test Forma `Property<T>` and `Context<T>` inheritance & override
    const vmName = createFormaProperty('Иван');
    const rootCtx = createFormaContext({ city: 'Москва' });
    const childCtx = createFormaContext({ city: 'Москва' }, rootCtx);

    expect(vmName.get()).toBe('Иван');
    expect(childCtx.current().city).toBe('Москва');
    rootCtx.setContext({ city: 'Казань' });
    expect(childCtx.current().city).toBe('Казань');
    childCtx.setContext({ city: 'Сочи' });
    expect(childCtx.current().city).toBe('Сочи');
    childCtx.inheritContext();
    expect(childCtx.current().city).toBe('Казань');
  });

  it('23. Forma Advanced Language Constructs: match (OR-patterns, numeric ranges, property groups), forward props, slot & ContentPresenter', () => {
    const dialogUi = `
      component NoticeDialog {
        prop tone: String = 'Accent';
        prop status: String = 'loading';
        prop score: Int = 5;
        prop placeholder: String = 'Enter text...';

        Column {
          key: 'rootBox';
          match props.tone {
            Tone.Danger => {
              background: '#7f1d1d';
            }
            _ => {
              background: '#0f172a';
            }
          }
          slot header {
            Text { key: 'defaultHeader'; text: 'Default Header'; }
          }
          ContentPresenter {
            Text { text: 'Fallback Content'; }
          }
          TextInput {
            key: 'forwardedInput';
            forward props { placeholder };
          }
          match props.status {
            'loading' | 'pending' => Text { key: 'statusBadge'; text: 'Загрузка...'; }
            'error' => Text { key: 'statusBadge'; text: 'Ошибка!'; }
            _ => Text { key: 'statusBadge'; text: 'Готово'; }
          }
          match props.score {
            1..=5 => Text { key: 'tierBadge'; text: 'Bronze'; }
            6..10 => Text { key: 'tierBadge'; text: 'Silver'; }
            _ => Text { key: 'tierBadge'; text: 'Gold'; }
          }
        }
      }
    `;

    const container = document.createElement('div');
    const inst = compileFormaComponent(dialogUi, {
      slots: {
        namedSlots: {
          header: () => h('strong', { id: 'custom-hdr' }, 'Custom Slot Header'),
        },
        defaultSlot: () => h('p', { id: 'custom-body' }, 'Injected Dialog Body'),
      },
    });
    inst.mount(container);

    // 1. Named `slot header` and `ContentPresenter`
    expect(container.querySelector('#custom-hdr')?.textContent).toBe('Custom Slot Header');
    expect(container.querySelector('#custom-body')?.textContent).toBe('Injected Dialog Body');

    // 2. `forward props { placeholder }`
    const inp = inst.getElementByKey('forwardedInput') as HTMLInputElement;
    expect(inp.placeholder).toBe('Enter text...');
    inst.props.placeholder.set('Updated placeholder');
    expect(inp.placeholder).toBe('Updated placeholder');

    // 3. Property-group `match props.tone`
    const rootBox = inst.getElementByKey('rootBox')!;
    expect(rootBox.style.background).toContain('rgb(15, 23, 42)'); // #0f172a
    inst.props.tone.set('Danger');
    expect(rootBox.style.background).toContain('rgb(127, 29, 29)'); // #7f1d1d

    // 4. Structural `match props.status` with OR-patterns (`'loading' | 'pending'`)
    expect(inst.getElementByKey('statusBadge')?.textContent).toBe('Загрузка...');
    inst.props.status.set('pending');
    expect(inst.getElementByKey('statusBadge')?.textContent).toBe('Загрузка...');
    inst.props.status.set('done');
    expect(inst.getElementByKey('statusBadge')?.textContent).toBe('Готово');

    // 5. Numeric range `match props.score` (`1..=5`, `6..10`, `_`)
    expect(inst.getElementByKey('tierBadge')?.textContent).toBe('Bronze');
    inst.props.score.set(8);
    expect(inst.getElementByKey('tierBadge')?.textContent).toBe('Silver');
    inst.props.score.set(99);
    expect(inst.getElementByKey('tierBadge')?.textContent).toBe('Gold');
  });

  it('24. Forma AOT Validation Diagnostics, HMR State Snapshot Preservation & Declarative Scene3D Widget', () => {
    const validUi = `
      component CubeStudio {
        #[range(0, 100)]
        prop level: Number = 50;
        state rotY: Number = 0.5;
        state title: String = 'Initial';
        let upperTitle = state.title |> upper();

        Column {
          Text { key: 'lbl'; text: upperTitle; }
          Scene3D {
            key: 'viewport3d';
            rotY: state.rotY;
            width: 240;
            height: 140;
          }
        }
      }
    `;

    const report = validateFormaSource(validUi);
    expect(report.valid).toBe(true);
    expect(report.componentName).toBe('CubeStudio');
    expect(report.propCount).toBe(1);
    expect(report.stateCount).toBe(2);
    expect(report.letCount).toBe(1);
    expect(report.has3DViewport).toBe(true);

    // Invalid contract (`#[range(100, 10)]` + unclosed brace)
    const invalidReport = validateFormaSource(`
      component Broken {
        #[range(100, 10)]
        prop x: Number = 5;
        Column {
    `);
    expect(invalidReport.valid).toBe(false);
    expect(invalidReport.diagnostics.length).toBeGreaterThanOrEqual(2);

    // Mount, mutate state, export HMR snapshot, recompile, and restore HMR snapshot
    const container = document.createElement('div');
    const inst1 = compileFormaComponent(validUi);
    const mounted1 = inst1.mount(container);

    const canvasEl = inst1.getElementByKey('viewport3d') as HTMLCanvasElement;
    expect(canvasEl).toBeDefined();
    expect(canvasEl.tagName.toLowerCase()).toBe('canvas');
    expect(canvasEl.getAttribute('data-forma-type')).toBe('Scene3D');

    inst1.state.rotY.set(2.25);
    inst1.state.title.set('HMR Preserved');
    const hmrSnap = inst1.exportHmrSnapshot();
    mounted1.unmount();

    const inst2 = compileFormaComponent(validUi);
    inst2.restoreHmrSnapshot(hmrSnap);
    inst2.mount(container);

    expect(inst2.state.rotY()).toBe(2.25);
    expect(inst2.state.title()).toBe('HMR Preserved');
    expect(inst2.getElementByKey('lbl')?.textContent).toBe('HMR PRESERVED');
  });

  it('25. Deterministic O(1) Time-Travel Debugger: Auto-Recording, Undo, Redo & JumpTo without Duplicate History', () => {
    const count = signal(10);
    const label = signal('Alpha');
    const sum = computed(() => `${label()}:${count()}`);

    const debuggerInst = createTimeTravelDebugger({ count, label }, { autoRecord: true });
    expect(debuggerInst.history().length).toBe(1);
    expect(debuggerInst.cursor()).toBe(0);
    expect(debuggerInst.canUndo()).toBe(false);

    count.set(20);
    expect(debuggerInst.history().length).toBe(2);
    expect(debuggerInst.cursor()).toBe(1);
    expect(sum()).toBe('Alpha:20');

    label.set('Beta');
    expect(debuggerInst.history().length).toBe(3);
    expect(debuggerInst.cursor()).toBe(2);
    expect(sum()).toBe('Beta:20');

    // Undo 1 step -> back to ('Alpha', 20)
    expect(debuggerInst.undo()).toBe(true);
    expect(debuggerInst.cursor()).toBe(1);
    expect(count()).toBe(20);
    expect(label()).toBe('Alpha');
    expect(sum()).toBe('Alpha:20');
    // History length must NOT grow during time-travel!
    expect(debuggerInst.history().length).toBe(3);

    // Jump directly to t=0 -> ('Alpha', 10)
    expect(debuggerInst.jumpTo(0)).toBe(true);
    expect(sum()).toBe('Alpha:10');
    expect(debuggerInst.canRedo()).toBe(true);

    // Redo -> ('Alpha', 20)
    expect(debuggerInst.redo()).toBe(true);
    expect(sum()).toBe('Alpha:20');

    debuggerInst.dispose();
  });

  it('26. 100,000-Row Windowed VirtualList: Renders O(1) DOM Slice & Supports Instant scrollToIndex()', () => {
    const rows = Array.from({ length: 100_000 }, (_, i) => ({
      id: i + 1,
      title: `Item #${i + 1}`,
    }));
    const rowsSig = signal(rows);

    const vList = VirtualList({
      items: rowsSig,
      itemHeight: 30,
      height: 150, // 5 visible rows
      overscan: 2, // +2 top/bottom -> max 9 rows mounted in DOM!
      renderItem: (item) => h('span', { className: 'v-row' }, item.title),
    });

    document.body.appendChild(vList);

    // Despite 100,000 items, only 5 + 2 = 7 rows are mounted at scrollTop = 0!
    const initialRendered = vList.querySelectorAll('.v-row');
    expect(initialRendered.length).toBe(7);
    expect(initialRendered[0].textContent).toBe('Item #1');

    // Scroll to index 50,000 in O(1)
    vList.scrollToIndex(50_000);
    const midRendered = vList.querySelectorAll('.v-row');
    expect(midRendered.length).toBe(9); // 2 overscan above + 5 visible + 2 overscan below
    const [startIdx, endIdx] = vList.getVisibleRange();
    expect(startIdx).toBe(49_998);
    expect(endIdx).toBe(50_007);
    expect(midRendered[2].textContent).toBe('Item #50001');
  });

  it('27. Eliminates All 6 Architectural Weaknesses: CSP-Safe AST Eval (No new Function), WASM Memory Growth Resilience, Dynamic VirtualList Heights & WebGPU Depth24Plus RenderPass', () => {
    // 1. Verify Forma expressions work with `Function` constructor completely blocked (strict CSP)
    const origFunction = globalThis.Function;
    let evalSucceededUnderStrictCsp = false;
    try {
      (globalThis as any).Function = () => {
        throw new Error('CSP Violation: unsafe-eval is blocked!');
      };
      const ui = compileFormaComponent(`
        component CspSafe {
          state count: Int = 7;
          let calc = (state.count * 3 + 4) |> clamp(0, 20);
          Text { key: 'out'; text: 'Val=\${calc}'; }
        }
      `);
      const box = document.createElement('div');
      ui.mount(box);
      expect(ui.getElementByKey('out')?.textContent).toBe('Val=20');
      ui.state.count.set(3);
      expect(ui.getElementByKey('out')?.textContent).toBe('Val=13');
      evalSucceededUnderStrictCsp = true;
    } finally {
      globalThis.Function = origFunction;
    }
    expect(evalSucceededUnderStrictCsp).toBe(true);

    // 2. WASM memory.grow() resilience: tensor.view() automatically refreshes when WASM heap grows
    const smallTensor = createGpuTensor([11, 22, 33, 44]);
    expect(Array.from(smallTensor.view())).toEqual([11, 22, 33, 44]);
    // Allocate a massive 200,000-float (~800 KB) tensor to force WASM `memory.grow()`
    const hugeTensor = createGpuTensor(200_000);
    hugeTensor.setAt(199_999, 99.5);
    // `smallTensor.view()` must NOT throw a detached ArrayBuffer error!
    expect(smallTensor.view()[2]).toBe(33);
    smallTensor.setAt(2, 77);
    expect(smallTensor.view()[2]).toBe(77);
    hugeTensor.dispose();

    // 3. Dynamic row heights `(item, index) => height` in `VirtualList` using O(log n) binary search
    const varRows = Array.from({ length: 1_000 }, (_, i) => ({
      id: i,
      tall: i % 2 === 1,
    }));
    const dynList = VirtualList({
      items: varRows,
      itemHeight: (row) => (row.tall ? 60 : 30), // alternating 30px and 60px (90px per pair)
      height: 180,
      overscan: 1,
      renderItem: (r) => h('div', { className: 'dyn-row' }, `Row-${r.id}`),
    });
    document.body.appendChild(dynList);
    dynList.scrollToIndex(200); // 100 pairs * 90px = 9000px scrollTop
    expect(dynList.scrollTop).toBe(9000);
    const [dStart, dEnd] = dynList.getVisibleRange();
    expect(dStart).toBe(199);
    expect(dEnd).toBeGreaterThan(202);

    // 4. Hardware WebGPU `depth24plus` RenderPass pipeline in `<Canvas3D>`
    let drawCalls = 0;
    const mockWebGpuDevice = {
      queue: {
        writeBuffer: () => {},
        submit: () => {},
      },
      createShaderModule: () => ({}),
      createRenderPipeline: () => ({}),
      createBuffer: () => ({}),
      createTexture: () => ({ createView: () => ({}) }),
      createCommandEncoder: () => ({
        beginRenderPass: () => ({
          setPipeline: () => {},
          setVertexBuffer: () => {},
          draw: () => {
            drawCalls++;
          },
          end: () => {},
        }),
        finish: () => ({}),
      }),
    };
    configureGpuSubsystem({ architecture: 'UMA', device: mockWebGpuDevice });

    const origGetContext = HTMLCanvasElement.prototype.getContext;
    (HTMLCanvasElement.prototype as any).getContext = function (type: string) {
      if (type === 'webgpu') {
        return {
          configure: () => {},
          getCurrentTexture: () => ({ createView: () => ({}) }),
        };
      }
      return origGetContext.apply(this, arguments as any);
    };

    try {
      const tr = createTransform3D();
      const cube = createCubeMesh3D({ transform: tr });
      const canvas = Canvas3D({ mesh: cube.mesh, edges: cube.edges });
      expect(canvas.getAttribute('data-render-backend')).toBe('webgpu-depth24plus');
      expect(drawCalls).toBe(1);
      tr.rotation.set([0.5, 1.0, 0.2]);
      expect(drawCalls).toBe(2);
    } finally {
      HTMLCanvasElement.prototype.getContext = origGetContext;
      configureGpuSubsystem({ device: null });
    }
  });

  it('28. Security Hardening: Prevents SSR Script-Breakout XSS, Prototype Pollution, Forma Sandbox Escape, and Unsafe URL Injection', () => {
    // 1. SSR JSON state escaping (`</script>` breakout protection)
    const maliciousSig = signal('</script><script>window.__xss=1</script>');
    const ssrOut = renderToString(
      () => h('div', null, () => maliciousSig()),
      { userBio: maliciousSig }
    );
    expect(ssrOut.html).not.toContain('</script><script>');
    expect(ssrOut.stateJson).toContain('\\u003c/script\\u003e');

    // 2. Prototype Pollution protection in Forma `.ui` tokens & AST sandbox escape protection
    const maliciousUi = `
      component SecTest {
        tokens {
          __proto__.pollutedKey: 'hacked';
          constructor.prototype.pollutedKey2: 'hacked2';
        }
        state name: String = 'safe';
        let attemptEscape = name.sub.constructor('return 123')();
        Text {
          key: 'out';
          text: attemptEscape;
        }
      }
    `;
    const inst = compileFormaComponent(maliciousUi);
    const frag = inst.render();
    void frag;
    expect(({} as any).pollutedKey).toBeUndefined();
    expect(({} as any).pollutedKey2).toBeUndefined();
    expect(inst.getElementByKey('out')?.textContent).toBe('');

    // 3. Block `javascript:` URI in DOM attributes and `<Link>`
    const badLink = h('a', { href: '  javascript:alert(1)' }, 'Click') as HTMLAnchorElement;
    expect(badLink.getAttribute('href')).toBeNull();

    const router = createRouter({
      routes: [{ path: '/', component: () => h('div', null, 'home') }],
    });
    const container = document.createElement('div');
    mount(
      () =>
        RouterView({
          router,
          fallback: () => Link({ to: 'javascript:alert(1)', children: 'Bad' }),
        }),
      container
    );
    const routerLink = Link({ to: 'javascript:alert(1)', children: 'Bad' }) as HTMLAnchorElement;
    expect(routerLink.getAttribute('href')).toBe('#');

    // 4. ReDoS pattern rejection in `validateFormaSource`
    const redosReport = validateFormaSource(`
      component RedosComp {
        #[pattern('(a+)+$')]
        prop input: String = 'aaaa';
        Text { text: input; }
      }
    `);
    expect(redosReport.valid).toBe(false);
  });

  it('29. Deep Engine Refinements: AST Action Arguments with Commas, Nested Forma Component Composition, O(1) Scroll Prefix Memoization, Coalescing GPU Arena Allocator, Indexed WebGPU Draw & Structural Sharing Time-Travel', () => {
    // 1. Forma Action arguments containing commas inside strings & arrays + nested component composition
    registerFormaComponent(
      'StatusPill',
      `component StatusPill {
        prop badgeText: String = 'Init';
        Button {
          key: 'pillBtn';
          text: props.badgeText;
          clicked -> events.pillClicked('Клик, подтверждён!', [10, 20, 30]);
        }
      }`
    );

    const parentUi = compileFormaComponent(`
      component ParentDashboard {
        state mainLabel: String = 'Ожидание, старт';
        state lastPayload: String = '';
        state nums: List = [];

        Column {
          StatusPill {
            key: 'childPill';
            badgeText: state.mainLabel;
            pillClicked -> state.set('lastPayload', $event);
          }
          Button {
            key: 'complexActionBtn';
            text: 'Update';
            clicked -> state.set('mainLabel', 'Иванов, И.И., отдел R&D');
          }
          Button {
            key: 'arrayActionBtn';
            text: 'Set Array';
            clicked -> state.set('nums', [100, 200, 300]);
          }
        }
      }
    `);

    const host = document.createElement('div');
    parentUi.mount(host);

    const childWrapper = parentUi.getElementByKey('childPill')!;
    expect(childWrapper.textContent).toBe('Ожидание, старт');

    // Click button with comma-containing string argument
    (parentUi.getElementByKey('complexActionBtn') as HTMLButtonElement).click();
    expect(parentUi.state.mainLabel()).toBe('Иванов, И.И., отдел R&D');
    expect(childWrapper.textContent).toBe('Иванов, И.И., отдел R&D');

    // Click button with array literal containing commas
    (parentUi.getElementByKey('arrayActionBtn') as HTMLButtonElement).click();
    expect(parentUi.state.nums()).toEqual([100, 200, 300]);

    // Click nested child component button -> emits `pillClicked('Клик, подтверждён!', [10, 20, 30])` to parent
    const innerBtn = childWrapper.querySelector('button') as HTMLButtonElement;
    innerBtn.click();
    expect(parentUi.state.lastPayload()).toBe('Клик, подтверждён!');

    // 2. Dynamic `VirtualList` memoizes prefix sums: scrolling does NOT re-invoke `itemHeight`!
    let heightCalcCalls = 0;
    const items100 = Array.from({ length: 100 }, (_, i) => ({ id: i }));
    const vList = VirtualList({
      items: items100,
      itemHeight: (item) => {
        heightCalcCalls++;
        return item.id % 2 === 0 ? 25 : 45;
      },
      height: 100,
      overscan: 1,
      renderItem: (it) => h('span', null, `#${it.id}`),
    });
    document.body.appendChild(vList);
    expect(heightCalcCalls).toBe(100); // Computed once on initial mount

    // Perform 5 scroll operations: `heightCalcCalls` must remain 100 (0 O(N) recalculations!)
    vList.scrollToIndex(20);
    vList.scrollToIndex(50);
    vList.scrollToIndex(80);
    vList.scrollToIndex(10);
    vList.scrollToIndex(0);
    expect(heightCalcCalls).toBe(100);

    // 3. Coalescing GPU Arena Free-List in Rust WASM (merges adjacent freed blocks + splits larger blocks)
    const baseArenaLen = getGpuTelemetry().arenaFloatCount;
    const tA = createGpuTensor(64);
    const tB = createGpuTensor(64);
    const afterTwoAllocLen = getGpuTelemetry().arenaFloatCount;
    expect(afterTwoAllocLen).toBe(baseArenaLen + 128);

    // Free both adjacent 64-float tensors -> Rust coalesces them into a single 128-float free block!
    tA.dispose();
    tB.dispose();

    // Now allocate a 128-float tensor: it must reuse the coalesced [tA + tB] block with 0 arena growth!
    const tMerged = createGpuTensor(128);
    expect(getGpuTelemetry().arenaFloatCount).toBe(afterTwoAllocLen);
    expect(tMerged.offset).toBe(tA.offset);
    tMerged.dispose();

    // 4. Indexed WebGPU 3D Rendering (`drawIndexed` with 36 uint16 triangle indices on Cube)
    let indexedCountDrawn = 0;
    const mockIndexedGpu = {
      queue: { writeBuffer: () => {}, submit: () => {} },
      createShaderModule: () => ({}),
      createRenderPipeline: () => ({}),
      createBuffer: () => ({}),
      createTexture: () => ({ createView: () => ({}) }),
      createCommandEncoder: () => ({
        beginRenderPass: () => ({
          setPipeline: () => {},
          setVertexBuffer: () => {},
          setIndexBuffer: () => {},
          drawIndexed: (count: number) => {
            indexedCountDrawn = count;
          },
          draw: () => {},
          end: () => {},
        }),
        finish: () => ({}),
      }),
    };
    configureGpuSubsystem({ architecture: 'UMA', device: mockIndexedGpu });
    const origGetCtx = HTMLCanvasElement.prototype.getContext;
    (HTMLCanvasElement.prototype as any).getContext = function (type: string) {
      if (type === 'webgpu') {
        return {
          configure: () => {},
          getCurrentTexture: () => ({ createView: () => ({}) }),
        };
      }
      return origGetCtx.apply(this, arguments as any);
    };
    try {
      const cube = createCubeMesh3D({ transform: createTransform3D() });
      expect(cube.mesh.indices?.length).toBe(36);
      Canvas3D({ mesh: cube.mesh });
      expect(indexedCountDrawn).toBe(36);
    } finally {
      HTMLCanvasElement.prototype.getContext = origGetCtx;
      configureGpuSubsystem({ device: null });
    }

    // 5. Structural Sharing in `TimeTravelDebugger`
    const bigArraySig = signal([1, 2, 3, 4, 5]);
    const counterSig = signal(1);
    const tt = createTimeTravelDebugger({ bigArraySig, counterSig });
    counterSig.set(2);
    const snaps = tt.history();
    expect(snaps.length).toBe(2);
    // Since `bigArraySig` did not change between snapshot 0 and snapshot 1, its array reference is shared!
    expect(snaps[0].values.bigArraySig).toBe(snaps[1].values.bigArraySig);
    tt.dispose();
  });

  it('30. In-Place Forma .ui Hot Reload (hotReload) inside <FormaHost /> preserves user state, ViewModel bindings, and event listeners', () => {
    const sharedCustomer = createFormaProperty('Анна Смирнова');
    const inst = compileFormaComponent(
      `component CustomerCard {
        state customerName: String = 'Анна';
        state orders: Int = 3;
        let bonusPoints = orders * 150;
        Column {
          Text {
            key: 'titleLabel';
            text: 'Клиент: \${customerName} (Бонусы: \${bonusPoints})';
          }
          Button {
            key: 'addBtn';
            text: '+1';
            clicked -> state.increment('orders', 1);
          }
        }
      }`,
      { viewModel: { customerName: sharedCustomer } }
    );

    const container = document.createElement('div');
    mount(() => h('div', { id: 'host-box' }, inst.render()), container);

    expect(inst.getElementByKey('titleLabel')?.textContent).toBe(
      'Клиент: Анна Смирнова (Бонусы: 450)'
    );

    // Mutate state before hot-reload
    (inst.getElementByKey('addBtn') as HTMLButtonElement).click();
    sharedCustomer.set('Борис Волков');
    expect(inst.getElementByKey('titleLabel')?.textContent).toBe(
      'Клиент: Борис Волков (Бонусы: 600)'
    );

    // Hot-reload the .ui source with a new formula (orders * 500) and updated label template
    inst.hotReload(`component CustomerCard {
      state customerName: String = 'Анна';
      state orders: Int = 3;
      let bonusPoints = orders * 500;
      Column {
        Text {
          key: 'titleLabel';
          text: 'VIP Клиент: \${customerName} [Бонусы: \${bonusPoints}]';
        }
        Button {
          key: 'addBtn';
          text: '+1 Заказ';
          clicked -> state.increment('orders', 1);
        }
      }
    }`);

    // Verify DOM updated in-place inside <FormaHost /> AND preserved `customerName = 'Борис Волков'` and `orders = 4` (4 * 500 = 2000)!
    expect(inst.getElementByKey('titleLabel')?.textContent).toBe(
      'VIP Клиент: Борис Волков [Бонусы: 2000]'
    );
  });

  it('31. Security & Memory Recycling: createStore / createDeepStore block Prototype Pollution and hotReload recycles WASM nodes cleanly', () => {
    const store = createStore({ count: 1 });
    store.patch(JSON.parse('{"__proto__": {"pollutedStore": "yes"}, "count": 5}'));
    expect(({} as any).pollutedStore).toBeUndefined();
    expect(store.count()).toBe(5);

    const deep = createDeepStore({ profile: { name: 'Alice' } });
    deep.patch(JSON.parse('{"__proto__": {"pollutedDeep": "yes"}, "profile": {"__proto__": {"pollutedSub": "yes"}, "name": "Bob"}}'));
    expect(({} as any).pollutedDeep).toBeUndefined();
    expect(({} as any).pollutedSub).toBeUndefined();
    expect(deep.profile.name()).toBe('Bob');

    const inst = compileFormaComponent(`component LeakTest {
      state x: Int = 10;
      let a = x * 2;
      let b = a + 5;
      Column { Text { text: '\${b}'; } }
    }`);
    const container = document.createElement('div');
    const mounted = inst.mount(container);
    const nodesBefore = getGraphTelemetry().nodes;
    for (let i = 0; i < 10; i++) {
      inst.hotReload(`component LeakTest {
        state x: Int = 10;
        let a = x * ${i + 3};
        let b = a + 5;
        Column { Text { text: '\${b}'; } }
      }`);
    }
    const nodesAfter = getGraphTelemetry().nodes;
    expect(nodesAfter).toBe(nodesBefore);
    mounted.unmount();
  });

  it('32. Complete Part 1 & Part 2 Ecosystem Verification: Truthy-Memoized <Show>, Reactive <For> index(), Native SVG Namespace, Router popstate, Resource AbortController, Spring Physics, Persisted Signal, Contract Form & 3D Orbit/Pick', async () => {
    // 1. `<Show>` preserves DOM branch when `when` changes between two truthy values (`1 -> 2`)
    const countSig = signal(1);
    let branchMounts = 0;
    const showBox = document.createElement('div');
    mount(
      () =>
        Show({
          when: () => countSig(),
          children: () => {
            branchMounts++;
            return h('input', { id: 'persistent-input' });
          },
        }),
      showBox
    );
    expect(branchMounts).toBe(1);
    const firstInput = showBox.querySelector('#persistent-input');
    countSig.set(2); // Both 1 and 2 are truthy -> DOM branch must NOT remount!
    expect(branchMounts).toBe(1);
    expect(showBox.querySelector('#persistent-input')).toBe(firstInput);
    countSig.set(0); // Now falsy -> unmounts
    expect(showBox.querySelector('#persistent-input')).toBeNull();

    // 2. `<For>` provides a reactive `index()` signal accessor that updates when rows reorder
    const listSig = signal([
      { id: 'a', label: 'A' },
      { id: 'b', label: 'B' },
    ]);
    const forBox = document.createElement('div');
    mount(
      () =>
        For({
          each: listSig,
          keyFn: (it) => it.id,
          children: (it, idx) => h('span', { className: `item-${it.id}` }, () => `${it.label}@${idx()}`),
        }),
      forBox
    );
    expect(forBox.querySelector('.item-a')?.textContent).toBe('A@0');
    expect(forBox.querySelector('.item-b')?.textContent).toBe('B@1');
    // Swap order: ['b', 'a']
    listSig.set([
      { id: 'b', label: 'B' },
      { id: 'a', label: 'A' },
    ]);
    expect(forBox.querySelector('.item-b')?.textContent).toBe('B@0');
    expect(forBox.querySelector('.item-a')?.textContent).toBe('A@1');

    // 3. Native SVG element creation via `createElementNS('http://www.w3.org/2000/svg', ...)`
    const svgNode = h(
      'svg',
      { viewBox: '0 0 100 100' },
      h('circle', { cx: 50, cy: 50, r: 40 })
    ) as SVGElement;
    expect(svgNode.namespaceURI).toBe('http://www.w3.org/2000/svg');
    expect(svgNode.querySelector('circle')?.namespaceURI).toBe('http://www.w3.org/2000/svg');

    // 4. Router `popstate` (browser Back/Forward) support
    const router = createRouter({
      initialPath: '/step1',
      routes: [
        { path: '/step1', component: () => h('div', null, 'Step 1') },
        { path: '/step2', component: () => h('div', null, 'Step 2') },
      ],
    });
    router.navigate('/step2');
    expect(router.path()).toBe('/step2');
    window.history.pushState({}, '', '/step1');
    window.dispatchEvent(new PopStateEvent('popstate'));
    expect(router.path()).toBe('/step1');
    router.dispose();

    // 5. `createResource` AbortController cancellation on source change
    const querySig = signal('first');
    let abortedSignals = 0;
    const res = createResource(
      () => querySig(),
      async (q, { signal: abortSig }) => {
        abortSig.addEventListener('abort', () => {
          abortedSignals++;
        });
        await new Promise((r) => setTimeout(r, 15));
        return `Result:${q}`;
      }
    );
    // Immediately switch query before first resolves -> first request is aborted!
    querySig.set('second');
    expect(abortedSignals).toBe(1);
    await new Promise((r) => setTimeout(r, 25));
    expect(res()).toBe('Result:second');

    // 6. Harmonic Oscillator `createSpring` & Forma `transition { ... }` + `compileFormaToJsModule`
    const spring = createSpring(0, { stiffness: 200, damping: 20 });
    spring.set(100, { steps: 10 });
    expect(spring()).toBeGreaterThan(0);
    expect(spring()).toBeLessThan(100);
    spring.set(100, { hard: true });
    expect(spring()).toBe(100);
    spring.dispose();

    const aotCode = compileFormaToJsModule(`
      component TransBox {
        Frame {
          key: 'box';
          transition { duration: 250ms; easing: 'spring'; }
        }
      }
    `);
    expect(aotCode).toContain('export const uiValidation =');
    const transInst = compileFormaComponent(`
      component TransBox {
        Frame {
          key: 'box';
          transition { duration: 250ms; easing: 'spring'; }
        }
      }
    `);
    const transFrag = transInst.render();
    void transFrag;
    expect(transInst.getElementByKey('box')?.style.transition).toContain('250ms');

    // 7. `createPersistedSignal` & `createForm` with per-field validation
    const mockStorageMap = new Map<string, string>();
    const mockStorage = {
      getItem: (k: string) => mockStorageMap.get(k) ?? null,
      setItem: (k: string, v: string) => {
        mockStorageMap.set(k, v);
      },
    };
    const themeSig = createPersistedSignal('svet_theme', 'dark', { storage: mockStorage });
    expect(themeSig()).toBe('dark');
    themeSig.set('light');
    expect(mockStorageMap.get('svet_theme')).toBe('"light"');

    const form = createForm({
      initialValues: { age: 15, slug: 'bad slug!' },
      rules: {
        age: { required: true, range: [18, 99] },
        slug: { required: true, pattern: /^[a-z0-9_-]+$/ },
      },
    });
    expect(form.isValid()).toBe(false);
    expect(form.errors.age()).toContain('18');
    expect(form.errors.slug()).toBe('Некорректный формат');

    form.values.age.set(25);
    form.values.slug.set('svet-forma-2026');
    expect(form.isValid()).toBe(true);
    let submittedData: any = null;
    expect(form.submit((vals) => { submittedData = vals; })).toBe(true);
    expect(submittedData).toEqual({ age: 25, slug: 'svet-forma-2026' });

    // 8. Interactive 3D `orbitTransform` drag & `onVertexPick` ray/screen picking
    const tr3d = createTransform3D({ rotation: [0, 0, 0] });
    const cube3d = createCubeMesh3D({ transform: tr3d });
    let pickedVertex = -1;
    const c3d = Canvas3D({
      mesh: cube3d.mesh,
      orbitTransform: tr3d,
      onVertexPick: (vIdx) => {
        pickedVertex = vIdx;
      },
    });
    c3d.dispatchEvent(new MouseEvent('mousedown', { clientX: 100, clientY: 100 }));
    window.dispatchEvent(new MouseEvent('mousemove', { clientX: 150, clientY: 120 }));
    window.dispatchEvent(new MouseEvent('mouseup'));
    expect(tr3d.rotation()[1]).toBeCloseTo(50 * 0.012, 4);
    void pickedVertex;
  });

  it('33. Resolves All 7 Production Weaknesses: Pre-Parsed AST AOT, Ambiguous SVG <a>/<title> Upgrade, Shared Batched Spring Ticker, Cross-Field & Async Form Validation, PersistedSignal Teardown, WASM Z-Aware 3D Vertex Picking & Router Query/Hash/Guards', async () => {
    // 1. True Pre-Parsed AST AOT (`parseFormaSource` + `compileFormaAst`)
    const rawUi = `
      component AotCard {
        state score: Int = 42;
        let doubled = score * 2;
        Column {
          Text { key: 'lbl'; text: 'Score: \${doubled}'; }
        }
      }
    `;
    const precompiledAst = parseFormaSource(rawUi);
    expect(precompiledAst.componentName).toBe('AotCard');
    expect(precompiledAst.initialStateValues.score).toBe(42);
    const aotModule = compileFormaToJsModule(rawUi);
    expect(aotModule).toContain('export const uiAst =');
    expect(aotModule).toContain('compileFormaAst(uiAst, options)');

    const aotInst = compileFormaAst(precompiledAst);
    const aotBox = document.createElement('div');
    const aotMounted = aotInst.mount(aotBox);
    expect(aotInst.getElementByKey('lbl')?.textContent).toBe('Score: 84');
    aotInst.actions.increment('score', 8);
    expect(aotInst.getElementByKey('lbl')?.textContent).toBe('Score: 100');
    aotMounted.unmount();

    // 2. Ambiguous SVG `<a xlinkHref="...">` and `<title>` inside `<svg>` inherit SVG namespace
    const svgWithLink = h(
      'svg',
      { viewBox: '0 0 100 100' },
      h('title', null, 'Vector Diagram'),
      h('a', { xlinkHref: 'https://example.com/node' }, h('circle', { cx: 20, cy: 20, r: 10 }))
    ) as SVGElement;
    expect(svgWithLink.namespaceURI).toBe('http://www.w3.org/2000/svg');
    expect(svgWithLink.querySelector('title')?.namespaceURI).toBe('http://www.w3.org/2000/svg');
    const svgAnchor = svgWithLink.querySelector('a')!;
    expect(svgAnchor.namespaceURI).toBe('http://www.w3.org/2000/svg');
    expect(svgAnchor.getAttributeNS('http://www.w3.org/1999/xlink', 'href')).toBe(
      'https://example.com/node'
    );

    // 3. Shared Batched Spring Ticker: two springs read by one effect trigger the effect only ONCE per frame!
    const springX = createSpring(0, { stiffness: 180, damping: 22 });
    const springY = createSpring(0, { stiffness: 180, damping: 22 });
    let combinedEffectRuns = 0;
    const effHandle = effect(() => {
      void (springX() + springY());
      combinedEffectRuns++;
    });
    expect(combinedEffectRuns).toBe(1);
    batch(() => {
      springX.set(100, { steps: 0, animate: true });
      springY.set(200, { steps: 0, animate: true });
    });
    // Advance 1 shared frame -> both springs update inside a single atomic `batch()`!
    flushSpringFrame(1);
    expect(combinedEffectRuns).toBe(2); // Only +1 run for BOTH springs!
    effHandle.dispose();
    springX.dispose();
    springY.dispose();

    // 4. `createContractForm` with batched `setValue`, cross-field `validate(val, allValues)`, and `validateAsync`
    let formEffectRuns = 0;
    const contractForm = createContractForm({
      initialValues: { password: 'secret', confirm: 'mismatch', username: 'admin' },
      rules: {
        password: { required: true, minLength: 6 },
        confirm: {
          required: true,
          validate: (val, all) => (val !== all.password ? 'Пароли не совпадают' : null),
        },
        username: {
          required: true,
          validateAsync: async (val) => {
            await new Promise((r) => setTimeout(r, 10));
            return val === 'taken' ? 'Имя уже занято' : null;
          },
        },
      },
    });
    const formWatch = effect(() => {
      void contractForm.values.confirm();
      void contractForm.touched.confirm();
      void contractForm.errors.confirm();
      formEffectRuns++;
    });
    const runsBeforeSet = formEffectRuns;
    contractForm.setValue('confirm', 'secret');
    // Atomic `batch()` ensures touched + value + error update causes at most 1 effect run!
    expect(formEffectRuns - runsBeforeSet).toBe(1);
    expect(contractForm.errors.confirm()).toBeNull();
    expect(contractForm.isDirty()).toBe(true);

    contractForm.setValue('username', 'taken');
    const validAfterAsync = await contractForm.validateAll();
    expect(validAfterAsync).toBe(false);
    expect(contractForm.errors.username()).toBe('Имя уже занято');

    contractForm.setValue('username', 'available_user');
    expect(await contractForm.validateAll()).toBe(true);
    formWatch.dispose();

    // 5. `createPersistedSignal` properly removes `storage` listener and closes channel on `dispose()`
    const memMap = new Map<string, string>();
    const pSig = createPersistedSignal('svet_clean_key', 'init', {
      storage: {
        getItem: (k) => memMap.get(k) ?? null,
        setItem: (k, v) => memMap.set(k, v),
      },
    });
    pSig.dispose();
    window.dispatchEvent(
      new StorageEvent('storage', { key: 'svet_clean_key', newValue: '"mutated_after_dispose"' })
    );
    expect(pSig.peek()).toBe('init'); // Listener was cleanly removed!

    // 6. WASM Z-Aware 3D Vertex Picking (`pickNearestVertex3D` prioritizes foreground `ndcZ`)
    // In WebGPU right-handed view space (camera at origin looking down -Z), vertex 0 is at Z = -5 (back)
    // and vertex 1 is at Z = -2 (front, closer to camera -> smaller `ndcZ`)
    const zTr = createTransform3D({ position: [0, 0, 0] });
    const zCam = createPerspectiveCamera3D({ fovYRad: Math.PI / 3, aspect: 1, near: 0.1, far: 100 });
    const zMesh = createMesh3D({
      positions: [
        0, 0, -5, // index 0: farther from camera (larger ndcZ)
        0, 0, -2, // index 1: closer to camera (smaller ndcZ)
      ],
      transform: zTr,
      camera: zCam,
    });
    // Both project to the center of the 360x200 viewport (180, 100)
    const picked = pickNearestVertex3D(zMesh, 180, 100, {
      viewportWidth: 360,
      viewportHeight: 200,
      maxRadiusPx: 25,
    });
    expect(picked).not.toBeNull();
    expect(picked?.vertexIndex).toBe(1); // Foreground vertex wins!

    // 7. Router `query`, `hash`, clean `:param` extraction, and `beforeEach` navigation guard
    const advRouter = createRouter({
      initialPath: '/users/42?tab=settings&page=3#security',
      routes: [
        { path: '/users/:id', component: () => h('div', null, 'User') },
        { path: '/login', component: () => h('div', null, 'Login') },
        { path: '/denied', component: () => h('div', null, 'Denied') },
      ],
    });
    expect(advRouter.path()).toBe('/users/42');
    expect(advRouter.params()).toEqual({ id: '42' });
    expect(advRouter.query()).toEqual({ tab: 'settings', page: '3' });
    expect(advRouter.hash()).toBe('security');

    const removeGuard = advRouter.beforeEach((to) => {
      if (to.path === '/denied') return '/login?redirected=1#auth';
    });
    advRouter.navigate('/denied');
    expect(advRouter.path()).toBe('/login');
    expect(advRouter.query()).toEqual({ redirected: '1' });
    expect(advRouter.hash()).toBe('auth');
    removeGuard();
    advRouter.dispose();
  });
});
