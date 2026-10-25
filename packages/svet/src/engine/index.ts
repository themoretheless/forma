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
} from './reactivity';

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
} from './dom';

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
  type RouterInstance,
} from './advanced';

export {
  serializeState,
  resumeContainer,
  renderToString,
  serializeDomNodeToHtml,
  createServerAction,
  type ResumableSnapshot,
  type ServerAction,
} from './resumable';

export {
  computeLisWasm,
  getWasmCore,
  createWasmCoreInstance,
  withWasmCore,
} from './wasm-bridge';

export { transformSvetJsx, svetVitePlugin } from './compiler';

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
} from './gpu';

export {
  createFormaProperty,
  createFormaContext,
  formaStdlib,
  transformFormaPipes,
  evaluateFormaExpression,
  doesFormaPatternMatch,
  compileFormaComponent,
  registerFormaComponent,
  validateFormaSource,
  compileFormaToJsModule,
  createSpring,
  createPersistedSignal,
  createForm,
  FormaHost,
  type FormaProperty,
  type FormaContextHolder,
  type FormaPropConstraint,
  type FormaStateMutator,
  type FormaRuntimeInstance,
  type FormaSlotMap,
  type FormaDiagnostic,
  type FormaValidationReport,
  type FormaHmrSnapshot,
  type SpringSignal,
  type FormFieldRule,
  type ReactiveForm,
} from './forma';

export {
  createTimeTravelDebugger,
  VirtualList,
  type TimeTravelSnapshot,
  type TimeTravelDebugger,
  type VirtualListProps,
  type VirtualListHandle,
} from './devtools';
