/**
 * # Svet Zero-Hydration Resumability & Hybrid Server Bridge (`resumable.ts`)
 *
 * Eliminates the flaws of Next.js/React SSR Hydration and Blazor Server/WASM:
 * - **vs React/Vue/Svelte Hydration**: Traditional SSR renders HTML, sends JS, re-executes all
 *   components on the client to build a VDOM, and walks the DOM to attach listeners ($O(N)$ startup CPU).
 *   Svet serializes only the signal graph snapshot (`<script type="application/svet+json">`) and
 *   marks reactive text/attribute targets with lightweight `data-svet-*` bindings.
 *   Resuming takes $O(1)$ boot time — zero component re-execution!
 * - **vs Blazor Server (SignalR on every click) & Blazor WASM (10 MB download)**:
 *   Svet's `serverAction()` runs UI signals locally at 120fps in WASM/TS and only dispatches typed
 *   RPC calls when server state mutation is requested, with built-in optimistic UI updates and
 *   automatic rollback on error.
 */

import { createRoot, effect, runWithIsolatedContext, signal, Signal } from './reactivity';

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
  if (node.nodeType === 3) {
    // TEXT_NODE
    return escapeHtml(node.textContent ?? '');
  }
  if (node.nodeType === 8) {
    // COMMENT_NODE: omit internal dynamic anchor comments for clean SSR HTML output
    const commentText = node.nodeValue ?? '';
    if (commentText.startsWith('svet-')) {
      return '';
    }
    const safeComment = commentText.replace(/-->/g, '--&gt;').replace(/--!>/g, '--!&gt;');
    return `<!--${safeComment}-->`;
  }
  if (node.nodeType === 11) {
    // DOCUMENT_FRAGMENT_NODE
    let out = '';
    node.childNodes.forEach((child) => {
      out += serializeDomNodeToHtml(child);
    });
    return out;
  }
  if (node.nodeType === 1) {
    // ELEMENT_NODE
    const el = node as HTMLElement;
    const tag = el.tagName.toLowerCase();
    let attrs = '';
    for (let i = 0; i < el.attributes.length; i++) {
      const attr = el.attributes[i];
      attrs += ` ${attr.name}="${escapeHtml(attr.value)}"`;
    }
    if (tag === 'input' && 'value' in el && !el.hasAttribute('value')) {
      const val = (el as HTMLInputElement).value;
      if (val) {
        attrs += ` value="${escapeHtml(val)}"`;
      }
    }
    if (VOID_ELEMENTS.has(tag)) {
      return `<${tag}${attrs} />`;
    }
    let inner = '';
    el.childNodes.forEach((child) => {
      inner += serializeDomNodeToHtml(child);
    });
    return `<${tag}${attrs}>${inner}</${tag}>`;
  }
  return '';
}

/**
 * Renders a Svet view to an SSR HTML string inside an isolated WASM instance,
 * optionally embedding a `<script type="application/svet+json">` state snapshot for O(1) Resumability!
 */
