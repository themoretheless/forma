/**
 * What can be registered with the Forma runtime from outside a component: renderers for tags the
 * core does not draw, and the sources of components other components embed by name.
 */
import type { FormaEvalScope } from './forma-expr.ts';

/** Sources of the components registered with `registerFormaComponent`, by name. */
export const formaComponentRegistry = new Map<string, string>();

/** What an extension element renderer receives for one `Tag { ... }` occurrence. */
export interface FormaElementContext {
  tag: string;
  /** Raw property expressions of the element, after `apply` styles and `forward props` are merged in. */
  props: Record<string, string>;
  /** Evaluation scope for `evaluateFormaExpression` / `evalAstNode`; reads inside effects are tracked. */
  scope: FormaEvalScope;
  /** Makes the element reachable through `instance.getElementByKey(key)`. */
  registerKey(key: string, element: HTMLElement): void;
}
export type FormaElementRenderer = (context: FormaElementContext) => Node;

export const FORMA_3D_TAGS = new Set(['Scene3D', 'OrbitCube3D', 'Viewport3D']);
// Shared across copies of the package, like the other cross-copy registries in this file.
const formaElementHost = globalThis as unknown as Record<
  symbol,
  Map<string, FormaElementRenderer> | undefined
>;
export const formaElementRenderers: Map<string, FormaElementRenderer> = (formaElementHost[
  Symbol.for('svet.forma.elements.v1')
] ??= new Map<string, FormaElementRenderer>());

/**
 * Registers a renderer for one or more Forma tags that the core runtime does not draw itself.
 * It runs inside the component's reactive owner, so `effect` and `onCleanup` behave as in built-in elements.
 * Returns a function that removes the registration.
 */
export function registerFormaElement(
  tags: string | readonly string[],
  renderer: FormaElementRenderer,
): () => void {
  const names = typeof tags === 'string' ? [tags] : [...tags];
  for (const name of names) {
    if (!/^[A-Z][A-Za-z0-9_]*$/.test(name))
      throw new TypeError(`Invalid Forma element name: ${name}`);
    formaElementRenderers.set(name, renderer);
  }
  return () => {
    for (const name of names) {
      if (formaElementRenderers.get(name) === renderer) formaElementRenderers.delete(name);
    }
  };
}
