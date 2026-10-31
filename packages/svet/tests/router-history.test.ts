/** @vitest-environment jsdom */
import {afterEach, beforeEach, expect, it, vi} from 'vitest';
import {createRouter, resetReactiveSystem, type RouterInstance} from '../src/engine';

let router: RouterInstance | undefined;
beforeEach(()=>{resetReactiveSystem();window.history.replaceState({host:'kept'},'', '/start?q=one#top');});
afterEach(()=>{router?.dispose();router=undefined;vi.restoreAllMocks();});
const url=()=>window.location.pathname+window.location.search+window.location.hash;
const waitFor=(predicate:()=>boolean)=>vi.waitFor(()=>expect(predicate()).toBe(true),{timeout:1500,interval:5});

it('a denied Back restores the accepted entry and preserves the forward stack',async()=>{
  router=createRouter({routes:[]});router.navigate('/middle');router.navigate('/end?q=two#last');
  const length=history.length, guard=vi.fn(()=>false),remove=router.beforeEach(guard);
  history.back();await waitFor(()=>guard.mock.calls.length===1 && url()==='/end?q=two#last');
  expect(router.path()).toBe('/end');expect(history.length).toBe(length);
  remove();history.back();await waitFor(()=>router!.path()==='/middle');
  history.forward();await waitFor(()=>router!.path()==='/end');
  expect(url()).toBe('/end?q=two#last');expect(guard).toHaveBeenCalledOnce();
});

it('a denied Forward restores the accepted entry without running guards on compensation',async()=>{
  router=createRouter({routes:[]});router.navigate('/middle');router.navigate('/end');
  history.back();await waitFor(()=>router!.path()==='/middle');
  const guard=vi.fn(()=>false),remove=router.beforeEach(guard);
  history.forward();await waitFor(()=>guard.mock.calls.length===1 && url()==='/middle');
  expect(router.path()).toBe('/middle');expect(guard).toHaveBeenCalledOnce();
  remove();history.forward();await waitFor(()=>router!.path()==='/end');
});

it('denied multi-entry traversal restores the full distance',async()=>{
  router=createRouter({routes:[]});router.navigate('/one');router.navigate('/two');router.navigate('/three');
  const guard=vi.fn(()=>false);router.beforeEach(guard);
  history.go(-3);await waitFor(()=>guard.mock.calls.length===1 && url()==='/three');
  expect(router.path()).toBe('/three');expect(guard).toHaveBeenCalledOnce();
});

it('a popstate redirect replaces the visited entry and retains its neighbors',async()=>{
  router=createRouter({routes:[]});router.navigate('/middle');router.navigate('/end');
  const length=history.length;
  const remove=router.beforeEach(to=>to.path==='/middle'?'/login?next=middle#gate':undefined);
  history.back();await waitFor(()=>router!.path()==='/login');
  expect(url()).toBe('/login?next=middle#gate');expect(history.length).toBe(length);
  remove();history.forward();await waitFor(()=>router!.path()==='/end');
  history.back();await waitFor(()=>router!.path()==='/login');expect(url()).toBe('/login?next=middle#gate');
});

it('a reentrant navigation during popstate wins without compensating over its commit',async()=>{
  router=createRouter({routes:[]});router.navigate('/middle');router.navigate('/end');
  const removeReentrant=router.beforeEach(to=>{if(to.path==='/middle')router!.navigate('/newer');});
  history.back();await waitFor(()=>router!.path()==='/newer');
  expect(url()).toBe('/newer');
  // The new push branches from the actually visited entry, replacing /end.
  removeReentrant();
  const deny=vi.fn(()=>false),remove=router.beforeEach(deny);
  history.back();await waitFor(()=>deny.mock.calls.length>0 && url()==='/newer');remove();
});

it('initial host history state survives stamping and a round trip',async()=>{
  router=createRouter({routes:[]});router.navigate('/next');history.back();
  await waitFor(()=>router!.path()==='/start');expect(history.state.host).toBe('kept');
  expect(url()).toBe('/start?q=one#top');
});

it('an unknown external entry falls back to restoring the accepted URL without adding history',async()=>{
  router=createRouter({routes:[]});router.navigate('/accepted');
  history.pushState({external:7},'', '/external');
  history.pushState({},'', '/tail');
  const length=history.length,guard=vi.fn(()=>false);router.beforeEach(guard);
  history.back();await waitFor(()=>guard.mock.calls.length===1 && url()==='/accepted');
  expect(router.path()).toBe('/accepted');expect(history.length).toBe(length);expect(history.state.external).toBe(7);
});