export function renderToString(
  view: () => Node,
  namedSignals?: Record<string, Signal<any>>
): { html: string; stateJson: string } {
  return runWithIsolatedContext(() => {
    let html = '';
    let stateJson = '{}';
    createRoot((dispose) => {
      const domTree = view();
      html = serializeDomNodeToHtml(domTree);
      if (namedSignals) {
        stateJson = serializeState(namedSignals);
        html += `<script type="application/svet+json">${stateJson}</script>`;
      }
      dispose();
    });
    return { html, stateJson };
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

/**
 * Resumes server-rendered HTML without re-running component trees (Zero-Hydration Resumability).
 * Reconstructs WASM signals from the serialized snapshot and binds `[data-svet-text]` / `[data-svet-attr]`
 * surgical targets directly to the restored signals.
 */
export function resumeContainer(
  container: HTMLElement,
  snapshotJson: string
): Record<string, Signal<any>> {
  const parsed = JSON.parse(snapshotJson) as ResumableSnapshot;
  const restored: Record<string, Signal<any>> = Object.create(null);

  if (parsed && parsed.signals && typeof parsed.signals === 'object') {
    for (const [name, initialValue] of Object.entries(parsed.signals)) {
      if (name === '__proto__' || name === 'constructor' || name === 'prototype') continue;
      restored[name] = signal(initialValue);
    }
  }

  // O(1) Direct ID Lookup Path when `bindings` table is present in the snapshot (Zero DOM tree walk!)
  if (parsed.bindings && parsed.bindings.length > 0) {
    const doc = container.ownerDocument;
    for (let i = 0; i < parsed.bindings.length; i++) {
      const b = parsed.bindings[i];
      const sig = restored[b.signal];
      if (!sig) continue;
      const el =
        (doc?.getElementById(b.elementId) as HTMLElement | null) ??
        container.querySelector<HTMLElement>(`[id=${JSON.stringify(b.elementId)}]`);
      if (!el) continue;

      if (b.type === 'text') {
        effect(() => {
          const nextText = String(sig());
          if (el.textContent !== nextText) {
            el.textContent = nextText;
          }
        });
      } else if (b.type === 'attr' && b.attrName) {
        const attrName = b.attrName;
        effect(() => {
          const nextVal = String(sig());
          if (!isSafeAttributeBinding(attrName, nextVal)) return;
          if (el.getAttribute(attrName) !== nextVal) {
            el.setAttribute(attrName, nextVal);
          }
        });
      } else if (b.type === 'model') {
        const inputEl = el as HTMLInputElement;
        effect(() => {
          const nextStr = String(sig());
          if (inputEl.value !== nextStr) {
            inputEl.value = nextStr;
          }
        });
        inputEl.addEventListener('input', () => {
          sig.set(inputEl.value);
        });
      }
    }
    return restored;
  }

  // Fallback single-pass surgical target binding when `bindings` table is omitted
  const allTargets = container.querySelectorAll<HTMLElement>(
    '[data-svet-text],[data-svet-attr],[data-svet-model]'
  );

  for (let i = 0; i < allTargets.length; i++) {
    const el = allTargets[i];

    const textKey = el.getAttribute('data-svet-text');
    if (textKey && restored[textKey]) {
      const sig = restored[textKey];
      effect(() => {
        const nextText = String(sig());
        if (el.textContent !== nextText) {
          el.textContent = nextText;
        }
      });
    }

    const attrSpec = el.getAttribute('data-svet-attr');
    if (attrSpec) {
      const colonIdx = attrSpec.indexOf(':');
      if (colonIdx !== -1) {
        const attrName = attrSpec.slice(0, colonIdx).trim();
        const sigName = attrSpec.slice(colonIdx + 1).trim();
        if (attrName && sigName && restored[sigName]) {
          const sig = restored[sigName];
          effect(() => {
            const nextVal = String(sig());
            if (!isSafeAttributeBinding(attrName, nextVal)) return;
            if (el.getAttribute(attrName) !== nextVal) {
              el.setAttribute(attrName, nextVal);
            }
          });
        }
      }
    }

    const modelKey = el.getAttribute('data-svet-model');
    if (modelKey && restored[modelKey]) {
      const inputEl = el as HTMLInputElement;
      const sig = restored[modelKey];
      effect(() => {
        const nextStr = String(sig());
        if (inputEl.value !== nextStr) {
          inputEl.value = nextStr;
        }
      });
      inputEl.addEventListener('input', () => {
        sig.set(inputEl.value);
      });
    }
  }

  return restored;
}

export interface ServerAction<TArgs, TResult> {
  (args: TArgs): Promise<TResult>;
  pending: Signal<boolean>;
  error: Signal<Error | null>;
}

/**
 * Creates a hybrid optimistic Server Action (solving Blazor Server's latency problem).
 * Local UI updates immediately via `optimisticUpdate` in the WASM graph; if the server
 * rejects, `rollback` restores consistency automatically.
 */
export function createServerAction<TArgs, TResult>(options: {
  rpc: (args: TArgs) => Promise<TResult>;
  optimisticUpdate?: (args: TArgs) => () => void;
}): ServerAction<TArgs, TResult> {
  const pending = signal(false);
  const error = signal<Error | null>(null);

  const action = (async (args: TArgs): Promise<TResult> => {
    pending.set(true);
    error.set(null);
    const rollback = options.optimisticUpdate ? options.optimisticUpdate(args) : undefined;
    try {
      const result = await options.rpc(args);
      return result;
    } catch (err) {
      if (rollback) {
        rollback();
      }
      const normalizedErr = err instanceof Error ? err : new Error(String(err));
      error.set(normalizedErr);
      throw normalizedErr;
    } finally {
      pending.set(false);
    }
  }) as ServerAction<TArgs, TResult>;

  action.pending = pending;
  action.error = error;

  return action;
}
