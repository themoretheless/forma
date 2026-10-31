/** @vitest-environment jsdom */
import {beforeEach, expect, it, vi} from 'vitest';
import {Portal, createRoot, getCurrentOwner, createGpuTensor, effect, signal, mount, createContext, useContext, getGraphTelemetry, resetReactiveSystem} from '../src/engine';

beforeEach(() => {vi.restoreAllMocks(); resetReactiveSystem(); document.body.innerHTML='';});

it('failed portal content releases partial resources while its caller survives', () => {
  const source=signal(0); let runs=0;
  createRoot(stop => {
    const owner=getCurrentOwner()!, baseline=getGraphTelemetry().nodes;
    expect(() => Portal({children:()=>{createGpuTensor([1]);effect(()=>{source();runs++;});throw new Error('content failed');}})).toThrow('content failed');
    expect(getGraphTelemetry().nodes).toBe(baseline); expect(owner.children?.size??0).toBe(0);
    source.set(1); expect(runs).toBe(1); stop();
  }); source.dispose();
});

it('partial target insertion is removed when append throws', () => {
  const target=document.createElement('div'), append=target.appendChild.bind(target);
  vi.spyOn(target,'appendChild').mockImplementation(node=>{append(node);throw new Error('append failed');});
  createRoot(stop => {
    expect(() => Portal({mount:target,children:()=>{createGpuTensor([1]);return document.createTextNode('content');}})).toThrow('append failed');
    expect(target.childNodes).toHaveLength(0); expect(getGraphTelemetry().nodes).toBe(0); stop();
  });
});

it('owner disposal during content construction cannot append an orphan portal', () => {
  const target=document.createElement('div');
  createRoot(stop => {
    expect(() => Portal({mount:target,children:()=>{stop();return document.createTextNode('content');}})).toThrow('owner');
    expect(target.childNodes).toHaveLength(0);
  });
});

it('owner disposal during target insertion removes the inserted portal', () => {
  const target=document.createElement('div'), append=target.appendChild.bind(target);
  createRoot(stop => {
    vi.spyOn(target,'appendChild').mockImplementation(node=>{append(node);stop();return node;});
    expect(() => Portal({mount:target,children:document.createTextNode('content')})).toThrow('owner');
    expect(target.childNodes).toHaveLength(0);
  });
});

it('reset during content construction cannot append stale content or dispose fresh state', () => {
  const target=document.createElement('div'); let fresh!:ReturnType<typeof signal<number>>;
  createRoot(() => {
    expect(() => Portal({mount:target,children:()=>{resetReactiveSystem();fresh=signal(10);return document.createTextNode('stale');}})).toThrow('reset');
  });
  expect(target.childNodes).toHaveLength(0); expect(getGraphTelemetry().nodes).toBe(1);
  fresh.set(11); expect(fresh()).toBe(11); fresh.dispose();
});

it('successful portals inherit context and release external DOM and subscriptions on unmount', () => {
  const context=createContext('default'), source=signal(0), target=document.createElement('div'); document.body.append(target);
  const host=document.createElement('div'); document.body.append(host); let runs=0;
  const app=mount(()=>context.Provider({value:'inherited',children:()=>Portal({mount:target,children:()=>{
    createGpuTensor([1]);effect(()=>{source();runs++;});return document.createTextNode(useContext(context));
  }})}),host);
  expect(target.textContent).toBe('inherited'); app.unmount(); source.set(1);
  expect(target.childNodes).toHaveLength(0); expect(runs).toBe(1); expect(getGraphTelemetry().nodes).toBe(1); source.dispose();
});

it('invalidating the owner in a mount getter stops before reading child descriptors', () => {
  const target=document.createElement('div'); let reads=0;
  createRoot(stop => {
    expect(()=>Portal({get mount(){stop();return target;},get children(){reads++;return document.createTextNode('content');}})).toThrow('owner');
  });
  expect(reads).toBe(0); expect(target.childNodes).toHaveLength(0);
});

it('nested portal containers and resources are removed when outer construction fails', () => {
  const target=document.createElement('div');
  createRoot(stop => {
    expect(()=>Portal({mount:target,children:()=>{
      Portal({mount:target,children:()=>{createGpuTensor([1]);return document.createTextNode('nested');}});
      throw new Error('outer failed');
    }})).toThrow('outer failed');
    expect(target.childNodes).toHaveLength(0); expect(getGraphTelemetry().nodes).toBe(0); stop();
  });
});

it('a disposed owner rejects portal construction without running children', () => {
  createRoot(stop=>{stop();let called=false;
    expect(()=>Portal({children:()=>{called=true;return document.createTextNode('content');}})).toThrow('disposed owner');
    expect(called).toBe(false);
  });
});

it('portal child descriptors are read once', () => {
  let reads=0; const target=document.createElement('div');
  createRoot(stop=>{
    Portal({mount:target,get children(){reads++;return document.createTextNode('content');}});
    expect(reads).toBe(1); expect(target.textContent).toBe('content'); stop();
  });
  expect(target.childNodes).toHaveLength(0);
});

it('an insertion hook that disposes the owner before attaching cannot leave a container behind', () => {
  const target=document.createElement('div'), append=target.appendChild.bind(target);
  createRoot(stop=>{
    vi.spyOn(target,'appendChild').mockImplementation(node=>{stop();append(node);return node;});
    expect(()=>Portal({mount:target,children:document.createTextNode('late insertion')})).toThrow('owner');
    expect(target.childNodes).toHaveLength(0);
  });
});