it('navigate refuses to push while a compensating traversal is pending',()=>{
  router=createRouter({routes:[]});router.navigate('/middle');const middle=history.state;
  router.navigate('/end');const end=history.state,length=history.length;
  const remove=router.beforeEach(()=>false),go=vi.spyOn(history,'go').mockImplementation(()=>{});
  history.replaceState(middle,'','/middle');window.dispatchEvent(new PopStateEvent('popstate',{state:middle}));
  expect(go).toHaveBeenCalledWith(1);expect(router.navigate('/blocked')).toBe(false);
  expect(history.length).toBe(length);expect(router.path()).toBe('/end');
  history.replaceState(end,'','/end');window.dispatchEvent(new PopStateEvent('popstate',{state:end}));
  remove();expect(router.navigate('/ready')).toBe(true);expect(url()).toBe('/ready');
});

it('a throwing pop guard compensates before reporting its original failure',async()=>{
  router=createRouter({routes:[]});router.navigate('/middle');router.navigate('/end');
  const failure=new Error('blocked by guard'),observed:unknown[]=[];
  const onError=(event:ErrorEvent)=>{observed.push(event.error);event.preventDefault();};
  window.addEventListener('error',onError);
  try {
    router.beforeEach(()=>{throw failure;});history.back();
    await waitFor(()=>observed.length===1 && url()==='/end');
    expect(observed[0]).toBe(failure);expect(router.path()).toBe('/end');
  } finally {window.removeEventListener('error',onError);}
});

it('primitive host state remains available through its reserved envelope',async()=>{
  history.replaceState(42,'','/primitive');router=createRouter({routes:[]});router.navigate('/next');
  history.back();await waitFor(()=>router!.path()==='/primitive');
  expect(history.state.__svet_router_state__).toBe(42);
});

it('external history writes start a new tracked segment rather than inventing a distance',async()=>{
  router=createRouter({routes:[]});router.navigate('/old');
  history.pushState({external:true},'', '/foreign');router.navigate('/new');
  const guard=vi.fn(()=>false);router.beforeEach(guard);
  history.back();await waitFor(()=>guard.mock.calls.length===1 && url()==='/new');
  expect(router.path()).toBe('/new');expect(guard).toHaveBeenCalledOnce();
});

it('native compensation failure does not replace the original throwing guard error',()=>{
  router=createRouter({routes:[]});router.navigate('/middle');const middle=history.state;router.navigate('/end');
  const primary=new Error('guard failed'),observed:unknown[]=[];
  const onError=(event:ErrorEvent)=>{observed.push(event.error);event.preventDefault();};
  window.addEventListener('error',onError);
  try {
    router.beforeEach(()=>{throw primary;});
    vi.spyOn(history,'go').mockImplementation(()=>{throw Error('native go failed');});
    history.replaceState(middle,'','/middle');window.dispatchEvent(new PopStateEvent('popstate',{state:middle}));
    expect(observed).toEqual([primary]);expect(router.path()).toBe('/end');
  } finally {window.removeEventListener('error',onError);}
});

// A settled URL alone can be an intermediate compensation landing. Observe a
// quiet event interval before checking the final position and admission state.
function settleHistory(action:()=>void):Promise<void>{
  return new Promise((resolve,reject)=>{
    let quiet:ReturnType<typeof setTimeout>;
    const cleanup=()=>{clearTimeout(quiet);clearTimeout(limit);window.removeEventListener('popstate',changed);};
    const changed=()=>{clearTimeout(quiet);quiet=setTimeout(()=>{cleanup();resolve();},40);};
    const limit=setTimeout(()=>{cleanup();reject(Error('history did not settle'));},1500);
    window.addEventListener('popstate',changed);changed();action();
  });
}

it.each([
  [-1,-1],[-1,-2],[-2,-1],[-1,1],[-2,1,-1],[-1,-1,1,1],
].map(deltas=>({deltas})))('settles traversal burst $deltas at the accepted middle entry',async({deltas})=>{
  router=createRouter({routes:[]});for(let i=1;i<=5;i++)router.navigate('/burst/'+i);
  history.go(-2);await waitFor(()=>router!.path()==='/burst/3');
  const length=history.length,remove=router.beforeEach(()=>false);
  await settleHistory(()=>{for(const delta of deltas)history.go(delta);});
  expect(url()).toBe('/burst/3');expect(router.path()).toBe('/burst/3');expect(history.length).toBe(length);
  remove();expect(router.navigate('/ready')).toBe(true);expect(url()).toBe('/ready');
});
