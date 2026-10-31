import {
  batch,
  Viewport3D,
  compileFormaComponent,
  computed,
  configureGpuSubsystem,
  createCubeMesh3D,
  createFormaProperty,
  createGpuTensor,
  createPerspectiveCamera3D,
  createStore,
  createTimeTravelDebugger,
  createTransform3D,
  effect,
  executeGpuVectorFma,
  For,
  FormaHost,
  getDomMutationCount,
  getGpuTelemetry,
  getGraphTelemetry,
  GpuSparkline,
  mount,
  signal,
  syncGpuArena,
  validateFormaSource,
  VirtualList,
} from '../engine';
import createCustomerCard from './CustomerCard.ui';

const DEFAULT_PLAYGROUND_UI = `component LiveStudioWidget {
  tokens {
    spacing.md: 12;
    colors.card: #0f172a;
    colors.accent: #38bdf8;
  }

  #[range(0, 100)]
  prop progress: Number = 65;

  state rotY: Number = 0.6;
  state rotX: Number = 0.4;
  state label: String = 'Svet + Forma 3D Studio';

  let titleUpper = state.label |> upper();

  Column {
    gap: tokens.spacing.md;
    padding: 12;
    background: tokens.colors.card;
    cornerRadius: 10;

    Text {
      key: 'studioTitle';
      text: titleUpper;
      color: tokens.colors.accent;
    }

    Scene3D {
      key: 'studioViewport3d';
      rotX: state.rotX;
      rotY: state.rotY;
      width: 300;
      height: 150;
    }

    Row {
      gap: 8;
      Button {
        key: 'spinBtn';
        text: 'Вращать 3D (+0.4 rad)';
        clicked -> state.increment('rotY', 0.4);
      }
      Button {
        key: 'tiltBtn';
        text: 'Наклон X (+0.3 rad)';
        clicked -> state.increment('rotX', 0.3);
      }
    }
  }
}`;

