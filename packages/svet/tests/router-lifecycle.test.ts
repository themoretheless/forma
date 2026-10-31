/** @vitest-environment jsdom */
import {beforeEach, expect, it, vi} from 'vitest';
import {createRouter, RouterView, createRoot, runWithOwnerScope, onCleanup, getCurrentOwner, mount, signal, effect, createGpuTensor, getGraphTelemetry, resetReactiveSystem} from '../src/engine';

beforeEach(() => { vi.restoreAllMocks(); resetReactiveSystem(); document.body.innerHTML = ''; window.history.replaceState({},'', '/'); });
const routes = [{path: '/a', component: () => document.createTextNode('A')}, {path: '/b', component: () => document.createTextNode('B')}];

it('disposed routers cannot navigate, push browser history or invoke newly registered guards', () => {
  const router = createRouter({routes, initialPath:'/a'}), push = vi.spyOn(window.history,'pushState');
  router.dispose(); const guard = vi.fn(); router.beforeEach(guard);
  expect(router.navigate('/b')).toBe(false); expect(push).not.toHaveBeenCalled(); expect(guard).not.toHaveBeenCalled();
  expect(router.path()).toBe('/a'); push.mockRestore(); expect(getGraphTelemetry().nodes).toBe(0);
});

it('manual router disposal unregisters cleanup from its surviving owner', () => {
  createRoot(stop => {
    const owner = getCurrentOwner()!, baseline = owner.disposables.length;
    for (let i=0;i<100;i++) { const router=createRouter({routes,initialPath:'/a'}); router.dispose(); router.dispose(); }
    expect(owner.disposables).toHaveLength(baseline); expect(getGraphTelemetry().nodes).toBe(0); stop();
  });
});

it('guard disposal cancels navigation before history publication', () => {
  const router=createRouter({routes,initialPath:'/a'}), push=vi.spyOn(window.history,'pushState');
  router.beforeEach(() => { router.dispose(); });
  expect(router.navigate('/b')).toBe(false); expect(push).not.toHaveBeenCalled(); push.mockRestore();
});

it('a reentrant guard navigation supersedes its outer navigation', () => {
  const router=createRouter({routes,initialPath:'/a'});
  router.beforeEach(to => { if(to.path==='/b') router.navigate('/newer'); });
  expect(router.navigate('/b')).toBe(false); expect(router.path()).toBe('/newer'); expect(window.location.pathname).toBe('/newer'); router.dispose();
});

it('router construction under a disposed owner allocates no nodes', () => {
  createRoot(stop => { stop(); expect(() => createRouter({routes})).toThrow('owner'); expect(getGraphTelemetry().nodes).toBe(0); });
});

it('RouterView unmount disposes its independent active route scope', () => {
  const source=signal(0); let runs=0;
  const router=createRouter({initialPath:'/a',routes:[{path:'/a',component:()=>{ createGpuTensor([1]); effect(()=>{source();runs++;}); return document.createTextNode('A'); }}]});
  const app=mount(()=>RouterView({router}),document.body);
  expect(runs).toBe(1); app.unmount(); source.set(1); expect(runs).toBe(1);
  router.dispose(); source.dispose(); expect(getGraphTelemetry().nodes).toBe(0);
});

it('graph reset invalidates retained navigation handles before guards or history run', () => {
  const router=createRouter({routes,initialPath:'/a'}), guard=vi.fn(); router.beforeEach(guard);
  resetReactiveSystem(); const push=vi.spyOn(window.history,'pushState');
  expect(router.navigate('/b')).toBe(false); expect(guard).not.toHaveBeenCalled(); expect(push).not.toHaveBeenCalled();
  router.dispose(); push.mockRestore();
});

it('guards registered during a transition run only on a later navigation', () => {
  const router=createRouter({routes,initialPath:'/a'}), later=vi.fn();
  router.beforeEach(() => { router.beforeEach(later); });
  expect(router.navigate('/b')).toBe(true); expect(later).not.toHaveBeenCalled();
  router.navigate('/a'); expect(later).toHaveBeenCalledOnce(); router.dispose();
});

it('RouterView captures its router instead of following caller descriptor mutation', () => {
  const first=createRouter({routes,initialPath:'/a'}), second=createRouter({initialPath:'/a',routes:[{path:'*',component:()=>document.createTextNode('other')}]});
  const descriptor={router:first}; const app=mount(()=>RouterView(descriptor),document.body);
  descriptor.router=second; first.navigate('/b'); expect(document.body.textContent).toBe('B');
  app.unmount(); first.dispose(); second.dispose(); expect(getGraphTelemetry().nodes).toBe(0);
});


it.each(['root stop', 'effect rerun'] as const)('retiring router ignores navigation and popstate during %s', mode => {
  const trigger = signal(0);
  let stop!: () => void, router!: ReturnType<typeof createRouter>;
  let later!: NonNullable<ReturnType<typeof getCurrentOwner>>, result: boolean | undefined;
  const guard = vi.fn(), lateGuard = vi.fn();
  let stillUnvisited = false, snapshot = '';
  createRoot(dispose => {
    stop = dispose;
    const children = () => {
      runWithOwnerScope([], () => onCleanup(() => {
        stillUnvisited = !later.disposed;
        router.beforeEach(lateGuard);
        result = router.navigate('/b');
        window.dispatchEvent(new PopStateEvent('popstate'));
        snapshot = router.path();
      }));
      runWithOwnerScope([], () => {
        later = getCurrentOwner()!;
        router = createRouter({routes, initialPath: '/a'});
        router.beforeEach(guard);
      });
    };
    if (mode === 'root stop') children();
    else effect(() => { if (trigger() === 0) children(); });
  });
  // Simulate a native traversal that reaches this router before its own cleanup.
  window.history.replaceState({}, '', '/b');
  const push = vi.spyOn(window.history, 'pushState');
  const replace = vi.spyOn(window.history, 'replaceState');
  const go = vi.spyOn(window.history, 'go');
  if (mode === 'root stop') stop(); else trigger.set(1);
  expect(stillUnvisited).toBe(true);
  expect(result).toBe(false); expect(snapshot).toBe('/a');
  expect(guard).not.toHaveBeenCalled(); expect(lateGuard).not.toHaveBeenCalled();
  expect(push).not.toHaveBeenCalled(); expect(replace).not.toHaveBeenCalled(); expect(go).not.toHaveBeenCalled();
  expect(router.path()).toBe('/a'); expect(window.location.pathname).toBe('/b');
  stop(); trigger.dispose(); expect(getGraphTelemetry().nodes).toBe(0);
});
