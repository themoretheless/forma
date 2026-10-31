/**
 * SSR serialization, signal/DOM binding resumption, and optimistic RPC actions.
 * Resumption does not re-execute components, but allocates each restored signal
 * and binding. Without an explicit binding table it scans annotated descendants.
 * Async request-local context propagation is provided by the separate Node server entry.
 */

import { batch, createRoot, effect, getCurrentOwner, getReactiveGeneration, isOwnerScopeDisposed, runUntrackedInCore, registerReactiveResetFinalizer, onCleanup, resetReactiveSystem, runWithIsolatedContext, signal, type Signal } from './reactivity.ts';
import { getWasmCore, withWasmCore } from './wasm-bridge.ts';
import { applyAttributeValue, bindProp, captureViewLifetime } from './dom.ts';

const VOID_ELEMENTS = new Set([
  'area',
  'base',
  'br',
  'col',
  'embed',
  'hr',
  'img',
  'input',
  'link',
  'meta',
  'param',
  'source',
  'track',
  'wbr',
]);

function escapeHtml(str: string): string {
  return str
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#39;');
}

function escapeJsonForScript(jsonStr: string): string {
  return jsonStr
    .replace(/</g, '\\u003c')
    .replace(/>/g, '\\u003e')
    .replace(/&/g, '\\u0026')
    .replace(/\u2028/g, '\\u2028')
    .replace(/\u2029/g, '\\u2029');
}

const URL_ATTRS = new Set(['href', 'src', 'xlink:href', 'action', 'formaction']);

