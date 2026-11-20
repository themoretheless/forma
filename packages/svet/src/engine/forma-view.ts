import { insertChild } from './dom.ts';
import type { FormaAstNode } from './forma-ast.ts';
import type { PreparedFormaView } from './forma-hmr.ts';
import { createRoot, onCleanup, untrack } from './reactivity.ts';

export interface FormaViewLifecycleOptions {
  componentName: string;
  rootAstNodes: FormaAstNode[];
  shortcuts: Array<{ combo: string; action: string }>;
  assertLive(): void;
  renderAstNode(node: FormaAstNode): Node;
  executeFormaAction(action: string): void;
}

export interface FormaViewLifecycle {
  readonly prepareViews: Set<() => PreparedFormaView>;
  registerKey(key: string, element: HTMLElement): void;
  getElementByKey(key: string): HTMLElement | undefined;
  render(): DocumentFragment;
  mount(container: HTMLElement): { unmount(): void };
  disposeViews(): { failed: boolean; firstError: unknown };
}

export function createFormaViewLifecycle(options: FormaViewLifecycleOptions): FormaViewLifecycle {
  const { componentName, rootAstNodes, shortcuts, assertLive, renderAstNode, executeFormaAction } =
    options;
  let keyedElements = new Map<string, HTMLElement>();
  const viewKeyMaps = new Set<Map<string, HTMLElement>>();
  const prepareViews = new Set<() => PreparedFormaView>();
  const renderDisposers = new Set<() => void>();
  const mountDisposers = new Set<() => void>();

  const render = (): DocumentFragment => {
    assertLive();
    const frag = document.createDocumentFragment();
    const anchor = document.createComment(`svet-forma-hmr:${componentName}`);
    frag.appendChild(anchor);

    let currentNodes: Node[] = [];
    let disposeView: (() => void) | null = null;
    let currentViewKeys: Map<string, HTMLElement> | null = null;
    let renderRetired = false;
    let pendingViewDispose: (() => void) | null = null;
    let abortPrepared: (() => void) | null = null;
    const assertViewLive = () => {
      assertLive();
      if (renderRetired) throw new Error('Forma view is disposed');
    };

    const clearCurrentView = () => {
      if (disposeView) {
        const stop = disposeView;
        disposeView = null;
        stop();
      }
      for (const n of currentNodes) {
        if (n.parentNode) n.parentNode.removeChild(n);
      }
      currentNodes = [];
      if (currentViewKeys) {
        viewKeyMaps.delete(currentViewKeys);
        if (keyedElements === currentViewKeys) {
          keyedElements = new Map();
          for (const remaining of viewKeyMaps) keyedElements = remaining;
        }
        currentViewKeys = null;
      }
    };

    const mountCurrentView = (): PreparedFormaView => {
      const stage: { dispose: (() => void) | null } = { dispose: null };
      let stagedNodes: Node[] = [];
      const previousKeys = keyedElements;
      const nextKeys = new Map<string, HTMLElement>();
      keyedElements = nextKeys;
      try {
        createRoot(dispose => {
          stage.dispose = dispose;
          pendingViewDispose = dispose;
          const temp = document.createDocumentFragment();
          for (const nodeAst of rootAstNodes) {
            insertChild(
              temp,
              untrack(() => renderAstNode(nodeAst)),
            );
            assertViewLive();
          }
          if (shortcuts.length > 0 && typeof window !== 'undefined') {
            const compiledShortcuts = shortcuts.map(sc => {
              const parts = sc.combo.split('+');
              const keyLower = (parts[parts.length - 1] ?? '').toLowerCase();
              const needsMod =
                parts.includes('Mod') || parts.includes('Ctrl') || parts.includes('Meta');
              return { keyLower, needsMod, action: sc.action };
            });
            const onKeyDown = (e: KeyboardEvent) => {
              const modPressed = e.metaKey || e.ctrlKey;
              const keyLower = e.key.toLowerCase();
              for (const sc of compiledShortcuts) {
                if (sc.needsMod && !modPressed) continue;
                if (keyLower === sc.keyLower) {
                  e.preventDefault();
                  executeFormaAction(sc.action);
                }
              }
            };
            window.addEventListener('keydown', onKeyDown);
            onCleanup(() => window.removeEventListener('keydown', onKeyDown));
          }
          stagedNodes = Array.from(temp.childNodes);
          const parent = anchor.parentNode ?? frag;
          for (const n of stagedNodes) {
            parent.insertBefore(n, anchor);
            assertViewLive();
          }
        });
        let settled = false;
        const prepared: PreparedFormaView = {
          commit() {
            if (settled) return;
            assertViewLive();
            clearCurrentView();
            assertViewLive();
            settled = true;
            abortPrepared = null;
            disposeView = stage.dispose;
            currentNodes = stagedNodes;
            keyedElements = nextKeys;
            currentViewKeys = nextKeys;
            viewKeyMaps.add(nextKeys);
          },
          abort() {
            if (settled) return;
            settled = true;
            abortPrepared = null;
            stage.dispose?.();
            for (const node of stagedNodes) node.parentNode?.removeChild(node);
            if (!renderRetired) keyedElements = previousKeys;
          },
        };
        abortPrepared = prepared.abort;
        return prepared;
      } catch (error) {
        stage.dispose?.();
        for (const node of stagedNodes) node.parentNode?.removeChild(node);
        if (!renderRetired) keyedElements = previousKeys;
        throw error;
      } finally {
        pendingViewDispose = null;
      }
    };

    mountCurrentView().commit();
    prepareViews.add(mountCurrentView);

    const retireRender = () => {
      if (renderRetired) return;
      renderRetired = true;
      renderDisposers.delete(retireRender);
      prepareViews.delete(mountCurrentView);
      abortPrepared?.();
      pendingViewDispose?.();
      clearCurrentView();
      anchor.remove();
    };
    renderDisposers.add(retireRender);
    onCleanup(retireRender);
    return frag;
  };

  const mount = (container: HTMLElement) => {
    assertLive();
    container.innerHTML = '';
    let disposeRoot!: () => void;
    const rootFrag = createRoot(dispose => {
      disposeRoot = dispose;
      return render();
    });

    let unmounted = false;
    const unmount = () => {
      if (unmounted) return;
      unmounted = true;
      mountDisposers.delete(unmount);
      disposeRoot();
    };
    mountDisposers.add(unmount);
    try {
      container.appendChild(rootFrag);
      assertLive();
    } catch (error) {
      unmount();
      throw error;
    }
    return { unmount };
  };

  const disposeViews = () => {
    let failed = false;
    let firstError: unknown;
    const recordFailure = (error: unknown) => {
      if (!failed) firstError = error;
      failed = true;
    };
    for (const stop of [...mountDisposers]) {
      try {
        stop();
      } catch (error) {
        recordFailure(error);
      }
    }
    for (const stop of [...renderDisposers]) {
      try {
        stop();
      } catch (error) {
        recordFailure(error);
      }
    }
    keyedElements.clear();
    viewKeyMaps.clear();
    mountDisposers.clear();
    renderDisposers.clear();
    prepareViews.clear();
    return { failed, firstError };
  };

  return {
    prepareViews,
    registerKey: (key: string, element: HTMLElement) => {
      keyedElements.set(key, element);
    },
    getElementByKey: (key: string) => keyedElements.get(key),
    render,
    mount,
    disposeViews,
  };
}
