/** @vitest-environment jsdom */
import {beforeEach,expect,it,vi} from 'vitest';
import {For,Show,mount,signal,effect,createGpuTensor,getDomMutationCount,resetDomMutationCount,getGraphTelemetry,resetReactiveSystem} from '../src/engine';

beforeEach(()=>{vi.restoreAllMocks();resetReactiveSystem();document.body.innerHTML='';});
function pair(value:number){const fragment=document.createDocumentFragment();for(const suffix of ['a','b']){const node=document.createElement('span');node.textContent=value+suffix;fragment.append(node);}return fragment;}

it('fragment rows move and delete every node while preserving keyed identity',()=>{
  const items=signal([1,2]);const app=mount(()=>For({each:items,children:value=>{createGpuTensor([value]);return pair(value);}}),document.body);
  const first=Array.from(document.querySelectorAll('span')).slice(0,2);
  expect(()=>items.set([2,1,3])).not.toThrow();expect(document.body.textContent).toBe('2a2b1a1b3a3b');expect(Array.from(document.querySelectorAll('span')).slice(2,4)).toEqual(first);
  items.set([3]);expect(document.body.textContent).toBe('3a3b');app.unmount();items.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('empty fragment rows retain a movable keyed position',()=>{
  const items=signal([1,2]);const app=mount(()=>For({each:items,children:value=>value===1?document.createDocumentFragment():pair(value)}),document.body);
  const spans=Array.from(document.querySelectorAll('span'));expect(()=>items.set([2,1])).not.toThrow();expect(Array.from(document.querySelectorAll('span'))).toEqual(spans);
  items.set([]);expect(document.body.textContent).toBe('');app.unmount();items.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('fragment ranges include nodes added later by nested Show',()=>{
  const items=signal([1,2]),visible=signal(false);let renders=0;
  const app=mount(()=>For({each:items,children:value=>{renders++;return Show({when:visible,children:()=>document.createTextNode(String(value))});}}),document.body);
  visible.set(true);expect(document.body.textContent).toBe('12');items.set([2,1]);expect(document.body.textContent).toBe('21');expect(renders).toBe(2);
  visible.set(false);visible.set(true);expect(document.body.textContent).toBe('21');items.set([1]);expect(document.body.textContent).toBe('1');
  app.unmount();items.dispose();visible.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('partial insertion of a new fragment row is removed on failure',()=>{
  const items=signal([1]);let fail=true;
  const app=mount(()=>For({each:items,children:value=>{createGpuTensor([value]);return pair(value);}}),document.body),baseline=getGraphTelemetry().nodes;
  const nodes=Array.from(document.querySelectorAll('span')),insert=document.body.insertBefore.bind(document.body);
  vi.spyOn(document.body,'insertBefore').mockImplementation(function<T extends Node>(node:T,anchor:Node|null):T{if(fail&&node.textContent==='2b')throw new Error('range insert failed');return insert(node,anchor);});
  expect(()=>items.set([1,2])).toThrow('range insert failed');expect(document.body.textContent).toBe('1a1b');expect(Array.from(document.querySelectorAll('span'))).toEqual(nodes);expect(getGraphTelemetry().nodes).toBe(baseline);
  fail=false;items.set([1,2]);expect(document.body.textContent).toBe('1a1b2a2b');app.unmount();items.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('partially moved fragment rows restore their original complete range',()=>{
  const items=signal([1,2]);let fail=true;
  const app=mount(()=>For({each:items,children:pair}),document.body),nodes=Array.from(document.querySelectorAll('span')),insert=document.body.insertBefore.bind(document.body);
  vi.spyOn(document.body,'insertBefore').mockImplementation(function<T extends Node>(node:T,anchor:Node|null):T{const result=insert(node,anchor);if(fail&&node.textContent==='1a'){fail=false;throw new Error('range move failed');}return result;});
  expect(()=>items.set([2,1])).toThrow('range move failed');expect(document.body.textContent).toBe('1a1b2a2b');expect(Array.from(document.querySelectorAll('span'))).toEqual(nodes);
  items.set([2,1]);expect(document.body.textContent).toBe('2a2b1a1b');app.unmount();items.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('outer ranges include rows added later by a nested For',()=>{
  const items=signal([1,2]),lists=new Map([[1,signal(['a'])],[2,signal(['a'])]]);
  const app=mount(()=>For({each:items,children:id=>For({each:lists.get(id)!,children:value=>document.createTextNode(id+value+';')})}),document.body);
  lists.get(1)!.set(['a','b']);expect(document.body.textContent).toBe('1a;1b;2a;');items.set([2,1]);expect(document.body.textContent).toBe('2a;1a;1b;');
  items.set([1]);lists.get(2)!.set(['a','b']);expect(document.body.textContent).toBe('1a;1b;');app.unmount();items.dispose();for(const list of lists.values())list.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('fragment rows retain correct reactive indexes through reorder and removal',()=>{
  const items=signal([1,2]);
  const app=mount(()=>For({each:items,children:(value,index)=>{const fragment=document.createDocumentFragment(),node=document.createElement('span');effect(()=>{node.textContent=value+':'+index()+';';});fragment.append(node);return fragment;}}),document.body);
  const node=document.querySelector('span');items.set([2,1]);expect(document.body.textContent).toBe('2:0;1:1;');items.set([1]);expect(document.body.textContent).toBe('1:0;');expect(document.querySelector('span')).toBe(node);
  app.unmount();items.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('late range insertion after owner cleanup is removed with its staged resources',()=>{
  const items=signal<number[]>([]);const app=mount(()=>For({each:items,children:value=>{createGpuTensor([value]);return pair(value);}}),document.body);
  const insert=document.body.insertBefore.bind(document.body);
  vi.spyOn(document.body,'insertBefore').mockImplementation(function<T extends Node>(node:T,anchor:Node|null):T{if(node.textContent==='1a'){app.unmount();document.body.appendChild(node);return node;}return insert(node,anchor);});
  expect(()=>items.set([1])).toThrow('owner');expect(document.body.childNodes).toHaveLength(0);expect(getGraphTelemetry().nodes).toBe(1);items.dispose();
});


it.each(['single','fragment'] as const)('%s rows retain logical small-move reconciliation',mode=>{
  const items=signal([1,2]);const app=mount(()=>For({each:items,children:mode==='single'?(value:number)=>document.createTextNode(String(value)):pair}),document.body);
  resetDomMutationCount();items.set([2,1]);expect(getDomMutationCount()).toBe(mode==='single'?1:4);
  app.unmount();items.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});
