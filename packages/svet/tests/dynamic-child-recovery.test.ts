/** @vitest-environment jsdom */
import {afterEach,beforeEach,expect,it,vi} from 'vitest';
import {getGraphTelemetry,h,mount,resetReactiveSystem,signal} from '../src/engine';
beforeEach(()=>{resetReactiveSystem();document.body.replaceChildren();});afterEach(()=>vi.restoreAllMocks());
function group(...nodes:Node[]){const fragment=document.createDocumentFragment();fragment.append(...nodes);return fragment;}

it('partial fragment insertion restores the old range and preserves the candidate for retry',()=>{
 const old=group(h('b',null,'old A'),h('i',null,'old B')),next=group(h('u',null,'new A'),h('em',null,'new B')),value=signal<Node>(old),retry=signal(0);
 const app=mount(()=>h('div',null,()=>{retry();return value();}),document.body),host=document.querySelector('div')!,native=host.insertBefore.bind(host);let count=0;
 const insert=vi.spyOn(host,'insertBefore').mockImplementation(function<T extends Node>(node:T,anchor:Node|null):T{const result=native(node,anchor);if(++count===2)throw Error('partial insertion');return result;});
 expect(()=>value.set(next)).toThrow('partial insertion');expect(host.textContent).toBe('old Aold B');expect(next.textContent).toBe('new Anew B');
 insert.mockRestore();retry.set(1);expect(host.textContent).toBe('new Anew B');app.unmount();value.dispose();retry.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('partial old-range removal rolls back without losing either fragment',()=>{
 const a=h('b',null,'old A'),b=h('i',null,'old B'),next=group(h('u',null,'new')),value=signal<Node>(group(a,b)),retry=signal(0);
 const app=mount(()=>h('div',null,()=>{retry();return value();}),document.body),host=document.querySelector('div')!,native=host.removeChild.bind(host);let failed=false;
 const remove=vi.spyOn(host,'removeChild').mockImplementation(function<T extends Node>(node:T):T{if(node===b&&!failed){failed=true;throw Error('remove failed');}return native(node);});
 expect(()=>value.set(next)).toThrow('remove failed');expect(host.textContent).toBe('old Aold B');expect(host.querySelector('b')).toBe(a);expect(host.querySelector('i')).toBe(b);expect(next.textContent).toBe('new');
 remove.mockRestore();retry.set(1);expect(host.textContent).toBe('new');app.unmount();value.dispose();retry.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('unmount from the accessor rejects its returned fragment',()=>{
 const update=signal(false);let app!:ReturnType<typeof mount>;const candidate=group(h('b',null,'stale'));
 app=mount(()=>h('div',null,()=>{if(update()){app.unmount();return candidate;}return 'old';}),document.body);
 expect(()=>update.set(true)).toThrow('disposed');expect(document.body.textContent).toBe('');expect(candidate.textContent).toBe('stale');update.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('reset from the accessor leaves new graph nodes valid and inserts no stale content',()=>{
 const update=signal(false);let fresh!:ReturnType<typeof signal<number>>;
 const app=mount(()=>h('div',null,()=>{if(update()){resetReactiveSystem();fresh=signal(7);return group(h('b',null,'stale'));}return 'old';}),document.body);
 expect(()=>update.set(true)).toThrow('reset');expect(document.body.textContent).toBe('old');expect(getGraphTelemetry().nodes).toBe(1);app.unmount();fresh.set(8);expect(fresh()).toBe(8);fresh.dispose();
});

it('late insertion after unmount removes provisional nodes and never restores the old view',()=>{
 const value=signal<Node|string>('old'),candidate=group(h('b',null,'stale A'),h('i',null,'stale B')),app=mount(()=>h('div',null,value),document.body),host=document.querySelector('div')!,native=host.insertBefore.bind(host);
 let count=0;
 const insert=vi.spyOn(host,'insertBefore').mockImplementation(function<T extends Node>(node:T,anchor:Node|null):T{if(++count===3)app.unmount();return native(node,anchor);});
 expect(()=>value.set(candidate)).toThrow('disposed');expect(document.body.textContent).toBe('');expect(host.querySelector('b')).toBeNull();expect(host.querySelector('i')).toBeNull();
 insert.mockRestore();value.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('unmount during rollback cannot restore late old nodes into the detached host',()=>{
 const old=group(h('b',null,'old')),next=group(h('i',null,'new')),value=signal<Node>(old),app=mount(()=>h('div',null,value),document.body),host=document.querySelector('div')!,native=host.insertBefore.bind(host);let count=0;
 const insert=vi.spyOn(host,'insertBefore').mockImplementation(function<T extends Node>(node:T,anchor:Node|null):T{
   if(++count===2){native(node,anchor);throw Error('insert failed');}
   if(count===3)app.unmount();return native(node,anchor);
 });
 expect(()=>value.set(next)).toThrow('insert failed');expect(document.body.textContent).toBe('');expect(host.querySelector('b')).toBeNull();expect(host.querySelector('i')).toBeNull();
 insert.mockRestore();value.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});