function isSafeAttributeBinding(attrName: string, valStr: string): boolean {
  const lowerAttr = attrName.toLowerCase().trim();
  if (!lowerAttr || lowerAttr.startsWith('on') || /[\s"'<>=/]/.test(lowerAttr)) {
    return false;
  }
  if (URL_ATTRS.has(lowerAttr)) {
    const cleanUrl = valStr.replace(/[\u0000-\u0020]+/g, '').toLowerCase();
    if (
      cleanUrl.startsWith('javascript:') ||
      cleanUrl.startsWith('vbscript:') ||
      cleanUrl.startsWith('data:text/html')
    ) {
      return false;
    }
  }
  return true;
}

/**
 * Serializes a real or JSDOM `Node` tree into an HTML string.
 */
export function serializeDomNodeToHtml(node: Node): string {
  type Frame = { node: Node } | { close: string };
  const frames: Frame[] = [{ node }], chunks: string[] = [];
  const pushChildren = (parent: Node) => {
    const children = Array.from(parent.childNodes);
    for (let i = children.length - 1; i >= 0; i--) frames.push({ node: children[i] });
  };
  while (frames.length) {
    const frame = frames.pop()!;
    if ('close' in frame) { chunks.push(frame.close); continue; }
    const current = frame.node;
    if (current.nodeType === 3) {
      chunks.push(escapeHtml(current.textContent ?? ''));
    } else if (current.nodeType === 8) {
      const commentText = current.nodeValue ?? '';
      // Internal dynamic anchors remain absent from SSR HTML.
      if (!commentText.startsWith('svet-')) {
        const safeComment = commentText.replace(/-->/g, '--&gt;').replace(/--!>/g, '--!&gt;');
        chunks.push(`<!--${safeComment}-->`);
      }
    } else if (current.nodeType === 11) {
      pushChildren(current);
    } else if (current.nodeType === 1) {
      const el = current as HTMLElement, tag = el.tagName.toLowerCase();
      let attrs = '';
      for (let i = 0; i < el.attributes.length; i++) {
        const attr = el.attributes[i];
        if (tag === 'input' && (attr.name === 'value' || attr.name === 'checked')) continue;
        if (tag === 'option' && attr.name === 'selected') continue;
        attrs += ` ${attr.name}="${escapeHtml(attr.value)}"`;
      }
      if (tag === 'input') {
        const input = el as HTMLInputElement;
        if (input.type !== 'file' && (input.value || el.hasAttribute('value'))) attrs += ` value="${escapeHtml(input.value)}"`;
        if (input.checked) attrs += ' checked=""';
      }
      if (tag === 'option' && (el as HTMLOptionElement).selected) attrs += ' selected=""';
      if (VOID_ELEMENTS.has(tag)) { chunks.push(`<${tag}${attrs} />`); continue; }
      chunks.push(`<${tag}${attrs}>`);
      if (tag === 'textarea') {
        const value = (el as HTMLTextAreaElement).value;
        // HTML parsing removes the first LF immediately following <textarea>.
        chunks.push((value.startsWith('\n') ? '\n' : '') + escapeHtml(value), `</${tag}>`);
      } else {
        frames.push({ close: `</${tag}>` });
        pushChildren(el);
      }
    }
  }
  return chunks.join('');
}

/**
 * Renders a Svet view to an SSR HTML string inside an isolated WASM instance,
 * optionally embedding a `<script type="application/svet+json">` state snapshot for client resumption.
 */
export function renderToString(
  view: () => Node,
  namedSignals?: Record<string, Signal<any>>
): { html: string; stateJson: string } {
  const assertCallerCurrent = captureViewLifetime();
  assertCallerCurrent();
  return runWithIsolatedContext(() => {
    const core = getWasmCore();
    try {
      let html = '', stateJson = '{}';
      createRoot(dispose => {
        const assertScopeCurrent = captureViewLifetime();
        const assertCurrent = () => { assertCallerCurrent(); assertScopeCurrent(); };
        try {
          assertCurrent();
          const domTree = view();
          assertCurrent();
          html = serializeDomNodeToHtml(domTree);
          assertCurrent();
          if (namedSignals) {
            stateJson = serializeState(namedSignals);
            assertCurrent();
            html += `<script type="application/svet+json">${stateJson}</script>`;
          }
        } finally { dispose(); }
      });
      assertCallerCurrent();
      return { html, stateJson };
    } finally {
      // Basic signals need not register owner cleanup; the request owns its core.
      withWasmCore(core, resetReactiveSystem);
    }
  });
}

export interface ResumableBinding {
  elementId: string;
  signal: string;
  type: 'text' | 'attr' | 'model';
  attrName?: string;
}

export interface ResumableSnapshot {
  signals: Record<string, any>;
  bindings?: ResumableBinding[];
  timestamp: number;
}

/**
 * Serializes a named map of signals into a compact, XSS-safe JSON snapshot for SSR HTML injection.
 */
export function serializeState(signals: Record<string, Signal<any>>): string {
  const payload: ResumableSnapshot = {
    signals: Object.create(null),
    timestamp: Date.now(),
  };
  for (const [key, sig] of Object.entries(signals)) {
    if (key === '__proto__' || key === 'constructor' || key === 'prototype') continue;
    payload.signals[key] = sig.peek();
  }
  return escapeJsonForScript(JSON.stringify(payload));
}

const resumedContainers = new WeakMap<HTMLElement, () => void>();
const resumptionAttempts = new WeakMap<HTMLElement, object>();

/** Releases resumed subscriptions, signals, and model event listeners. */
export function disposeResumedContainer(container: HTMLElement): void {
  resumedContainers.get(container)?.();
}

/**
 * Restores signals and annotated DOM bindings without re-executing components.
 * Replaces a previous resumption and registers cleanup with the current owner.
 */
export function resumeContainer(
  container: HTMLElement,
  snapshotJson: string
): Record<string, Signal<any>> {
  const assertCallerCurrent = captureViewLifetime(), callerOwner = getCurrentOwner(), core = getWasmCore();
  assertCallerCurrent();
  const parsed = JSON.parse(snapshotJson) as ResumableSnapshot;
  if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed) ||
      !parsed.signals || typeof parsed.signals !== 'object' || Array.isArray(parsed.signals)) {
    throw new TypeError('Invalid Svet snapshot: signals must be an object');
  }
  if (parsed.bindings !== undefined && (!Array.isArray(parsed.bindings) ||
      parsed.bindings.some(b => !b || typeof b !== 'object' ||
        typeof b.elementId !== 'string' || typeof b.signal !== 'string' ||
        !['text', 'attr', 'model'].includes(b.type) ||
        (b.type === 'attr' && typeof b.attrName !== 'string')))) {
    throw new TypeError('Invalid Svet snapshot: malformed bindings');
  }

  // Validate before replacing live bindings or superseding an existing attempt.
  assertCallerCurrent();
  const attempt = {};
  resumptionAttempts.set(container, attempt);
  let disposeRoot: (() => void) | undefined, assertRootCurrent: (() => void) | undefined;
  let stopped = false;
  const stop = () => withWasmCore(core, () => {
    if (stopped) return;
    stopped = true;
    if (callerOwner && !callerOwner.disposed) {
      const index = callerOwner.disposables.indexOf(stop);
      if (index >= 0) callerOwner.disposables.splice(index, 1);
    }
    if (resumedContainers.get(container) === stop) resumedContainers.delete(container);
    disposeRoot?.();
  });
  const assertCurrent = () => {
    try {
      assertCallerCurrent();
      if (resumptionAttempts.get(container) !== attempt) throw new Error('Resumption superseded by a newer attempt');
      if (stopped) throw new Error('Resumption disposed during construction');
      assertRootCurrent?.();
    } catch (failure) {
      try { stop(); } catch { /* Preserve the lifetime/supersession failure. */ }
      throw failure;
    }
  };
  const restored: Record<string, Signal<any>> = Object.create(null);
  try {
    disposeResumedContainer(container);
    assertCurrent();
    // Make pending construction cancellable before any user-observable DOM write.
    onCleanup(stop);
    resumedContainers.set(container, stop);
    createRoot(dispose => {
      disposeRoot = dispose;
      assertRootCurrent = captureViewLifetime();
      assertCurrent();
      for (const [name, value] of Object.entries(parsed.signals)) {
        if (name === '__proto__' || name === 'constructor' || name === 'prototype') continue;
        const sig = signal(value);
        restored[name] = sig;
        onCleanup(sig.dispose);
      }

      const bind = (el: HTMLElement, key: string, type: ResumableBinding['type'], attrName?: string) => {
        assertCurrent();
        const sig = restored[key];
        if (!sig) return;
        if (type === 'text') {
          effect(() => {
            assertCurrent();
            const text = String(sig());
            assertCurrent();
            if (el.textContent !== text) el.textContent = text;
            assertCurrent();
          });
        } else if (type === 'attr' && attrName) {
          effect(() => {
            assertCurrent();
            const value = sig(), safe = isSafeAttributeBinding(attrName, String(value));
            assertCurrent();
            if (safe) applyAttributeValue(el, attrName, value, assertCurrent);
            assertCurrent();
          });
        } else if (type === 'model' && 'value' in el) {
          bindProp(el, 'model', sig);
        }
        assertCurrent();
      };

      if (parsed.bindings?.length) {
        let localIds: Map<string, HTMLElement> | undefined;
        for (const binding of parsed.bindings) {
          const candidate = container.ownerDocument.getElementById(binding.elementId);
          let el = candidate && container.contains(candidate) ? candidate : null;
          if (!el) {
            // Detached trees and duplicate document IDs need a container-local index.
            // Exact ID comparisons also handle punctuation without CSS interpolation.
            if (!localIds) {
              localIds = new Map();
              for (const node of [container, ...container.querySelectorAll<HTMLElement>('[id]')]) {
                if (node.id && !localIds.has(node.id)) localIds.set(node.id, node);
              }
            }
            el = localIds.get(binding.elementId) ?? null;
          }
          if (el) bind(el, binding.signal, binding.type, binding.attrName);
        }
      } else {
        const selector = '[data-svet-text],[data-svet-attr],[data-svet-model]';
        const targets = [...container.querySelectorAll<HTMLElement>(selector)];
        if (container.matches(selector)) targets.unshift(container);
        for (const el of targets) {
          const textKey = el.getAttribute('data-svet-text');
          if (textKey) bind(el, textKey, 'text');
          const attrSpec = el.getAttribute('data-svet-attr');
          if (attrSpec) {
            const colon = attrSpec.indexOf(':');
            if (colon !== -1) bind(el, attrSpec.slice(colon + 1).trim(), 'attr', attrSpec.slice(0, colon).trim());
          }
          const modelKey = el.getAttribute('data-svet-model');
          if (modelKey) bind(el, modelKey, 'model');
        }
      }
      assertCurrent();
    });
    assertCurrent();
    return restored;
  } catch (failure) {
    try { stop(); } catch { /* Preserve the construction failure. */ }
    throw failure;
  }
}

