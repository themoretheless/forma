/** Host-neutral SSR implementation. Native-await context propagation is required. */

import { captureViewLifetime, installAsyncComponentRenderer } from './dom.ts';
import {
  getCurrentOwner,
  getReactiveGeneration,
  installAsyncOwnerResolver,
  isOwnerScopeDisposed,
  onCleanup,
  type OwnerScope,
  retireReactiveContextCache,
  runWithAsyncRequestScope,
  runWithOwnerScope,
  untrack,
} from './reactivity.ts';
import { serializeDomNodeToHtml, serializeState } from './resumable.ts';
import {
  type AsyncRenderSession,
  type AsyncSsrView,
  awaitRequest,
  chunkSsrStream,
  deadlineSignal,
  readableSsrStreamFromChunks,
  retireRenderSession,
  settleComponents,
  type SsrAsyncContextCarrier,
  type SsrReadableStreamOptions,
  type SsrRequestOptions,
  type SsrRuntimeHost,
  type SsrStreamOptions,
  SsrTimeoutError,
} from './ssr-stream-channel.ts';
import type { SsrStreamChunk } from './stream-protocol.ts';
import {
  createWasmCoreInstance,
  getWasmCore,
  installAsyncCoreResolver,
  type SvetWasmExports,
  withWasmCore,
} from './wasm-bridge.ts';

export {
  type SsrRequestOptions,
  type AsyncSsrView,
  type SsrStreamOptions,
  type SsrReadableStreamOptions,
  type SsrAsyncContextCarrier,
  type SsrRuntimeHost,
  SsrTimeoutError,
};

type RequestContext = { core?: SvetWasmExports | undefined; closed: boolean };
type AsyncOwnerContext = {
  request: RequestContext;
  core?: SvetWasmExports | undefined;
  owner?: OwnerScope | undefined;
  generation: number;
  closed: boolean;
};

function assertOwnerCurrent(context: AsyncOwnerContext): void {
  if (context.closed || !context.core || !context.owner)
    throw new Error('Svet SSR owner scope has already completed');
  if (context.generation !== getReactiveGeneration(context.core))
    throw new Error('Svet SSR owner graph was reset');
  if (isOwnerScopeDisposed(context.owner)) throw new Error('Svet SSR owner has been disposed');
}

function retireOwnerContext(context: AsyncOwnerContext): void {
  context.closed = true;
  context.core = undefined;
  context.owner = undefined;
}

function installSsrComponentHook(
  renders: SsrAsyncContextCarrier<AsyncRenderSession>,
  runWithOwnerScopeAsync: <T>(
    contextEntries: Array<[symbol, unknown]>,
    fn: () => T | PromiseLike<T>,
    options?: SsrRequestOptions,
  ) => Promise<T>,
): void {
  installAsyncComponentRenderer((component, props) => {
    const session = renders.getStore();
    if (!session) return undefined;
    if (session.closed) throw new Error('Svet SSR render has already completed');
    const assertCurrent = captureViewLifetime();
    const boundary = session.stream ? ++session.stream.sequence : 0;
    const marker = session.stream ? `svet:${session.stream.id}:${boundary}` : 'svet-async';
    const fragment = document.createDocumentFragment(),
      start = document.createComment(marker),
      end = document.createComment(session.stream ? marker + ':end' : '/svet-async');
    fragment.append(start, end);
    let immediate: Node | undefined;
    const task = runWithOwnerScopeAsync<{ node: Node }>([], () =>
      untrack(() => {
        const result = component(props);
        assertCurrent();
        if (result && typeof (result as Node).nodeType === 'number') {
          immediate = result as Node;
          // Wrap Nodes to avoid accidental Promise assimilation of custom then fields.
          return { node: immediate };
        }
        return Promise.resolve(result).then(node => ({ node }));
      }),
    )
      .then(async ({ node }) => {
        assertCurrent();
        if (session.closed) throw new Error('Svet SSR render has already completed');
        if (immediate) return;
        const commit = () => {
          assertCurrent();
          if (session.closed) throw new Error('Svet SSR render has already completed');
          const parent = start.parentNode;
          if (!parent || end.parentNode !== parent)
            throw new Error('Async SSR component insertion range was removed');
          parent.insertBefore(node, end);
          if (!session.stream) {
            start.remove();
            end.remove();
          }
        };
        if (session.stream) await session.stream.publish(boundary, node, commit);
        else commit();
      })
      .catch(failure => {
        if (!session.failed && !session.closed) {
          session.failed = true;
          session.failure = failure;
          session.notifyFailure();
          session.stream?.fail(failure);
        }
      });
    if (session.stream && !immediate) {
      if (session.stream.records.size >= session.stream.limit) {
        const error = RangeError('SSR stream exceeds maxBoundaries');
        session.stream.fail(error);
        throw error;
      }
      session.stream.records.set(boundary, { start, end });
    }
    session.pending.add(task);
    void task.then(() => session.pending.delete(task));
    return immediate ?? fragment;
  });
}

