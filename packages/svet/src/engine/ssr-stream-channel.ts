/**
 * Progressive SSR stream rendezvous channel, chunk iterator lifecycle, deadline
 * signals, and pull-driven Web `ReadableStream` framing.
 */

import { captureViewLifetime } from './dom.ts';
import { getCurrentOwner, onCleanup, type OwnerScope, type ReadonlySignal } from './reactivity.ts';
import { serializeDomNodeToHtml, serializeState } from './resumable.ts';
import { createSsrStreamTransportEncoder } from './stream-codec.ts';
import {
  SSR_STREAM_VERSION,
  type SsrStreamChunk,
  type SsrStreamCodec,
  type SsrStreamPayload,
} from './stream-protocol.ts';

export interface SsrRequestOptions {
  /** Abort on disconnect or a host-controlled deadline. Awaited user work must honor it too. */
  signal?: AbortSignal | undefined;
  /** Optional timeout timer in integer milliseconds (1..2147483647); cannot preempt synchronous work. */
  timeoutMs?: number | undefined;
}

export interface AsyncSsrView {
  node: Node;
  signals?: Record<string, ReadonlySignal<unknown>>;
}

export interface SsrStreamOptions extends SsrRequestOptions {
  /** Encoded payload limit for each shell, patch or final snapshot (default 8 MiB). */
  maxChunkBytes?: number | undefined;
  /** Maximum async boundaries created by one stream (default 1024). */
  maxBoundaries?: number | undefined;
}

export interface SsrReadableStreamOptions extends SsrStreamOptions {
  /** Transport framing must be explicitly agreed with the client. Default identity. */
  codec?: SsrStreamCodec;
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

export function deadlineSignal(signal: AbortSignal | undefined, timeoutMs: number | undefined) {
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

export async function awaitRequest<T>(
  fn: () => T | PromiseLike<T>,
  signal?: AbortSignal,
): Promise<T> {
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

export type StreamPublication = {
  limit: number;
  id: string;
  sequence: number;
  records: Map<number, { start: Comment; end: Comment }>;
  publish: (id: number, node: Node, commit: () => void) => Promise<void>;
  fail: (failure: unknown) => void;
};

export type AsyncRenderSession = {
  stream?: StreamPublication | undefined;
  pending: Set<Promise<void>>;
  failed: boolean;
  failure?: unknown;
  failureReady: Promise<void>;
  notifyFailure: () => void;
  closed: boolean;
};

export function retireRenderSession(session: AsyncRenderSession): void {
  session.closed = true;
  session.pending.clear();
  session.stream = undefined;
  session.failure = undefined;
  session.notifyFailure();
  session.notifyFailure = () => {};
}

export async function settleComponents(session: AsyncRenderSession): Promise<void> {
  while (session.pending.size) {
    await Promise.race([Promise.all(Array.from(session.pending)), session.failureReady]);
    if (session.failed) throw session.failure;
  }
  if (session.failed) throw session.failure;
}

/** One-slot rendezvous queue. Return/cancellation also wakes an outstanding next(). */
export class SsrChunkChannel {
  private slot?: { chunk: SsrStreamChunk; consumed: () => void } | undefined;
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

export interface StreamIteratorSlot {
  current?: AsyncIterableIterator<SsrStreamChunk> | undefined;
  terminal: boolean;
  failed: boolean;
  failure?: unknown;
}

export interface SsrStreamRuntimeDeps {
  randomUUID(): string;
  renders: SsrAsyncContextCarrier<AsyncRenderSession>;
  runWithIsolatedContextAsync<T>(
    fn: () => T | PromiseLike<T>,
    options?: SsrRequestOptions,
  ): Promise<T>;
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
  deps: SsrStreamRuntimeDeps,
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
    producer = deps
      .runWithIsolatedContextAsync(
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
            id: deps.randomUUID(),
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
          return deps.renders.run(session, async () => {
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
  return {
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
}

export function chunkSsrStream(
  deps: SsrStreamRuntimeDeps,
  view: () => AsyncSsrView | PromiseLike<AsyncSsrView>,
  options: SsrStreamOptions,
  onFailure?: (failure: unknown) => void,
): AsyncIterableIterator<SsrStreamChunk> {
  const slot: StreamIteratorSlot = { terminal: false, failed: false };
  const iterator = createStreamIterator(deps, view, options, (failed, failure) => {
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

// Separate closure: a retained terminal stream must not retain the view argument.
export function readableSsrStreamFromChunks(
  makeIterator:
    ((notify: (failure: unknown) => void) => AsyncIterableIterator<SsrStreamChunk>) | undefined,
  signal: AbortSignal | undefined,
  maxChunkBytes: number | undefined,
  codec: SsrStreamCodec,
  caller: OwnerScope | null | undefined,
): ReadableStream<Uint8Array> {
  let iterator: AsyncIterableIterator<SsrStreamChunk> | undefined;
  const utf8 = new TextEncoder(),
    encoder = codec === 'identity' ? undefined : createSsrStreamTransportEncoder({ maxChunkBytes });
  let active = true,
    controller: ReadableStreamDefaultController<Uint8Array>;
  // Retain the native stream through producer teardown. A host reader can
  // detach during error delivery before its closed observer has resumed.
  // A holder rather than a local: the reference only has to exist, it is never read back.
  const retained: { stream?: ReadableStream<Uint8Array> | undefined } = {};
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
        retained.stream = undefined;
      },
      failure => {
        retained.stream = undefined;
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
  return (retained.stream = new ReadableStream<Uint8Array>(
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
