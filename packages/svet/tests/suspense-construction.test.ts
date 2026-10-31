/** @vitest-environment jsdom */
import {beforeEach, expect, it, vi} from 'vitest';
import {Suspense, createRoot, mount, createResource, signal, getGraphTelemetry, resetReactiveSystem} from '../src/engine';

beforeEach(()=>{vi.restoreAllMocks();resetReactiveSystem();document.body.innerHTML='';});

it('does not construct fallback after content disposes its owner',()=>{
  let fallbacks=0;
  createRoot(stop=>{
    expect(()=>Suspense({children:()=>{stop();return document.createTextNode('stale');},fallback:()=>{fallbacks++;return document.createTextNode('fallback');}})).toThrow('owner');
  });expect(fallbacks).toBe(0);expect(getGraphTelemetry().nodes).toBe(0);
});

it('rejects reset during content construction without creating fallback or harming replacement state',()=>{
  let fresh!:ReturnType<typeof signal<number>>,fallbacks=0;
  expect(()=>createRoot(()=>Suspense({children:()=>{resetReactiveSystem();fresh=signal(10);return document.createTextNode('stale');},fallback:()=>{fallbacks++;return document.createTextNode('fallback');}}))).toThrow('reset');
  expect(fallbacks).toBe(0);expect(getGraphTelemetry().nodes).toBe(1);fresh.set(11);expect(fresh()).toBe(11);fresh.dispose();
});

it('reads content and fallback descriptors once',()=>{
  let contentReads=0,fallbackReads=0;
  const app=mount(()=>Suspense({get children(){contentReads++;return ()=>document.createTextNode('content');},get fallback(){fallbackReads++;return ()=>document.createTextNode('fallback');}}),document.body);
  expect([contentReads,fallbackReads]).toEqual([1,1]);app.unmount();expect(getGraphTelemetry().nodes).toBe(0);
});

it('a fallback getter that disposes the owner cannot invoke its returned callback',()=>{
  let callbacks=0;
  createRoot(stop=>{
    expect(()=>Suspense({children:'content',get fallback(){stop();return ()=>{callbacks++;return document.createTextNode('stale');};}})).toThrow('owner');
  });expect(callbacks).toBe(0);
});

it('removes wrappers attached by a fragment hook after owner cleanup',()=>{
  const append=DocumentFragment.prototype.appendChild;
  createRoot(stop=>{
    vi.spyOn(DocumentFragment.prototype,'appendChild').mockImplementation(function<T extends Node>(this:DocumentFragment,node:T):T{
      if(node instanceof Element&&node.hasAttribute('data-svet-suspense-content')){stop();document.body.appendChild(node);return node;}
      return append.call(this,node) as T;
    });
    expect(()=>Suspense({children:'content',fallback:'waiting'})).toThrow('owner');
  });expect(document.body.childNodes).toHaveLength(0);expect(getGraphTelemetry().nodes).toBe(0);
});

it('a display observer failure settles loading and allows a later resource retry',async()=>{
  let resource!:ReturnType<typeof createResource<string,boolean>>,calls=0;
  const app=mount(()=>Suspense({children:()=>{resource=createResource(()=>false,async()=>{calls++;return 'ready';});return document.createTextNode('content');},fallback:'waiting'}),document.body);
  const content=document.querySelector<HTMLElement>('[data-svet-suspense-content]')!;
  const stylePrototype=Object.getPrototypeOf(content.style) as CSSStyleDeclaration;
  const descriptor=Object.getOwnPropertyDescriptor(stylePrototype,'display')!;let armed=true;
  vi.spyOn(stylePrototype,'display','set').mockImplementation(function(this:CSSStyleDeclaration,value:string){if(this===content.style&&armed&&value==='none')throw new Error('display failed');descriptor.set!.call(this,value);});
  expect(await resource.refetch()).toBeUndefined();expect(resource.error()?.message).toBe('display failed');expect(resource.loading()).toBe(false);expect(calls).toBe(0);
  armed=false;expect(await resource.refetch()).toBe('ready');expect(content.style.display).toBe('contents');expect(calls).toBe(1);
  app.unmount();expect(getGraphTelemetry().nodes).toBe(0);
});
