/**
 * # Svet Application Primitives (`advanced.ts`)
 *
 * Completes the Svet ecosystem with zero external dependencies:
 * 1. **Dependency Injection (`createContext` / `useContext`)**: Hierarchical context lookup
 *    along the `OwnerScope` tree — zero VDOM provider wrapper overhead.
 * 2. **Portals (`<Portal mount={target}>`)**: Renders a reactive DOM subtree into any external
 *    DOM container (modals, tooltips, overlays) while keeping full `OwnerScope` lifecycle cleanup
 *    and context inheritance from its logical parent!
 * 3. **Async Resources & Boundaries (`createResource`, `<Suspense>`, `<ErrorBoundary>`)**:
 *    First-class reactive async data fetching with automatic `<Suspense>` loading orchestration
 *    and `<ErrorBoundary>` fault isolation with recovery (`reset`).
 * 4. **Reactive Router (`createRouter`, `<RouterView>`, `<Link>`)**: Signal-backed client/SSR
 *    router with dynamic parameters (`/users/:id`), programmatic navigation, and automatic
 *    `OwnerScope` disposal when switching routes.
 */

export { type SvetContext, createContext, useContext, Portal } from './advanced-context.ts';
export {
  type ResourceFetcherContext,
  type Resource,
  createResource,
  Suspense,
  ErrorBoundary,
} from './advanced-resource.ts';
export {
  type RouteDefinition,
  type NavigationGuard,
  type RouterInstance,
  createRouter,
  useRouter,
  RouterView,
  Link,
} from './advanced-router.ts';
