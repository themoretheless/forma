/** Host-neutral SSR implementation. Native-await context propagation is required. */
import { createSsrStreamTransportEncoder } from './stream-codec.ts';
import {
  SSR_STREAM_VERSION,
  type SsrStreamChunk,
  type SsrStreamPayload,
} from './stream-protocol.ts';
import {
  getCurrentOwner,
  getReactiveGeneration,
  installAsyncOwnerResolver,
  isOwnerScopeDisposed,
  onCleanup,
  retireReactiveContextCache,
  runWithAsyncRequestScope,
  runWithOwnerScope,
  untrack,
  type OwnerScope,
  type Signal,
} from './reactivity.ts';
import { serializeDomNodeToHtml, serializeState } from './resumable.ts';
import { captureViewLifetime, installAsyncComponentRenderer } from './dom.ts';
import {
  createWasmCoreInstance,
  getWasmCore,
  installAsyncCoreResolver,
  withWasmCore,
  type SvetWasmExports,
} from './wasm-bridge.ts';

export interface SsrRequestOptions {
  /** Abort on disconnect or a host-controlled deadline. Awaited user work must honor it too. */
  signal?: AbortSignal;
  /** Optional timeout timer in integer milliseconds (1..2147483647); cannot preempt synchronous work. */
  timeoutMs?: number;
}

export interface AsyncSsrView {
  node: Node;
  signals?: Record<string, Signal<any>>;
}

export interface SsrStreamOptions extends SsrRequestOptions {
  /** Encoded payload limit for each shell, patch or final snapshot (default 8 MiB). */
  maxChunkBytes?: number;
  /** Maximum async boundaries created by one stream (default 1024). */
  maxBoundaries?: number;
}

export interface SsrReadableStreamOptions extends SsrStreamOptions {
  /** Transport framing must be explicitly agreed with the client. Default identity. */
  codec?: import('./stream-protocol.ts').SsrStreamCodec;
}