function renderSsrToStringAsync(
  renders: SsrAsyncContextCarrier<AsyncRenderSession>,
  runWithIsolatedContextAsync: <T>(
    fn: () => T | PromiseLike<T>,
    options?: SsrRequestOptions,
  ) => Promise<T>,
  view: () => AsyncSsrView | PromiseLike<AsyncSsrView>,
  options: SsrRequestOptions = {},
): Promise<{ html: string; stateJson: string }> {
  let assertCallerCurrent: () => void,
    callerOwner: OwnerScope | null,
    userSignal: AbortSignal | undefined;
  try {
    assertCallerCurrent = captureViewLifetime();
    assertCallerCurrent();
    callerOwner = getCurrentOwner();
    userSignal = options.signal;
    assertCallerCurrent();
  } catch (failure) {
    return Promise.reject(failure);
  }
  let timeoutMs: number | undefined;
  try {
    timeoutMs = options.timeoutMs;
    assertCallerCurrent();
  } catch (error) {
    return Promise.reject(error);
  }
  let requestSignal = userSignal,
    releaseCaller = () => {};
  try {
    if (callerOwner) {
      const owner = callerOwner,
        controller = new AbortController();
      const stop = () => controller.abort(new Error('SSR calling owner disposed'));
      const forward = () => controller.abort(userSignal!.reason);
      releaseCaller = () => {
        try {
          userSignal?.removeEventListener('abort', forward);
        } catch {
          /* Complete caller unlinking. */
        }
        if (!owner.disposed) {
          const index = owner.disposables.indexOf(stop);
          if (index >= 0) owner.disposables.splice(index, 1);
        }
      };
      onCleanup(stop);
      if (userSignal?.aborted) forward();
      else userSignal?.addEventListener('abort', forward, { once: true });
      requestSignal = controller.signal;
    }
    assertCallerCurrent();
  } catch (failure) {
    releaseCaller();
    return Promise.reject(failure);
  }
  let request: Promise<{ html: string; stateJson: string }>;
  try {
    request = runWithIsolatedContextAsync(
      async () => {
        const assertRequestCurrent = captureViewLifetime();
        const assertCurrent = () => {
          assertCallerCurrent();
          assertRequestCurrent();
        };
        assertCurrent();
        let notifyFailure!: () => void;
        const failureReady = new Promise<void>(resolve => {
          notifyFailure = resolve;
        });
        const session: AsyncRenderSession = {
          pending: new Set(),
          failed: false,
          failureReady,
          notifyFailure,
          closed: false,
        };
        onCleanup(() => retireRenderSession(session));
        return renders.run(session, async () => {
          try {
            const result = await view();
            await settleComponents(session);
            assertCurrent();
            const node = result.node;
            assertCurrent();
            await settleComponents(session);
            const signals = result.signals;
            assertCurrent();
            await settleComponents(session);
            assertCurrent();
            session.closed = true;
            let html = serializeDomNodeToHtml(node);
            assertCurrent();
            const stateJson = signals ? serializeState(signals) : '{}';
            assertCurrent();
            if (signals) html += `<script type="application/svet+json">${stateJson}</script>`;
            return { html, stateJson };
          } finally {
            retireRenderSession(session);
          }
        });
      },
      { signal: requestSignal, timeoutMs },
    );
  } catch (failure) {
    releaseCaller();
    return Promise.reject(failure);
  }
  return request
    .then(result => {
      // Request teardown can itself dispose the original calling owner.
      assertCallerCurrent();
      return result;
    })
    .finally(releaseCaller);
}

let initialized = false;

