/** Node-only SSR entry. Import from svet-engine/server, not the browser barrel. */
import { AsyncLocalStorage } from 'node:async_hooks';
import { getCurrentOwner, onCleanup, runWithAsyncRequestScope, type OwnerScope, type Signal } from './reactivity.ts';
import { serializeDomNodeToHtml, serializeState } from './resumable.ts';
import { captureViewLifetime } from './dom.ts';
import { createWasmCoreInstance, installAsyncCoreResolver, withWasmCore, type SvetWasmExports } from './wasm-bridge.ts';

type RequestContext = { core: SvetWasmExports; closed: boolean };
const requests = new AsyncLocalStorage<RequestContext>();
installAsyncCoreResolver(() => {
  const request = requests.getStore();
  if (request?.closed) throw new Error('Svet SSR request has already completed');
  return request?.core;
}, callback => requests.exit(callback));

export interface SsrRequestOptions {
  /** Abort on disconnect or a host-controlled deadline. Awaited user work must honor it too. */
  signal?: AbortSignal;
}

async function awaitRequest<T>(fn: () => T | PromiseLike<T>, signal?: AbortSignal): Promise<T> {
  if (!signal) return await fn();
  signal.throwIfAborted();
  let abort!: () => void;
  const cancelled = new Promise<never>((_, reject) => {
    abort = () => reject(signal.reason);
    signal.addEventListener('abort', abort, { once: true });
  });
  try {
    // Both outcomes are observed by the race even if user work settles after abort.
    return await Promise.race([Promise.resolve().then(() => {
      signal.throwIfAborted();
      return fn();
    }), cancelled]);
  } finally {
    signal.removeEventListener('abort', abort);
  }
}

/**
 * Runs one request in its own WASM graph and ownership root through await boundaries.
 * Effects, cleanups and graph registries are released on success or rejection.
 * Return plain results; reactive resources belong to the request and cannot be
 * used after completion. Await all work that needs the request context.
 */
export function runWithIsolatedContextAsync<T>(
  fn: () => T | PromiseLike<T>,
  options: SsrRequestOptions = {}
): Promise<T> {
  const request: RequestContext = { core: createWasmCoreInstance(), closed: false };
  return requests.run(request, async () => {
    try {
      return await withWasmCore(request.core, () => runWithAsyncRequestScope(() => awaitRequest(fn, options.signal)));
    } finally {
      request.closed = true;
    }
  });
}

export interface AsyncSsrView {
  node: Node;
  signals?: Record<string, Signal<any>>;
}

/** Requires a DOM implementation supplied by the server host. Does not install globals. */
export function renderToStringAsync(
  view: () => AsyncSsrView | PromiseLike<AsyncSsrView>,
  options: SsrRequestOptions = {}
): Promise<{ html: string; stateJson: string }> {
  let assertCallerCurrent: () => void, callerOwner: OwnerScope | null, userSignal: AbortSignal | undefined;
  try {
    assertCallerCurrent = captureViewLifetime(); assertCallerCurrent();
    callerOwner = getCurrentOwner(); userSignal = options.signal;
    assertCallerCurrent();
  } catch (failure) { return Promise.reject(failure); }
  let requestSignal = userSignal, releaseCaller = () => {};
  try {
    if (callerOwner) {
      const owner = callerOwner, controller = new AbortController();
      const stop = () => controller.abort(new Error('SSR calling owner disposed'));
      const forward = () => controller.abort(userSignal!.reason);
      releaseCaller = () => {
        try { userSignal?.removeEventListener('abort', forward); } catch { /* Complete caller unlinking. */ }
        if (!owner.disposed) {
          const index = owner.disposables.indexOf(stop);
          if (index >= 0) owner.disposables.splice(index, 1);
        }
      };
      onCleanup(stop);
      if (userSignal?.aborted) forward();
      else userSignal?.addEventListener('abort', forward, {once: true});
      requestSignal = controller.signal;
    }
    assertCallerCurrent();
  } catch (failure) { releaseCaller(); return Promise.reject(failure); }
  let request: Promise<{ html: string; stateJson: string }>;
  try {
    request = runWithIsolatedContextAsync(async () => {
      const assertRequestCurrent = captureViewLifetime();
      const assertCurrent = () => { assertCallerCurrent(); assertRequestCurrent(); };
      assertCurrent();
      const result = await view();
      assertCurrent();
      const node = result.node;
      assertCurrent();
      let html = serializeDomNodeToHtml(node);
      assertCurrent();
      const signals = result.signals;
      assertCurrent();
      const stateJson = signals ? serializeState(signals) : '{}';
      assertCurrent();
      if (signals) html += `<script type="application/svet+json">${stateJson}</script>`;
      return { html, stateJson };
    }, {signal: requestSignal});
  } catch (failure) { releaseCaller(); return Promise.reject(failure); }
  return request.then(result => {
    // Request teardown can itself dispose the original calling owner.
    assertCallerCurrent();
    return result;
  }).finally(releaseCaller);
}
