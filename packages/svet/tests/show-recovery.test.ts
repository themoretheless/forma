/** @vitest-environment jsdom */
import {beforeEach, expect, it, vi} from 'vitest';
import {Show, mount, signal, createGpuTensor, getGraphTelemetry, resetReactiveSystem} from '../src/engine';

beforeEach(()=>{vi.restoreAllMocks();resetReactiveSystem();document.body.innerHTML='';});

it('a failed truthy branch retries on the next truthy value instead of caching success',()=>{
  const condition=signal<number|false>(false);let fail=true,calls=0;
  const app=mount(()=>Show({when:condition,children:()=>{calls++;createGpuTensor([1]);if(fail)throw new Error('branch failed');return document.createTextNode('ready');}}),document.body);
  expect(()=>condition.set(1)).toThrow('branch failed');fail=false;condition.set(2);
  expect(document.body.textContent).toBe('ready');expect(calls).toBe(2);app.unmount();condition.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('a failed fallback retries on the next falsy value',()=>{
  const condition=signal<number|null>(1);let fail=true,calls=0;
  const app=mount(()=>Show({when:condition,children:'ready',fallback:()=>{calls++;if(fail)throw new Error('fallback failed');return document.createTextNode('empty');}}),document.body);
  expect(()=>condition.set(0)).toThrow('fallback failed');fail=false;condition.set(null);
  expect(document.body.textContent).toBe('empty');expect(calls).toBe(2);app.unmount();condition.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('partial branch insertion is rolled back before a same-truthiness retry',()=>{
  const condition=signal<number|false>(false);let reject=true;
  const app=mount(()=>Show({when:condition,children:()=>{const fragment=document.createDocumentFragment();fragment.append(document.createTextNode('first'),document.createTextNode('second'));return fragment;}}),document.body);
  const insert=document.body.insertBefore.bind(document.body);
  vi.spyOn(document.body,'insertBefore').mockImplementation(function<T extends Node>(node:T,child:Node|null):T{if(reject&&node.textContent==='second')throw new Error('insert failed');return insert(node,child);});
  expect(()=>condition.set(1)).toThrow('insert failed');expect(document.body.textContent).toBe('');reject=false;condition.set(2);
  expect(document.body.textContent).toBe('firstsecond');app.unmount();condition.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('output after owner unmount during rendering is rejected without leaving DOM',()=>{
  const condition=signal(false);let app!:ReturnType<typeof mount>;
  app=mount(()=>Show({when:condition,children:()=>{app.unmount();return document.createTextNode('stale');}}),document.body);
  expect(()=>condition.set(true)).toThrow('owner');expect(document.body.childNodes).toHaveLength(0);condition.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('successful non-keyed truthy branches retain their DOM identity',()=>{
  const condition=signal(1);let calls=0;
  const app=mount(()=>Show({when:condition,children:()=>{calls++;return document.createTextNode('stable');}}),document.body);
  const node=document.body.firstChild;condition.set(2);expect(document.body.firstChild).toBe(node);expect(calls).toBe(1);
  app.unmount();condition.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('reactive when getters stay tracked and are read once per invocation',()=>{
  const condition=signal(1);let reads=0;
  const app=mount(()=>Show({get when(){reads++;return condition();},children:value=>document.createTextNode(String(value))}),document.body);
  expect(reads).toBe(1);condition.set(2);expect(reads).toBe(2);expect(document.body.textContent).toBe('2');
  app.unmount();condition.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('reset during branch rendering rejects stale output and preserves replacement graph state',()=>{
  const condition=signal(false);let fresh!:ReturnType<typeof signal<number>>;
  const app=mount(()=>Show({when:condition,children:()=>{resetReactiveSystem();fresh=signal(10);return document.createTextNode('stale');}}),document.body);
  expect(()=>condition.set(true)).toThrow('reset');expect(document.body.textContent).toBe('');expect(getGraphTelemetry().nodes).toBe(1);
  app.unmount();condition.dispose();fresh.set(11);expect(fresh()).toBe(11);fresh.dispose();
});
