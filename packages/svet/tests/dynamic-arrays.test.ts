/** @vitest-environment jsdom */
import {afterEach,beforeEach,expect,it,vi} from 'vitest';
import {getGraphTelemetry,h,mount,resetReactiveSystem,signal,type ChildNodeValue} from '../src/engine';
beforeEach(()=>{resetReactiveSystem();document.body.replaceChildren();});afterEach(()=>vi.restoreAllMocks());

it('flattens mixed nested arrays while preserving nodes and omitting empty values',()=>{
 const node=h('b',null,'B'),value=signal<ChildNodeValue[]>(['A',[null,node,[false,3]],undefined]);
 const app=mount(()=>h('div',null,'left:',value,':right'),document.body);
 expect(document.body.textContent).toBe('left:AB3:right');expect(document.querySelector('b')).toBe(node);
 value.set([]);expect(document.body.textContent).toBe('left::right');expect(node.parentNode).toBeNull();app.unmount();value.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('reorders retained array nodes and transitions through text and fragments',()=>{
 const a=h('b',null,'A'),b=h('i',null,'B'),value=signal<ChildNodeValue>([a,b]),app=mount(()=>h('div',null,value),document.body);
 value.set([b,a]);expect(document.body.textContent).toBe('BA');expect(document.querySelector('b')).toBe(a);expect(document.querySelector('i')).toBe(b);
 const fragment=document.createDocumentFragment();fragment.append(h('u',null,'F'));value.set(fragment);expect(document.body.textContent).toBe('F');value.set('text');expect(document.body.textContent).toBe('text');value.set([a]);expect(document.body.textContent).toBe('A');
 app.unmount();value.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('reevaluates an array identity when another tracked dependency changes',()=>{
 const values:ChildNodeValue[]=['old'],tick=signal(0),app=mount(()=>h('div',null,()=>{tick();return values;}),document.body);
 values[0]='new';tick.set(1);expect(document.body.textContent).toBe('new');app.unmount();tick.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('nested accessors in arrays remain reactive and dispose with their parent',()=>{
 const text=signal('old'),value=signal<ChildNodeValue[]>(['before:',text]),app=mount(()=>h('div',null,value),document.body);
 expect(document.body.textContent).toBe('before:old');text.set('new');expect(document.body.textContent).toBe('before:new');
 value.set(['replaced']);const baseline=getGraphTelemetry().nodes;text.set('detached');expect(document.body.textContent).toBe('replaced');expect(getGraphTelemetry().nodes).toBe(baseline);
 app.unmount();text.dispose();value.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('cyclic arrays and repeated native nodes fail before changing committed DOM',()=>{
 const value=signal<ChildNodeValue>('safe'),app=mount(()=>h('div',null,value),document.body),cycle:ChildNodeValue[]=[];cycle.push(cycle);
 expect(()=>value.set(cycle)).toThrow('Cyclic dynamic child array');expect(document.body.textContent).toBe('safe');
 const node=h('b',null,'duplicate');expect(()=>value.set([node,node])).toThrow('Duplicate dynamic child node');expect(document.body.textContent).toBe('safe');expect(node.parentNode).toBeNull();
 app.unmount();value.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('failed array moves restore old DOM identities and can be retried',()=>{
 const a=h('b',null,'A'),b=h('i',null,'B'),value=signal<ChildNodeValue[]>([a,b]),retry=signal(0),app=mount(()=>h('div',null,()=>{retry();return value();}),document.body),host=document.querySelector('div')!,native=host.insertBefore.bind(host);let count=0;
 const insert=vi.spyOn(host,'insertBefore').mockImplementation(function<T extends Node>(node:T,anchor:Node|null):T{const result=native(node,anchor);if(++count===2)throw Error('move failed');return result;});
 expect(()=>value.set([b,a])).toThrow('move failed');expect(host.textContent).toBe('AB');expect(host.querySelector('b')).toBe(a);expect(host.querySelector('i')).toBe(b);
 insert.mockRestore();retry.set(1);expect(host.textContent).toBe('BA');app.unmount();value.dispose();retry.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('deep nested arrays normalize without consuming the JavaScript call stack',()=>{
 let values:ChildNodeValue='deep';for(let i=0;i<12000;i++)values=[values];const value=signal(values),app=mount(()=>h('div',null,value),document.body);
 expect(document.body.textContent).toBe('deep');app.unmount();value.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});
