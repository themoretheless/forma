import { installAsyncCoreResolver } from '../../src/engine/wasm-bridge';

type Resolver = Parameters<typeof installAsyncCoreResolver>[0];
let restore: (() => void) | undefined;

/**
 * Replaces the async core resolver for one test. The server entry installs the real resolver once
 * per module graph, and test files of the `unit` project share that graph, so a stub that is not
 * put back breaks every later file that awaits inside an isolated context.
 */
export function stubAsyncCoreResolver(resolver: Resolver): void {
  restoreAsyncCoreResolver();
  restore = installAsyncCoreResolver(resolver);
}

export function restoreAsyncCoreResolver(): void {
  restore?.();
  restore = undefined;
}