function App() {
  // 1. Destructurable Store (Fixes Vue's reactive() destructuring trap)
  const store = createStore({
    count: 1,
    multiplier: 10,
    title: 'Svet Hybrid TS + Rust WASM SIMD128 + Forma',
  });
  const { count, multiplier, title } = store;

  // 2. Glitch-Free Diamond Computed Graph in Rust WASM
  const doubled = computed(() => count() * 2);
  const scaled = computed(() => count() * multiplier());
  const totalEnergy = computed(() => doubled() + scaled());

  // 3. Deterministic Time-Travel Debugger (Undo / Redo / Scrub in O(1))
  const timeTravel = createTimeTravelDebugger(
    { count, multiplier, title },
    { maxSnapshots: 50, autoRecord: true }
  );

  // 4. Keyed List reconciled via WASM O(n log n) LIS
  const items = signal([
    { id: 101, name: 'WASM SIMD128 Arena DAG + FreeList' },
    { id: 102, name: 'Zero Virtual DOM (1x Component Execution)' },
    { id: 103, name: 'O(n log n) LIS Keyed Diff in Rust' },
    { id: 104, name: 'Forma .ui AOT + HMR + 3D Scene3D' },
    { id: 105, name: 'O(1) Deterministic Time-Travel Debugger' },
  ]);

  const telemetry = signal(getGraphTelemetry());
  const mutations = signal(0);

  // 5. GPU + UMA + DUMA Tensor Signals in Shared WASM Linear Arena + Sparkline
  configureGpuSubsystem({ architecture: 'UMA' });
  const gpuInput = createGpuTensor([
    1.5, 3.0, 2.2, 5.4, 4.1, 7.8, 6.2, 9.5, 4.8, 8.1, 11.2, 7.4, 12.6, 9.9, 14.0, 10.5,
  ]);
  const gpuOutput = createGpuTensor(16);
  effect(() => {
    executeGpuVectorFma(gpuInput, gpuOutput, 2.0, 1.0);
    syncGpuArena();
  });
  const gpuStats = signal(getGpuTelemetry());

  // 6. Forma .ui Component + Shared ViewModel running directly inside Svet WASM DAG
  const sharedCustomerName = createFormaProperty('Анна Смирнова');
  const lastSavedCustomer = signal('—');
  const formaCard = createCustomerCard({
    viewModel: {
      customerName: sharedCustomerName,
    },
  });
  formaCard.onEvent('customerSaved', (name: string, points: number) => {
    lastSavedCustomer.set(`${name} (${points} бонусов)`);
    refreshMetrics();
  });

  // 7. Interactive 3D GPU Cube in WASM UMA/DUMA Arena (with Face Shading + Wireframe)
  const cubeTransform = createTransform3D({
    position: [0, 0, -3.2],
    rotation: [0.45, 0.65, 0.1],
    scale: [0.95, 0.95, 0.95],
  });
  const camera3D = createPerspectiveCamera3D({
    fovYRad: Math.PI / 3.2,
    aspect: 320 / 180,
    near: 0.1,
    far: 100,
  });
  const cube3D = createCubeMesh3D({
    size: 1.5,
    transform: cubeTransform,
    camera: camera3D,
  });

  // 8. Live Forma .ui Visual Studio (Real-Time Compiler + AOT Validator + HMR Snapshot Preservation)
  const studioSource = signal(DEFAULT_PLAYGROUND_UI);
  const studioValidation = computed(() => validateFormaSource(studioSource()));
  const studioHostRef = document.createElement('div');
  const activeStudioInstance = compileFormaComponent(DEFAULT_PLAYGROUND_UI);
  activeStudioInstance.mount(studioHostRef);

  const recompileStudioWithHmr = (nextCode: string) => {
    studioSource.set(nextCode);
    const report = validateFormaSource(nextCode);
    if (report.valid) {
      activeStudioInstance.hotReload(nextCode);
      refreshMetrics();
    }
  };

  // 9. 100,000-Row Windowed VirtualList (O(1) Viewport Slicing)
  const virtualRowsData = Array.from({ length: 100_000 }, (_, i) => ({
    id: i + 1,
    metric: `WASM-Node #${i + 1}`,
    latencyUs: ((i * 17) % 95) + 5,
  }));
  const virtualRowsSig = signal(virtualRowsData);
  const virtualListEl = VirtualList({
    items: virtualRowsSig,
    itemHeight: 28,
    height: 196,
    overscan: 3,
    style: 'border:1px solid #23304d;border-radius:8px;background:#070b14;padding:0 8px;',
    renderItem: (row, idx) => {
      const div = document.createElement('div');
      div.style.display = 'flex';
      div.style.justifyContent = 'space-between';
      div.style.width = '100%';
      div.style.fontFamily = 'monospace';
      div.style.fontSize = '12px';
      div.style.color = idx % 2 === 0 ? '#e2e8f0' : '#94a3b8';
      const leftSpan = document.createElement('span');
      leftSpan.textContent = `[${row.id}] ${row.metric}`;
      const rightSpan = document.createElement('span');
      rightSpan.style.color = '#34d399';
      rightSpan.textContent = `${row.latencyUs} µs`;
      div.appendChild(leftSpan);
      div.appendChild(rightSpan);
      return div;
    },
  });

  const refreshMetrics = () => {
    telemetry.set(getGraphTelemetry());
    mutations.set(getDomMutationCount());
    gpuStats.set(getGpuTelemetry());
  };

  const shuffleList = () => {
    const copy = [...items.peek()];
    const last = copy.pop()!;
    copy.splice(1, 0, last);
    items.set(copy);
    refreshMetrics();
  };

  return (
    <div className="container">
      <header>
        <h1>Svet Engine — {title()}</h1>
        <p>
          Гибридный фреймворк <b>TypeScript + Rust WebAssembly (SIMD128) + GPU (UMA/DUMA) + Forma .ui</b>.
          Демонстрация реактивных обновлений, списков, Forma .ui и вычислительных примитивов.
        </p>
      </header>

      <div className="grid">
        <div className="card">
          <h2>1. Auto-Reactive JSX & Two-Way Model</h2>
          <div className="stat-row">
            <span>Two-Way Input (model={'{title}'}):</span>
            <input
              model={title}
              style="background:#0b0f19;color:#38bdf8;border:1px solid #23304d;padding:4px 8px;border-radius:4px;"
            />
          </div>
          <div className="stat-row">
            <span>Signal count():</span>
            <span className="stat-val">{count()}</span>
          </div>
          <div className="stat-row">
            <span>Signal multiplier():</span>
            <span className="stat-val">{multiplier()}</span>
          </div>
          <div className="stat-row">
            <span>Diamond Computed totalEnergy():</span>
            <span className="stat-val">{totalEnergy()}</span>
          </div>
          <div>
            <button
              onClick={() => {
                count.update((c) => c + 1);
                refreshMetrics();
              }}
            >
              +1 Count (Surgical DOM)
            </button>
            <button
              onClick={() => {
                batch(() => {
                  count.update((c) => c + 5);
                  multiplier.update((m) => m + 2);
                });
                refreshMetrics();
              }}
            >
              Atomic Batch (+5, +2)
            </button>
          </div>
        </div>

        <div className="card">
          <h2>2. WASM O(n log n) LIS Reconciler</h2>
          <p>Перестановка элементов вычисляет LIS в WASM и двигает ровно 1 DOM-узел:</p>
          <ul className="pill-list">
            <For each={items} keyFn={(item) => item.id}>
              {(item) => (
                <li>
                  #{item.id}: {item.name}
                </li>
              )}
            </For>
          </ul>
          <button onClick={shuffleList}>Rotate Last Item to Index 1 (1 DOM Move)</button>
        </div>

        <div className="card">
          <h2>3. Live WASM SIMD128 & GPU Telemetry</h2>
          <div className="stat-row">
            <span>Active WASM Nodes / Capacity:</span>
            <span className="stat-val">
              {telemetry().nodes} / {telemetry().capacity}
            </span>
          </div>
          <div className="stat-row">
            <span>WASM Graph Edges:</span>
            <span className="stat-val">{telemetry().edges}</span>
          </div>
          <div className="stat-row">
            <span>Surgical DOM Mutations:</span>
            <span className="stat-val">{mutations()}</span>
          </div>
          <div className="stat-row">
            <span>GPU Memory Architecture:</span>
            <span className="stat-val">
              {gpuStats().architecture} ({gpuStats().arenaByteLength} B Arena)
            </span>
          </div>
          <div style="margin: 0.6rem 0;">
            {GpuSparkline({ tensor: gpuOutput, width: 300, height: 64 })}
          </div>
          <button
            onClick={() => {
              const idx = Math.floor(Math.random() * 16);
              gpuInput.setAt(idx, gpuInput.peek()[idx] + 4.5);
              refreshMetrics();
            }}
          >
            Mutate GPU Tensor & Redraw Sparkline
          </button>
        </div>

        <div className="card">
          <h2>4. Интерфейс на Forma (.ui) внутри Svet</h2>
          <FormaHost instance={formaCard} />
          <div className="stat-row" style="margin-top:0.75rem;">
            <span>Сохранено из Forma в Svet:</span>
            <span className="stat-val">{lastSavedCustomer()}</span>
          </div>
          <div>
            <button
              onClick={() => {
                formaCard.locale.set(formaCard.locale.peek() === 'ru' ? 'en' : 'ru');
                refreshMetrics();
              }}
            >
              Переключить локаль Forma (RU / EN)
            </button>
          </div>
        </div>

        <div className="card">
          <h2>5. 3D GPU Scene Graph (SIMD128 Viewport)</h2>
          {Viewport3D({
            renderMode: 'animation-frame',
            mesh: cube3D.mesh,
            edges: cube3D.edges,
            faces: cube3D.faces,
            width: 310,
            height: 175,
          })}
          <div style="margin-top:0.5rem;">
            <button
              onClick={() => {
                const [rx, ry, rz] = cubeTransform.rotation.peek();
                cubeTransform.rotation.set([rx + 0.25, ry + 0.35, rz + 0.1]);
                refreshMetrics();
              }}
            >
              Повернуть 3D-куб в WASM SIMD128 Arena
            </button>
          </div>
        </div>

        <div className="card">
          <h2>6. O(1) Time-Travel Debugger (Svet DevTools)</h2>
          <div className="stat-row">
            <span>Snapshot Cursor / Total History:</span>
            <span className="stat-val">
              #{timeTravel.cursor()} / {timeTravel.history().length}
            </span>
          </div>
          <div className="stat-row">
            <span>Can Undo / Can Redo:</span>
            <span className="stat-val">
              {String(timeTravel.canUndo())} / {String(timeTravel.canRedo())}
            </span>
          </div>
          <div>
            <button
              onClick={() => {
                timeTravel.undo();
                refreshMetrics();
              }}
            >
              ⏪ Undo (t - 1)
            </button>
            <button
              onClick={() => {
                timeTravel.redo();
                refreshMetrics();
              }}
            >
              ⏩ Redo (t + 1)
            </button>
            <button
              onClick={() => {
                timeTravel.jumpTo(0);
                refreshMetrics();
              }}
            >
              ⏮ Jump to t=0
            </button>
          </div>
        </div>

        <div className="card">
          <h2>7. Live Forma .ui Studio (AOT + HMR + Scene3D)</h2>
          <div className="stat-row">
            <span>AOT Статус валидации:</span>
            <span className="stat-val">
              {studioValidation().valid
                ? `OK (${studioValidation().componentName}, 3D: ${studioValidation().has3DViewport})`
                : `Ошибок: ${studioValidation().diagnostics.length}`}
            </span>
          </div>
          <textarea
            value={studioSource()}
            onInput={(e: Event) =>
              recompileStudioWithHmr((e.target as HTMLTextAreaElement).value)
            }
            style="width:100%;height:110px;background:#070b14;color:#38bdf8;border:1px solid #23304d;border-radius:6px;font-family:monospace;font-size:11px;padding:6px;margin:6px 0;"
          />
          {studioHostRef}
        </div>

        <div className="card">
          <h2>8. 100,000-Row VirtualList (O(1) DOM Window)</h2>
          <p style="font-size:0.85rem;margin-top:0;">
            В памяти 100 000 строк, но в DOM рендерится только видимое окно (~13 узлов):
          </p>
          {virtualListEl}
          <div>
            <button onClick={() => virtualListEl.scrollToIndex(50_000)}>
              Jump to Row #50,000
            </button>
            <button onClick={() => virtualListEl.scrollToIndex(99_990)}>
              Jump to Row #99,990
            </button>
            <button onClick={() => virtualListEl.scrollToIndex(0)}>Jump to Top (#1)</button>
          </div>
        </div>
      </div>
    </div>
  );
}

const root = document.getElementById('app');
if (root) {
  const appMount = mount(() => <App />, root);
  if ((import.meta as any).hot) {
    (import.meta as any).hot.dispose(() => {
      appMount.unmount();
    });
    (import.meta as any).hot.accept();
  }
}