/** One runtime per engine module instance; host supplies DOM and async context. */
export function createSsrRuntime(host: SsrRuntimeHost) {
  if (initialized) throw Error('Svet SSR runtime is already installed for this engine instance');
  if (
    !host ||
    typeof host.createAsyncContext !== 'function' ||
    typeof host.randomUUID !== 'function'
  )
    throw TypeError('Invalid Svet SSR runtime host');
  const requests = host.createAsyncContext<RequestContext>(),
    owners = host.createAsyncContext<AsyncOwnerContext>(),
    renders = host.createAsyncContext<AsyncRenderSession>();
  for (const carrier of [requests, owners, renders])
    if (
      !carrier ||
      typeof carrier.getStore !== 'function' ||
      typeof carrier.run !== 'function' ||
      typeof carrier.exit !== 'function'
    )
      throw TypeError('Invalid Svet SSR async context carrier');
  if (new Set([requests, owners, renders]).size !== 3)
    throw Error('Svet SSR context carriers must be independent');
  initialized = true;
  const randomUUID = () => host.randomUUID();
  const requestByCore = new WeakMap<SvetWasmExports, RequestContext>();

  installAsyncOwnerResolver(core => {
    const context = owners.getStore();
    if (!context || requestByCore.get(core) !== context.request) return undefined;
    assertOwnerCurrent(context);
    return context.owner;
  });
  installAsyncCoreResolver(
    () => {
      const request = requests.getStore();
      if (request?.closed) throw new Error('Svet SSR request has already completed');
      const owner = owners.getStore();
      if (owner && owner.request === request) assertOwnerCurrent(owner);
      return request?.core;
    },
    callback => requests.exit(() => owners.exit(callback)),
  );

  /**
   * Runs one request in its own WASM graph and ownership root through await boundaries.
   * Effects, cleanups and graph registries are released on success or rejection.
   * Return plain results; reactive resources belong to the request and cannot be
   * used after completion. Await all work that needs the request context.
   */
  function runWithIsolatedContextAsync<T>(
    fn: () => T | PromiseLike<T>,
    options: SsrRequestOptions = {},
  ): Promise<T> {
    const deadline = deadlineSignal(options.signal, options.timeoutMs);
    let core: SvetWasmExports;
    try {
      core = createWasmCoreInstance();
    } catch (error) {
      deadline.dispose();
      throw error;
    }
    const request: RequestContext = { core, closed: false };
    requestByCore.set(core, request);
    return requests.run(request, async () => {
      try {
        return await withWasmCore(core, () =>
          runWithAsyncRequestScope(() => awaitRequest(fn, deadline.signal)),
        );
      } finally {
        deadline.dispose();
        request.closed = true;
        // Host async-context frames can outlive the request. Keep only a closed
        // lifetime token there, never the retired WASM graph.
        request.core = undefined;
        retireReactiveContextCache(core);
      }
    });
  }

  /**
   * Host-carried async child ownership within an active isolated request. Context values
   * and resources created after await belong to this child, independently of siblings.
   * Successful children live until request teardown; failed/cancelled children stop
   * immediately. Await this work. Detached continuations cannot reopen a settled scope.
   */
  function runWithOwnerScopeAsync<T>(
    contextEntries: Array<[symbol, unknown]>,
    fn: () => T | PromiseLike<T>,
    options: SsrRequestOptions = {},
  ): Promise<T> {
    let releaseDeadline = () => {};
    try {
      const request = requests.getStore(),
        core = getWasmCore();
      if (!request || request.closed || request.core !== core) {
        throw new Error('runWithOwnerScopeAsync requires an active isolated SSR request');
      }
      const assertCallerCurrent = captureViewLifetime();
      assertCallerCurrent();
      const inputSignal = options.signal;
      assertCallerCurrent();
      const timeoutMs = options.timeoutMs;
      assertCallerCurrent();
      const deadline = deadlineSignal(inputSignal, timeoutMs);
      releaseDeadline = deadline.dispose;
      const userSignal = deadline.signal;
      userSignal?.throwIfAborted();
      return runWithOwnerScope(contextEntries, dispose => {
        assertCallerCurrent();
        const owner = getCurrentOwner()!;
        const context: AsyncOwnerContext = {
          request,
          core,
          owner,
          generation: getReactiveGeneration(core),
          closed: false,
        };
        const controller = new AbortController();
        const stop = () => controller.abort(new Error('Svet SSR owner has been disposed'));
        const forward = () => controller.abort(userSignal!.reason);
        onCleanup(stop);
        const release = () => {
          releaseDeadline();
          try {
            userSignal?.removeEventListener('abort', forward);
          } catch {
            /* Still unlink the owner hook. */
          }
          const index = owner.disposables.indexOf(stop);
          if (index >= 0) owner.disposables.splice(index, 1);
        };
        try {
          userSignal?.addEventListener('abort', forward, { once: true });
          // Also cover cancellation that happened while a host hook was installed.
          if (userSignal?.aborted) forward();
          return owners.run(context, async () => {
            try {
              const result = await awaitRequest(() => {
                assertCallerCurrent();
                assertOwnerCurrent(context);
                return fn();
              }, controller.signal);
              assertOwnerCurrent(context);
              return result;
            } catch (failure) {
              dispose();
              throw failure;
            } finally {
              retireOwnerContext(context);
              release();
            }
          });
        } catch (failure) {
          try {
            release();
            dispose();
          } finally {
            retireOwnerContext(context);
          }
          throw failure;
        }
      });
    } catch (failure) {
      releaseDeadline();
      return Promise.reject(failure);
    }
  }

  installSsrComponentHook(renders, runWithOwnerScopeAsync);
  const streamDeps = { randomUUID, renders, runWithIsolatedContextAsync };

  /** Requires a DOM implementation supplied by the server host. Does not install globals. */
  const renderToStringAsync = (
    view: () => AsyncSsrView | PromiseLike<AsyncSsrView>,
    options: SsrRequestOptions = {},
  ) => renderSsrToStringAsync(renders, runWithIsolatedContextAsync, view, options);

  /**
   * Progressive Node SSR data: shell, ready boundary patches, then authoritative
   * final HTML/state. Consume with createSsrStreamReceiver. This is not raw HTML.
   * One chunk is queued; return() or the host AbortSignal closes request ownership.
   */
  const renderToChunksAsync = (
    view: () => AsyncSsrView | PromiseLike<AsyncSsrView>,
    options: SsrStreamOptions = {},
  ): AsyncIterableIterator<SsrStreamChunk> => chunkSsrStream(streamDeps, view, options);

  /** SSR exposed as pull-driven UTF-8 NDJSON; no eager reads or gzip policy. */
  const renderToReadableStreamAsync = (
    view: () => AsyncSsrView | PromiseLike<AsyncSsrView>,
    options: SsrReadableStreamOptions = {},
  ): ReadableStream<Uint8Array> => {
    const assertCaller = captureViewLifetime();
    assertCaller();
    const codec = options.codec ?? 'identity';
    assertCaller();
    if (codec !== 'identity' && codec !== 'svet-html-ref-v1')
      throw Error('Unsupported SSR readable codec');
    const signal = options.signal;
    assertCaller();
    const timeoutMs = options.timeoutMs;
    assertCaller();
    const maxChunkBytes = options.maxChunkBytes;
    assertCaller();
    const maxBoundaries = options.maxBoundaries;
    assertCaller();
    return readableSsrStreamFromChunks(
      notify =>
        chunkSsrStream(
          streamDeps,
          view,
          { signal, timeoutMs, maxChunkBytes, maxBoundaries },
          notify,
        ),
      signal,
      maxChunkBytes,
      codec,
      getCurrentOwner(),
    );
  };

  return {
    runWithIsolatedContextAsync,
    runWithOwnerScopeAsync,
    renderToStringAsync,
    renderToChunksAsync,
    renderToReadableStreamAsync,
  };
}
export type SsrRuntime = ReturnType<typeof createSsrRuntime>;