export class ServerActionDisposedError extends Error {
  constructor() {
    super('Server action is disposed or its owner lifetime has ended');
    this.name = 'ServerActionDisposedError';
  }
}

export interface ServerAction<TArgs, TResult> {
  (args: TArgs): Promise<TResult>;
  pending: Signal<boolean>;
  error: Signal<Error | null>;
  dispose(): void;
}

/**
 * Runs optional optimistic local updates before an RPC and invokes that call's
 * rollback on failure. Pending counts overlapping calls; error reflects the latest
 * started call. Shared-state rollback coordination remains the caller's policy.
 */
export function createServerAction<TArgs, TResult>(options: {
  rpc: (args: TArgs, signal: AbortSignal) => Promise<TResult>;
  optimisticUpdate?: (args: TArgs) => () => void;
}): ServerAction<TArgs, TResult> {
  const owner = getCurrentOwner(), core = getWasmCore(), generation = getReactiveGeneration(core);
  if (isOwnerScopeDisposed(owner)) throw new ServerActionDisposedError();
  const pending = signal(false);
  const error = signal<Error | null>(null);
  type Request = { controller: AbortController; reject: (error: Error) => void };
  const requests = new Set<Request>();
  let disposed = false;
  let latestRequest: Request | undefined;
  let unregisterReset: (() => void) | undefined;
  const isCurrent = () => !disposed && !isOwnerScopeDisposed(owner) && generation === getReactiveGeneration(core);
  const dispose = () => runUntrackedInCore(core, () => {
    if (disposed) return;
    disposed = true; latestRequest = undefined;
    unregisterReset?.(); unregisterReset = undefined;
    if (owner && !owner.disposed) {
      const index = owner.disposables.indexOf(dispose);
      if (index >= 0) owner.disposables.splice(index, 1);
    }
    const active = Array.from(requests);
    requests.clear();
    const reason = new ServerActionDisposedError();
    for (const request of active) {
      request.reject(reason);
      try { request.controller.abort(reason); } catch { /* Finish every request and signal teardown. */ }
    }
    try { pending.set(false); }
    finally { try { pending.dispose(); } finally { error.dispose(); } }
  });
  const assertCurrent = () => {
    if (!isCurrent()) {
      try { dispose(); } catch { /* Preserve the typed lifetime failure. */ }
      throw new ServerActionDisposedError();
    }
  };
  onCleanup(dispose);
  const runCallback = <T>(callback: () => T): T => runUntrackedInCore(core, () => {
    assertCurrent();
    return callback();
  });
  const normalizeError = (value: unknown): Error => runCallback(() => {
    if (value instanceof Error) return value;
    try { return new Error(String(value)); }
    catch { return new Error('Server action rejected with an unprintable value', { cause: value }); }
  });

  const action = (async (args: TArgs): Promise<TResult> => {
    assertCurrent();
    let rejectCancellation!: (error: Error) => void;
    const cancelled = new Promise<never>((_resolve, reject) => { rejectCancellation = reject; });
    // Disposal may happen synchronously before RPC/race setup.
    void cancelled.catch(() => {});
    const request: Request = { controller: new AbortController(), reject: rejectCancellation };
    latestRequest = request; requests.add(request);
    unregisterReset ??= registerReactiveResetFinalizer(core, dispose);
    let failed = false;
    let rollback: (() => void) | undefined;
    try {
      runCallback(() => batch(() => { pending.set(true); error.set(null); }));
      assertCurrent();
      const optimisticUpdate = runCallback(() => options.optimisticUpdate);
      assertCurrent();
      rollback = runCallback(() => optimisticUpdate?.call(options, args));
      assertCurrent();
      const rpc = runCallback(() => options.rpc);
      assertCurrent();
      const operation = runCallback(() => rpc.call(options, args, request.controller.signal));
      // Fresh targets avoid invariants of frozen thenables. Native Promise
      // resolution controls first settlement and schedules each then-method job;
      // adapters only bind protocol access/calls to this core. Resolving through
      // another thenable gets another adapter, without synchronously walking it.
      let adoptedValues: WeakMap<object, object> | undefined;
      const adopt = (value: unknown): unknown => {
        if (value === null || (typeof value !== 'object' && typeof value !== 'function')) return value;
        const adapter: object = new Proxy(Object.create(null), {
          get(_target, key) {
            if (key !== 'then') return undefined;
            const then = runUntrackedInCore(core, () => Reflect.get(value, 'then', value));
            if (typeof then === 'function') return (resolve: (next: unknown) => void, reject: (reason: unknown) => void) =>
              runUntrackedInCore(core, () => Reflect.apply(then, value, [
                (next: unknown) => resolve(adopt(next)), reject,
              ]));
            // Only terminal object adapters can reach the public return value.
            // Promise/thenable adapters need no reverse entry (or map allocation).
            (adoptedValues ??= new WeakMap()).set(adapter, value);
            return then;
          },
        });
        return adapter;
      };
      // Always observe RPC results, even after synchronous disposal. Skipping
      // adoption can leave an already-rejected native Promise unhandled.
      const adopted = adopt(operation) as Promise<TResult>;
      const result = await runUntrackedInCore(core, () => Promise.race([adopted, cancelled]));
      assertCurrent();
      return (result !== null && (typeof result === 'object' || typeof result === 'function')
        ? adoptedValues?.get(result as object) ?? result : result) as TResult;
    } catch (err) {
      failed = true;
      assertCurrent();
      let failure = normalizeError(err);
      assertCurrent();
      if (rollback) {
        try { runCallback(rollback); }
        catch (rollbackError) {
          failure = new AggregateError([failure, normalizeError(rollbackError)],
            'Server action and optimistic rollback failed', { cause: failure });
        }
      }
      if (isCurrent() && latestRequest === request) {
        try { error.set(failure); } catch { /* Observer errors must not replace the action failure. */ }
      }
      throw failure;
    } finally {
      requests.delete(request);
      if (!requests.size) { unregisterReset?.(); unregisterReset = undefined; }
      if (isCurrent()) {
        try { pending.set(requests.size > 0); }
        catch (observerError) { if (!failed) throw observerError; }
      } else { try { dispose(); } catch { /* Complete invalidated request finalization. */ } }
    }
  }) as ServerAction<TArgs, TResult>;

  action.pending = pending;
  action.error = error;
  action.dispose = dispose;

  return action;
}
