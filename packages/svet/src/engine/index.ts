/**
 * # Svet Engine — Public API (`src/engine/index.ts`)
 *
 * Complete Hybrid TypeScript + Rust WebAssembly UI Framework.
 */

export {
  signal,
  computed,
  effect,
  batch,
  untrack,
  createRoot,
  onCleanup,
  getCurrentOwner,
  runWithOwnerScope,
  lookupOwnerContext,
  runWithIsolatedContext,
  createStore,
  createDeepStore,
  flushEffects,
  resetReactiveSystem,
  getGraphTelemetry,
  type Signal,
  type ReadonlySignal,
  type EffectHandle,
  type OwnerScope,
  type SignalStore,
  type DeepSignalStore,
} from './reactivity.ts';

export {
  h,
  Fragment,
  Show,
  For,
  mount,
  bindProp,
  insertChild,
  getDomMutationCount,
  resetDomMutationCount,
  type Component,
  type ChildNodeValue,
  type ReactiveValue,
} from './dom.ts';

export {
  createContext,
  useContext,
  Portal,
  createResource,
  Suspense,
  ErrorBoundary,
  createRouter,
  useRouter,
  RouterView,
  Link,
  type SvetContext,
  type Resource,
  type RouteDefinition,
  type NavigationGuard,
  type RouterInstance,
} from './advanced.ts';

export {
  serializeState,
  resumeContainer,
  renderToString,
  serializeDomNodeToHtml,
  createServerAction,
  type ResumableSnapshot,
  type ServerAction,
} from './resumable.ts';

export {
  computeLisWasm,
  getWasmCore,
  createWasmCoreInstance,
  withWasmCore,
} from './wasm-bridge.ts';

export { transformSvetJsx, svetVitePlugin } from './compiler.ts';

export {
  configureGpuSubsystem,
  initWebGpuHardware,
  resetGpuTelemetry,
  createGpuTensor,
  syncGpuArena,
  executeGpuVectorFma,
  dispatchGpuComputeKernel,
  getGpuTelemetry,
  createTransform3D,
  createPerspectiveCamera3D,
  createMesh3D,
  createCubeMesh3D,
  pickNearestVertex3D,
  Canvas3D,
  GpuSparkline,
  type MemoryArchitecture,
  type GpuDeviceTelemetry,
  type GpuTensorSignal,
  type WebGpuDeviceLike,
  type WebGpuQueueLike,
  type GpuSubsystemConfig,
  type WebGpuComputeDispatchOptions,
  type Vec3Tuple,
  type ReactiveTransform3D,
  type PerspectiveCamera3D,
  type Mesh3D,
} from './gpu.ts';

export {
  createFormaProperty,
  createFormaContext,
  formaStdlib,
  transformFormaPipes,
  evaluateFormaExpression,
  doesFormaPatternMatch,
  compileFormaComponent,
  compileFormaAst,
  parseFormaSource,
  registerFormaComponent,
  validateFormaSource,
  compileFormaToJsModule,
  createSpring,
  flushSpringFrame,
  createPersistedSignal,
  createForm,
  createContractForm,
  FormaHost,
  type FormaProperty,
  type FormaContextHolder,
  type FormaPropConstraint,
  type FormaDocumentAst,
  type FormaStateMutator,
  type FormaRuntimeInstance,
  type FormaSlotMap,
  type FormaDiagnostic,
  type FormaValidationReport,
  type FormaHmrSnapshot,
  type SpringSignal,
  type FormFieldRule,
  type ReactiveForm,
} from './forma.ts';

export {
  createTimeTravelDebugger,
  VirtualList,
  type TimeTravelSnapshot,
  type TimeTravelDebugger,
  type VirtualListProps,
  type VirtualListHandle,
} from './devtools.ts';