/** Checks a host carrier before installing hooks; a synchronous stack is insufficient. */
export async function assertSsrAsyncContextCarrier(
  create: () => SsrAsyncContextCarrier<object>,
): Promise<void> {
  const carrier = create(),
    outside = carrier.getStore(),
    tokens = [{}, {}],
    failure = Error('carrier restoration probe');
  let entered = 0;
  await Promise.all(
    tokens.map(token =>
      carrier.run(token, async () => {
        entered++;
        if (carrier.getStore() !== token) throw Error('SSR carrier lost synchronous context');
        await Promise.resolve();
        if (carrier.getStore() !== token) throw Error('SSR carrier lost native await context');
        await carrier.exit(async () => {
          if (carrier.getStore() !== undefined)
            throw Error('SSR carrier exit did not clear context');
          await Promise.resolve();
          if (carrier.getStore() !== undefined)
            throw Error('SSR carrier exit lost native await isolation');
        });
        if (carrier.getStore() !== token) throw Error('SSR carrier did not restore exited context');
        const nested = {};
        try {
          await carrier.run(nested, async () => {
            await Promise.resolve();
            if (carrier.getStore() !== nested) throw Error('SSR carrier lost nested context');
            throw failure;
          });
        } catch (error) {
          if (error !== failure) throw error;
        }
        if (carrier.getStore() !== token)
          throw Error('SSR carrier did not restore rejected context');
      }),
    ),
  );
  if (entered !== tokens.length || carrier.getStore() !== outside)
    throw Error('SSR carrier did not restore caller context');
}
