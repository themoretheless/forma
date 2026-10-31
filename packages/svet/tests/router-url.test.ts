/** @vitest-environment jsdom */
import {beforeEach, expect, it, vi} from 'vitest';
import {createRouter, getGraphTelemetry, resetReactiveSystem} from '../src/engine';

beforeEach(()=>{vi.restoreAllMocks();resetReactiveSystem();window.history.replaceState({},'', '/dir/page?q=old#start');});

it.each([
  ['child','/dir/child'],['../next','/next'],['?q=new','/dir/page?q=new'],['#part','/dir/page?q=old#part'],
  ['./x/../next','/dir/next'],['','/dir/page?q=old'],
])('normalizes navigate(%j) consistently for history, guards and signals', (to,expected)=>{
  const guard=vi.fn(), router=createRouter({routes:[],beforeEach:guard});
  expect(router.navigate(to)).toBe(true);
  const url=new URL(expected,window.location.origin);
  expect(router.path()).toBe(url.pathname);expect(router.query()).toEqual(Object.fromEntries(url.searchParams));expect(router.hash()).toBe(url.hash.slice(1));
  expect(window.location.pathname+window.location.search+window.location.hash).toBe(expected);
  expect(guard.mock.calls[0][0].path).toBe(url.pathname);router.dispose();
});

it('normalizes direct same-origin absolute navigation before matching',()=>{
  const router=createRouter({routes:[{path:'/next/:id',component:()=>document.createTextNode('next')}]});
  expect(router.navigate(window.location.origin+'/next/42?q=value#part')).toBe(true);
  expect(router.path()).toBe('/next/42');expect(router.params()).toEqual({id:'42'});expect(router.query()).toEqual({q:'value'});expect(router.hash()).toBe('part');router.dispose();
});

it.each(['https://external.invalid/path','//external.invalid/path','mailto:test@example.com','javascript:alert(1)','http://['])('rejects noninternal or invalid URL %j before guards/history',to=>{
  const guard=vi.fn(),router=createRouter({routes:[],beforeEach:guard}),push=vi.spyOn(window.history,'pushState');
  expect(()=>expect(router.navigate(to)).toBe(false)).not.toThrow();
  expect(guard).not.toHaveBeenCalled();expect(push).not.toHaveBeenCalled();expect(router.path()).toBe('/dir/page');router.dispose();push.mockRestore();
});

it('relative navigation preserves the current raw query when only hash changes',()=>{
  const router=createRouter({routes:[],initialPath:'/dir/page?tag=one&tag=two&space=hello%20world'});
  router.navigate('#next');expect(window.location.search).toBe('?tag=one&tag=two&space=hello%20world');
  expect(router.query()).toEqual({tag:'two',space:'hello world'});router.dispose();
});

it('a redirect to the already normalized target runs guards once',()=>{
  const guard=vi.fn(()=>'/dir/next'),router=createRouter({routes:[],beforeEach:guard});
  expect(router.navigate('next')).toBe(true);expect(guard).toHaveBeenCalledOnce();expect(router.path()).toBe('/dir/next');router.dispose();
});

it('invalid external guard redirects retain the last full internal URL',()=>{
  const router=createRouter({routes:[]}),remove=router.beforeEach(()=> 'https://external.invalid/path');
  expect(router.navigate('/next')).toBe(false);expect(window.location.pathname).toBe('/dir/page');
  remove();expect(router.navigate('#later')).toBe(true);expect(window.location.pathname+window.location.search+window.location.hash).toBe('/dir/page?q=old#later');router.dispose();
});

it.each(['https://external.invalid/path','http://['])('invalid initial URL %j allocates no graph state',initialPath=>{
  const baseline=getGraphTelemetry().nodes;
  expect(()=>createRouter({routes:[],initialPath})).toThrow('initial router URL');expect(getGraphTelemetry().nodes).toBe(baseline);
});
