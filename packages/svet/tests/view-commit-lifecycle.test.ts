/** @vitest-environment jsdom */
import {beforeEach, expect, it, vi} from 'vitest';
import {mount, RouterView, ErrorBoundary, createRouter, createGpuTensor, effect, signal, getGraphTelemetry, resetReactiveSystem} from '../src/engine';

beforeEach(()=>{vi.restoreAllMocks();resetReactiveSystem();document.body.innerHTML='';window.history.replaceState({},'', '/');});
function pair(){const fragment=document.createDocumentFragment();fragment.append(document.createTextNode('first'),document.createTextNode('second'));return fragment;}

it('RouterView removes partial DOM insertion after a route failure and recovers on the next navigation',()=>{
  const source=signal(0);let runs=0;
  const router=createRouter({initialPath:'/safe',routes:[{path:'/safe',component:()=>document.createTextNode('safe')},{path:'/bad',component:()=>{createGpuTensor([1]);effect(()=>{source();runs++;});return pair();}}]});
  const host=document.createElement('div');document.body.append(host);const app=mount(()=>RouterView({router}),host),baseline=getGraphTelemetry().nodes;
  const insert=host.insertBefore.bind(host);
  vi.spyOn(host,'insertBefore').mockImplementation(function<T extends Node>(node:T,child:Node|null):T{if(node.textContent==='second')throw new Error('insert failed');return insert(node,child);});
  expect(()=>router.navigate('/bad')).toThrow('insert failed');expect(host.textContent).toBe('');expect(getGraphTelemetry().nodes).toBe(baseline);
  source.set(1);expect(runs).toBe(1);router.navigate('/safe');expect(host.textContent).toBe('safe');
  app.unmount();router.dispose();source.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('ErrorBoundary removes partial content before mounting its fallback',()=>{
  const insert=Node.prototype.insertBefore;
  vi.spyOn(Node.prototype,'insertBefore').mockImplementation(function<T extends Node>(this:Node,node:T,child:Node|null):T{if(node.textContent==='second')throw new Error('insert failed');return insert.call(this,node,child) as T;});
  const app=mount(()=>ErrorBoundary({children:pair,fallback:()=>document.createTextNode('fallback')}),document.body);
  expect(document.body.textContent).toBe('fallback');app.unmount();expect(getGraphTelemetry().nodes).toBe(0);
});

it('RouterView rejects component output after its owner was unmounted during rendering',()=>{
  let app!:ReturnType<typeof mount>;
  const router=createRouter({initialPath:'/safe',routes:[{path:'/safe',component:()=>document.createTextNode('safe')},{path:'/bad',component:()=>{app.unmount();return document.createTextNode('stale');}}]});
  const host=document.createElement('div');document.body.append(host);app=mount(()=>RouterView({router}),host);
  expect(()=>router.navigate('/bad')).toThrow('owner');expect(host.childNodes).toHaveLength(0);router.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('RouterView removes nodes inserted by a hook after owner cleanup already ran',()=>{
  const router=createRouter({initialPath:'/safe',routes:[{path:'/safe',component:()=>document.createTextNode('safe')},{path:'/next',component:()=>document.createTextNode('late')}]});
  const host=document.createElement('div');document.body.append(host);const app=mount(()=>RouterView({router}),host);
  vi.spyOn(host,'insertBefore').mockImplementation(function<T extends Node>(node:T):T{app.unmount();host.appendChild(node);return node;});
  expect(()=>router.navigate('/next')).toThrow('owner');expect(host.childNodes).toHaveLength(0);router.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('RouterView rejects output after graph reset without disposing replacement state',()=>{
  let fresh!:ReturnType<typeof signal<number>>;
  const router=createRouter({initialPath:'/safe',routes:[{path:'/safe',component:()=>document.createTextNode('safe')},{path:'/reset',component:()=>{resetReactiveSystem();fresh=signal(10);return document.createTextNode('stale');}}]});
  const host=document.createElement('div');document.body.append(host);const app=mount(()=>RouterView({router}),host);
  expect(()=>router.navigate('/reset')).toThrow('reset');expect(host.textContent).toBe('');expect(getGraphTelemetry().nodes).toBe(1);
  app.unmount();router.dispose();fresh.set(11);expect(fresh()).toBe(11);fresh.dispose();
});

it('ErrorBoundary does not invoke fallback user code after its graph was reset during rendering',()=>{
  let fresh!:ReturnType<typeof signal<number>>,fallbacks=0;
  expect(()=>mount(()=>ErrorBoundary({children:()=>{resetReactiveSystem();fresh=signal(10);return document.createTextNode('stale');},fallback:()=>{fallbacks++;return document.createTextNode('fallback');}}),document.body)).toThrow('reset');
  expect(fallbacks).toBe(0);expect(document.body.childNodes).toHaveLength(0);expect(getGraphTelemetry().nodes).toBe(1);fresh.dispose();
});
