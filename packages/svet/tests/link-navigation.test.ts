/** @vitest-environment jsdom */
import {beforeEach, expect, it, vi} from 'vitest';
import {Link, RouterView, createRouter, mount, resetReactiveSystem} from '../src/engine';

beforeEach(() => { vi.restoreAllMocks(); resetReactiveSystem(); document.body.innerHTML=''; window.history.replaceState({},'', '/start'); });

function click(anchor: Element, init: MouseEventInit = {}, alreadyPrevented = false) {
  let prevented = false;
  const observe = (event: Event) => { prevented=event.defaultPrevented; event.preventDefault(); };
  document.addEventListener('click',observe);
  const event=new MouseEvent('click',{bubbles:true,cancelable:true,...init});
  if(alreadyPrevented) event.preventDefault();
  anchor.dispatchEvent(event); document.removeEventListener('click',observe); return prevented;
}
function setup(to: string, extra: Partial<Parameters<typeof Link>[0]> = {}) {
  const router=createRouter({initialPath:'/start',routes:[{path:'*',component:()=>Link({to,children:'go',...extra})}]});
  const app=mount(()=>RouterView({router}),document.body), navigate=vi.spyOn(router,'navigate');
  return {router,navigate,anchor:document.querySelector('a')!,stop(){app.unmount();router.dispose();}};
}

it.each([{ctrlKey:true},{metaKey:true},{shiftKey:true},{altKey:true},{button:1},{button:2}])('preserves modified/nonprimary click %j', init => {
  const link=setup('/next'); expect(click(link.anchor,init)).toBe(false); expect(link.navigate).not.toHaveBeenCalled(); link.stop();
});

it.each(['https://external.invalid/page','mailto:test@example.com','tel:+123456'])('preserves external browser navigation for %s', to => {
  const link=setup(to); expect(click(link.anchor)).toBe(false); expect(link.navigate).not.toHaveBeenCalled(); link.stop();
});

it('preserves links without a router context', () => {
  const app=mount(()=>Link({to:'/native',children:'go'}),document.body);
  expect(click(document.querySelector('a')!)).toBe(false); app.unmount();
});

it('does not route clicks already prevented by another handler', () => {
  const link=setup('/next'); click(link.anchor,{},true); expect(link.navigate).not.toHaveBeenCalled(); link.stop();
});

it.each([{target:'_blank'},{target:'frame'},{download:'file.bin'}])('preserves browser target/download behavior %j', extra => {
  const link=setup('/next',extra); expect(click(link.anchor)).toBe(false); expect(link.navigate).not.toHaveBeenCalled(); link.stop();
});

it('normalizes a same-origin absolute link into a router path with query and hash', () => {
  const link=setup(window.location.origin+'/next?q=hello#part'); expect(click(link.anchor)).toBe(true);
  expect(link.navigate).toHaveBeenCalledWith('/next?q=hello#part'); expect(link.router.path()).toBe('/next');
  expect(link.router.query()).toEqual({q:'hello'}); expect(link.router.hash()).toBe('part'); link.stop();
});

it.each(['javascript:alert(1)','vbscript:alert(1)','data:text/html,<script>bad()</script>'])('blocks unsafe URL %s even without router navigation', to => {
  const link=setup(to); expect(link.anchor.getAttribute('href')).toBe('#');
  expect(click(link.anchor)).toBe(true); expect(link.navigate).not.toHaveBeenCalled(); link.stop();
});

it('honors a document base target', () => {
  const base=document.createElement('base'); base.target='_blank'; document.head.append(base);
  try { const link=setup('/next'); expect(click(link.anchor)).toBe(false); expect(link.navigate).not.toHaveBeenCalled(); link.stop(); }
  finally { base.remove(); }
});

it('target _self and download=false still route normally and preserve rel', () => {
  const link=setup('/next',{target:'_self',download:false,rel:'nofollow'});
  expect(link.anchor.hasAttribute('download')).toBe(false); expect(link.anchor.rel).toBe('nofollow');
  expect(click(link.anchor)).toBe(true); expect(link.navigate).toHaveBeenCalledWith('/next'); link.stop();
});

it('resolves relative URLs using the anchor document base URI', () => {
  const base=document.createElement('base'); base.href='/nested/'; document.head.append(base);
  try { const link=setup('next'); click(link.anchor); expect(link.navigate).toHaveBeenCalledWith('/nested/next'); link.stop(); }
  finally { base.remove(); }
});

it('query-only links retain the current pathname', () => {
  const link=setup('?q=next#part'); click(link.anchor);
  expect(link.navigate).toHaveBeenCalledWith('/start?q=next#part'); expect(link.router.path()).toBe('/start'); link.stop();
});


it('routes nested child clicks through the bound anchor currentTarget', () => {
  const span=document.createElement('span'); span.textContent='nested';
  const link=setup('/next',{children:span}); expect(click(span)).toBe(true);
  expect(link.navigate).toHaveBeenCalledWith('/next'); link.stop();
});