export interface SsrAsyncContextCarrier<T> {
  getStore(): T | undefined;
  run<R>(store: T, callback: () => R): R;
  exit<R>(callback: () => R): R;
}
/** Each create call must return a distinct carrier preserving native await context. */
export interface SsrRuntimeHost {
  createAsyncContext<T>(): SsrAsyncContextCarrier<T>;
  /** Unique canonical UUID for every progressive request. */
  randomUUID(): string;
}
export class SsrTimeoutError extends Error {
  override readonly name = 'SsrTimeoutError';
  constructor(readonly timeoutMs: number) {
    super(`SSR request timed out after ${timeoutMs}ms`);
    // Freeze V8 lazy call sites to text; structured frames can retain ALS stores.
    try {
      this.stack = String(this.stack);
    } catch {
      this.stack = `${this.name}: ${this.message}`;
    }
  }
}
function deadlineSignal(signal: AbortSignal | undefined, timeoutMs: number | undefined) {
  if (timeoutMs === undefined) return { signal, dispose: () => {} };
  if (!Number.isSafeInteger(timeoutMs) || timeoutMs < 1 || timeoutMs > 2147483647)
    throw RangeError('Invalid SSR timeoutMs');
  const controller = new AbortController();
  let timer: ReturnType<typeof setTimeout> | undefined;
  const dispose = () => {
    clearTimeout(timer);
    try {
      signal?.removeEventListener('abort', forward);
    } catch {
      /* Always release timer. */
    }
  };
  const forward = () => {
    controller.abort(signal!.reason);
    dispose();
  };
  try {
    signal?.addEventListener('abort', forward, { once: true });
    if (signal?.aborted) forward();
    else
      timer = setTimeout(() => {
        controller.abort(new SsrTimeoutError(timeoutMs));
        dispose();
      }, timeoutMs);
  } catch (error) {
    dispose();
    throw error;
  }
  return { signal: controller.signal, dispose };
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
  type RequestContext = { core?: SvetWasmExports; closed: boolean };
  const requestByCore = new WeakMap<SvetWasmExports, RequestContext>();
  type AsyncOwnerContext = {
    request: RequestContext;
    core?: SvetWasmExports;
    owner?: OwnerScope;
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

  type StreamPublication = {
    limit: number;
    id: string;
    sequence: number;
    records: Map<number, { start: Comment; end: Comment }>;
    publish: (id: number, node: Node, commit: () => void) => Promise<void>;
    fail: (failure: unknown) => void;
  };
  type AsyncRenderSession = {
    stream?: StreamPublication;
    pending: Set<Promise<void>>;
    failed: boolean;
    failure?: unknown;
    failureReady: Promise<void>;
    notifyFailure: () => void;
    closed: boolean;
  };
  function retireRenderSession(session: AsyncRenderSession): void {
    session.closed = true;
    session.pending.clear();
    session.stream = undefined;
    session.failure = undefined;
    session.notifyFailure();
    session.notifyFailure = () => {};
  }

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
  async function settleComponents(session: AsyncRenderSession): Promise<void> {
    while (session.pending.size) {
      await Promise.race([Promise.all(Array.from(session.pending)), session.failureReady]);
      if (session.failed) throw session.failure;
    }
    if (session.failed) throw session.failure;
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
      // Start in the installed owner context, preserving synchronous component DOM.
      // A host callback can abort and throw before Promise.race observes cancellation.
      void cancelled.catch(() => {});
      signal.throwIfAborted();
      return await Promise.race([fn(), cancelled]);
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
    contextEntries: Array<[symbol, any]>,
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

  /** Requires a DOM implementation supplied by the server host. Does not install globals. */
  function renderToStringAsync(
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

  /** One-slot rendezvous queue. Return/cancellation also wakes an outstanding next(). */
  class SsrChunkChannel {
    private slot?: { chunk: SsrStreamChunk; consumed: () => void };
    private changed: Promise<void>;
    private wake!: () => void;
    private ended = false;
    private failure: unknown;
    private hasFailure = false;
    constructor() {
      this.changed = this.event();
    }
    private event() {
      return new Promise<void>(resolve => {
        this.wake = resolve;
      });
    }
    private notify() {
      const wake = this.wake;
      this.changed = this.event();
      wake();
    }
    async ready(): Promise<void> {
      while (this.slot && !this.ended) await this.changed;
      if (this.ended) throw this.hasFailure ? this.failure : Error('SSR stream closed');
    }
    async send(chunk: SsrStreamChunk): Promise<void> {
      await this.ready();
      let consumed!: () => void;
      const ack = new Promise<void>(resolve => {
        consumed = resolve;
      });
      this.slot = { chunk, consumed };
      this.notify();
      await ack;
      if (this.ended && this.hasFailure) throw this.failure;
    }
    async next(): Promise<IteratorResult<SsrStreamChunk>> {
      while (!this.slot && !this.ended) await this.changed;
      if (this.slot) {
        const slot = this.slot;
        this.slot = undefined;
        slot.consumed();
        this.notify();
        return { done: false, value: slot.chunk };
      }
      if (this.hasFailure) throw this.failure;
      return { done: true, value: undefined };
    }
    assertHealthy(): void {
      if (this.ended && this.hasFailure) throw this.failure;
    }
    close(failure?: unknown, failed = false): void {
      if (this.ended) return;
      this.ended = true;
      this.failure = failure;
      this.hasFailure = failed;
      if (failed && this.slot) {
        this.slot.consumed();
        this.slot = undefined;
      }
      this.notify();
    }
  }
  /**
   * Progressive Node SSR data: shell, ready boundary patches, then authoritative
   * final HTML/state. Consume with createSsrStreamReceiver. This is not raw HTML.
   * One chunk is queued; return() or the host AbortSignal closes request ownership.
   */
  interface StreamIteratorSlot {
    current?: AsyncIterableIterator<SsrStreamChunk>;
    terminal: boolean;
    failed: boolean;
    failure?: unknown;
  }
  function renderToChunksAsync(
    view: () => AsyncSsrView | PromiseLike<AsyncSsrView>,
    options: SsrStreamOptions = {},
  ): AsyncIterableIterator<SsrStreamChunk> {
    return chunkStream(view, options);
  }
  function chunkStream(
    view: () => AsyncSsrView | PromiseLike<AsyncSsrView>,
    options: SsrStreamOptions,
    onFailure?: (failure: unknown) => void,
  ): AsyncIterableIterator<SsrStreamChunk> {
    const slot: StreamIteratorSlot = { terminal: false, failed: false };
    const iterator = createStreamIterator(view, options, (failed, failure) => {
      const notify = !slot.terminal && failed;
      slot.terminal = true;
      slot.current = undefined;
      slot.failed = failed;
      slot.failure = failure;
      if (notify) onFailure?.(failure);
    });
    if (!slot.terminal) slot.current = iterator;
    return streamIteratorHandle(slot);
  }
  // Terminal handles retain only a pending failure, never the producer closure.
  function streamIteratorHandle(slot: StreamIteratorSlot): AsyncIterableIterator<SsrStreamChunk> {
    const acknowledge = () => {
      slot.current = undefined;
      slot.failed = false;
      slot.failure = undefined;
    };
    return {
      [Symbol.asyncIterator]() {
        return this;
      },
      async next() {
        const iterator = slot.current;
        if (!iterator) {
          if (slot.failed) {
            const failure = slot.failure;
            acknowledge();
            throw failure;
          }
          return { done: true, value: undefined };
        }
        try {
          const result = await iterator.next();
          if (result.done) acknowledge();
          return result;
        } catch (error) {
          if (slot.terminal) acknowledge();
          throw error;
        }
      },
      async return() {
        const iterator = slot.current;
        try {
          return iterator ? await iterator.return!() : { done: true, value: undefined };
        } finally {
          acknowledge();
        }
      },
      async throw(error) {
        await this.return!();
        throw error;
      },
    };
  }
  function createStreamIterator(
    view: () => AsyncSsrView | PromiseLike<AsyncSsrView>,
    options: SsrStreamOptions,
    retire: (failed: boolean, failure: unknown) => void,
  ): AsyncIterableIterator<SsrStreamChunk> {
    const assertCaller = captureViewLifetime();
    assertCaller();
    const caller = getCurrentOwner();
    const inputSignal = options.signal;
    assertCaller();
    const timeoutMs = options.timeoutMs;
    assertCaller();
    const maxBytes = options.maxChunkBytes ?? 8 * 1024 * 1024;
    assertCaller();
    const maxBoundaries = options.maxBoundaries ?? 1024;
    assertCaller();
    if (
      !Number.isSafeInteger(maxBytes) ||
      maxBytes < 1 ||
      !Number.isSafeInteger(maxBoundaries) ||
      maxBoundaries < 1
    )
      throw RangeError('Invalid SSR stream limits');
    const deadline = deadlineSignal(inputSignal, timeoutMs),
      userSignal = deadline.signal;
    const controller = new AbortController(),
      channel = new SsrChunkChannel(),
      encoder = new TextEncoder();
    let sequence = 0,
      hasTerminalFailure = false,
      terminalFailure: unknown;
    let started = false,
      done = false,
      reading = false,
      producer: Promise<void> | undefined;
    const fail = (failure: unknown) => {
      hasTerminalFailure = true;
      terminalFailure = failure;
      if (!controller.signal.aborted) controller.abort(failure);
      channel.close(failure, true);
      if (!started) release();
    };
    const forward = () => fail(userSignal!.reason);
    const stop = () => fail(Error('SSR calling owner disposed'));
    function release() {
      deadline.dispose();
      retire(hasTerminalFailure, terminalFailure);
      try {
        userSignal?.removeEventListener('abort', forward);
      } catch {
        /* Still release owner hook. */
      }
      if (caller && !caller.disposed) {
        const i = caller.disposables.indexOf(stop);
        if (i >= 0) caller.disposables.splice(i, 1);
      }
    }
    try {
      if (caller) onCleanup(stop);
      userSignal?.addEventListener('abort', forward, { once: true });
      if (userSignal?.aborted) forward();
      assertCaller();
    } catch (error) {
      release();
      throw error;
    }
    const send = async (record: SsrStreamPayload & { streamId: string }) => {
      const { streamId, ...payload } = record;
      const chunk: SsrStreamChunk = {
        version: SSR_STREAM_VERSION,
        sequence: sequence++,
        streamId,
        payload,
      };
      controller.signal.throwIfAborted();
      if (encoder.encode(JSON.stringify(chunk)).byteLength > maxBytes)
        throw RangeError('SSR stream chunk exceeds maxChunkBytes');
      await channel.send(chunk);
    };
    const start = () => {
      if (started) return;
      controller.signal.throwIfAborted();
      started = true;
      producer = runWithIsolatedContextAsync(
        async () => {
          const assertRequest = captureViewLifetime();
          let notifyFailure!: () => void;
          const failureReady = new Promise<void>(resolve => {
            notifyFailure = resolve;
          });
          const published = new Set<number>();
          let publication = Promise.resolve();
          const stream: StreamPublication = {
            limit: maxBoundaries,
            id: randomUUID(),
            sequence: 0,
            records: new Map(),
            fail,
            publish(id, node, commit) {
              const job = publication.then(async () => {
                controller.signal.throwIfAborted();
                assertRequest();
                assertCaller();
                if (!published.has(id)) {
                  commit();
                  return;
                }
                await channel.ready();
                controller.signal.throwIfAborted();
                assertRequest();
                assertCaller();
                const html = serializeDomNodeToHtml(node);
                mark(node);
                commit();
                await send({ type: 'patch', streamId: stream.id, boundary: id, html });
              });
              publication = job.catch(() => {});
              return job;
            },
          };
          const mark = (node: Node) => {
            // Inspect actual native comment identities, not strings supplied by users.
            const pending: Node[] = [node];
            const starts = new Map<Comment, number>();
            for (const [id, record] of stream.records) starts.set(record.start, id);
            while (pending.length) {
              const current = pending.pop()!,
                id = starts.get(current as Comment);
              if (id !== undefined) published.add(id);
              for (const child of Array.from(current.childNodes)) pending.push(child);
            }
          };
          const session: AsyncRenderSession = {
            pending: new Set(),
            failed: false,
            failureReady,
            notifyFailure,
            closed: false,
            stream,
          };
          onCleanup(() => retireRenderSession(session));
          return renders.run(session, async () => {
            try {
              const result = await view();
              assertRequest();
              assertCaller();
              const node = result.node;
              assertRequest();
              assertCaller();
              mark(node);
              await send({
                type: 'shell',
                streamId: stream.id,
                html: serializeDomNodeToHtml(node),
              });
              await settleComponents(session);
              assertRequest();
              assertCaller();
              const signals = result.signals;
              assertRequest();
              assertCaller();
              await settleComponents(session);
              session.closed = true;
              const stateJson = serializeState(signals ?? {});
              assertRequest();
              assertCaller();
              for (const record of stream.records.values()) {
                record.start.remove();
                record.end.remove();
              }
              await send({
                type: 'complete',
                streamId: stream.id,
                html: serializeDomNodeToHtml(node),
                stateJson,
              });
            } finally {
              retireRenderSession(session);
            }
          });
        },
        { signal: controller.signal },
      )
        .then(
          () => channel.close(),
          error => fail(error),
        )
        .finally(release);
    };
    const iterator: AsyncIterableIterator<SsrStreamChunk> = {
      [Symbol.asyncIterator]() {
        return this;
      },
      async next() {
        if (done) return { done: true, value: undefined };
        if (reading) throw Error('Concurrent SSR stream next() calls are unsupported');
        reading = true;
        try {
          assertCaller();
          start();
          const result = await channel.next();
          channel.assertHealthy();
          assertCaller();
          if (result.done) {
            done = true;
            await producer;
          }
          return result;
        } catch (error) {
          done = true;
          fail(error);
          await producer;
          release();
          throw error;
        } finally {
          reading = false;
        }
      },
      async return() {
        done = true;
        fail(Error('SSR stream consumer closed'));
        await producer;
        release();
        return { done: true, value: undefined };
      },
      async throw(error) {
        await this.return!();
        throw error;
      },
    };
    return iterator;
  }

  /** SSR exposed as pull-driven UTF-8 NDJSON; no eager reads or gzip policy. */
  function renderToReadableStreamAsync(
    view: () => AsyncSsrView | PromiseLike<AsyncSsrView>,
    options: SsrReadableStreamOptions = {},
  ): ReadableStream<Uint8Array> {
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
    return readableStreamFromChunks(
      notify => chunkStream(view, { signal, timeoutMs, maxChunkBytes, maxBoundaries }, notify),
      signal,
      maxChunkBytes,
      codec,
      getCurrentOwner(),
    );
  }

  // Separate closure: a retained terminal stream must not retain the view argument.
  function readableStreamFromChunks(
    makeIterator:
      ((notify: (failure: unknown) => void) => AsyncIterableIterator<SsrStreamChunk>) | undefined,
    signal: AbortSignal | undefined,
    maxChunkBytes: number | undefined,
    codec: import('./stream-protocol.ts').SsrStreamCodec,
    caller: OwnerScope | null | undefined,
  ): ReadableStream<Uint8Array> {
    let iterator: AsyncIterableIterator<SsrStreamChunk> | undefined;
    const utf8 = new TextEncoder(),
      encoder =
        codec === 'identity' ? undefined : createSsrStreamTransportEncoder({ maxChunkBytes });
    let active = true,
      controller: ReadableStreamDefaultController<Uint8Array>;
    // Retain the native stream through producer teardown. A host reader can
    // detach during error delivery before its closed observer has resumed.
    let stream: ReadableStream<Uint8Array> | undefined;
    const finish = (mode: 'close' | 'cancel' | 'error', error?: unknown): Promise<void> => {
      if (!active) return Promise.resolve();
      active = false;
      const current = iterator;
      iterator = undefined;
      try {
        signal?.removeEventListener('abort', abort);
      } catch {
        /* Continue releasing request ownership. */
      }
      signal = undefined;
      if (caller && !caller.disposed) {
        const index = caller.disposables.indexOf(ownerDisposed);
        if (index >= 0) caller.disposables.splice(index, 1);
      }
      caller = undefined;
      encoder?.dispose();
      if (mode === 'close') controller.close();
      else if (mode === 'error') controller.error(error);
      return Promise.resolve(current?.return?.()).then(
        () => {
          stream = undefined;
        },
        failure => {
          stream = undefined;
          throw failure;
        },
      );
    };
    const abort = () => {
      void finish('error', signal?.reason).catch(() => {});
    };
    const ownerDisposed = () => {
      void finish('error', Error('SSR calling owner disposed')).catch(() => {});
    };
    return (stream = new ReadableStream<Uint8Array>(
      {
        start(value) {
          controller = value;
          try {
            const create = makeIterator!;
            makeIterator = undefined;
            const current = create(failure => {
              void finish('error', failure).catch(() => {});
            });
            if (!active) {
              void current.return?.().catch(() => {});
              return;
            }
            iterator = current;
            signal?.addEventListener('abort', abort, { once: true });
            if (caller) onCleanup(ownerDisposed);
            if (signal?.aborted) abort();
          } catch (error) {
            void finish('error', error).catch(() => {});
          }
        },
        async pull() {
          const current = iterator;
          if (!active || !current) return;
          try {
            const next = await current.next();
            if (!active) return;
            if (next.done) {
              await finish('close');
              return;
            }
            controller.enqueue(
              utf8.encode(JSON.stringify(encoder ? encoder.encode(next.value) : next.value) + '\n'),
            );
            // Complete is the final data frame. Acknowledge producer teardown and
            // close without requiring the consumer to request an extra record.
            if (next.value.payload.type === 'complete') {
              const terminal = await current.next();
              if (!active) return;
              if (!terminal.done) throw Error('Unexpected SSR frame after complete');
              await finish('close');
            }
          } catch (error) {
            await finish('error', error);
          }
        },
        cancel() {
          return finish('cancel');
        },
      },
      { highWaterMark: 0 },
    ));
  }

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
