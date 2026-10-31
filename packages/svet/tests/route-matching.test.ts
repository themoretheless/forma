/** @vitest-environment jsdom */
import {beforeEach, expect, it} from 'vitest';
import {RouterView, mount, getGraphTelemetry, createRouter, resetReactiveSystem} from '../src/engine';

beforeEach(()=>resetReactiveSystem());

it.each([
  ['/каталог','/каталог'],['/café','/caf%C3%A9'],['/hello world','/hello%20world'],['/literal%','/literal%25'],
  ['/literal%2Fpart','/literal%2fpart'],['/literal%3Aid','/literal%3aid'],['/literal?query','/literal%3Fquery'],['/literal#hash','/literal%23hash'],
])('matches static pattern %j against normalized URL %j',(path,url)=>{
  const route={path,component:()=>document.createTextNode('route')},router=createRouter({routes:[route],initialPath:'/'});
  expect(router.navigate(url)).toBe(true);expect(router.match(router.path()).route).toBe(route);router.dispose();
});

it.each(['__proto__','constructor','prototype'])('captures parameter %s as an own data property without changing the object prototype',name=>{
  const router=createRouter({routes:[{path:'/:'+name,component:()=>document.createTextNode('route')}],initialPath:'/'});
  router.navigate('/value');const params=router.params();expect(Object.hasOwn(params,name)).toBe(true);expect(params[name]).toBe('value');
  expect(Object.getPrototypeOf(params)).toBe(Object.prototype);expect(Object.keys(params)).toEqual([name]);router.dispose();
});

it('decodes dynamic segments once and does not treat an encoded slash as a path separator',()=>{
  const router=createRouter({routes:[{path:'/a/b',component:()=>document.createTextNode('static')},{path:'/:value',component:()=>document.createTextNode('parameter')}],initialPath:'/'});
  router.navigate('/a%2Fb');expect(router.params()).toEqual({value:'a/b'});
  router.navigate('/%252F');expect(router.params()).toEqual({value:'%2F'});router.dispose();
});

it.each(['%ZZ','%E0%A4'])('preserves malformed encoded parameter %j without throwing',segment=>{
  const router=createRouter({routes:[{path:'/:value',component:()=>document.createTextNode('route')}],initialPath:'/'});
  expect(()=>router.navigate('/'+segment)).not.toThrow();expect(router.params()).toEqual({value:segment});router.dispose();
});

it('an encoded leading colon remains a literal static segment',()=>{
  const route={path:'/%3Aid',component:()=>document.createTextNode('literal')},router=createRouter({routes:[route],initialPath:'/'});
  router.navigate('/:id');expect(router.match(router.path()).route).toBe(route);expect(router.params()).toEqual({});router.dispose();
});

it('static segments are not decoded twice',()=>{
  const route={path:'/%253Aid',component:()=>document.createTextNode('literal')},router=createRouter({routes:[route],initialPath:'/'});
  router.navigate('/%253Aid');expect(router.match(router.path()).route).toBe(route);
  router.navigate('/%3Aid');expect(router.match(router.path()).route).toBeNull();router.dispose();
});

it('RouterView renders Unicode static routes and decoded dynamic parameters',()=>{
  const router=createRouter({routes:[{path:'/товары/:id',component:params=>document.createTextNode('Товар '+params.id)}],initialPath:'/'});
  const host=document.createElement('div');document.body.append(host);const app=mount(()=>RouterView({router}),host);
  router.navigate('/товары/кофе');expect(host.textContent).toBe('Товар кофе');
  app.unmount();host.remove();router.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});
