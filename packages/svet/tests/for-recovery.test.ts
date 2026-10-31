/** @vitest-environment jsdom */
import {beforeEach,expect,it,vi} from 'vitest';
import {For,mount,signal,effect,createGpuTensor,getGraphTelemetry,resetReactiveSystem,flushEffects} from '../src/engine';

beforeEach(()=>{vi.restoreAllMocks();resetReactiveSystem();document.body.innerHTML='';});
function row(value:number){const node=document.createElement('span');node.textContent=String(value);return node;}

it('failed preparation releases every staged row and preserves existing keyed rows',()=>{
  const items=signal([1]),source=signal(0),runs=new Map<number,number>();let fail=true;
  const app=mount(()=>For({each:items,children:value=>{createGpuTensor([value]);effect(()=>{source();runs.set(value,(runs.get(value)??0)+1);});if(value===3&&fail)throw new Error('row failed');return row(value);}}),document.body);
  const first=document.querySelector('span'),baseline=getGraphTelemetry().nodes;
  expect(()=>items.set([1,2,3])).toThrow('row failed');expect(document.body.textContent).toBe('1');expect(document.querySelector('span')).toBe(first);expect(getGraphTelemetry().nodes).toBe(baseline);
  source.set(1);expect(runs.get(1)).toBe(2);expect(runs.get(2)).toBe(1);
  fail=false;items.set([1,2,3]);expect(document.body.textContent).toBe('123');expect(document.querySelector('span')).toBe(first);
  app.unmount();items.dispose();source.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('failed preparation does not publish reordered reactive indexes',()=>{
  const items=signal([1,2]);let fail=true;
  const app=mount(()=>For({each:items,children:(value,index)=>{const node=row(value);effect(()=>{node.textContent=value+':'+index()+';';});if(value===3&&fail)throw new Error('row failed');return node;}}),document.body);
  const baseline=getGraphTelemetry().nodes;expect(()=>items.set([2,1,3])).toThrow('row failed');flushEffects();
  expect(document.body.textContent).toBe('1:0;2:1;');expect(getGraphTelemetry().nodes).toBe(baseline);
  fail=false;items.set([2,1,3]);expect(document.body.textContent).toBe('2:0;1:1;3:2;');app.unmount();items.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('removing rows releases lazy reactive index signals',()=>{
  const items=signal([1,2,3]);const app=mount(()=>For({each:items,children:(value,index)=>{const node=row(value);effect(()=>{node.textContent=String(index());});return node;}}),document.body);
  items.set([]);expect(getGraphTelemetry().nodes).toBe(2);app.unmount();items.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('retained unused index accessors cannot allocate signals after row disposal',()=>{
  const items=signal([1]),indexes:Array<()=>number>=[];
  const app=mount(()=>For({each:items,children:(value,index)=>{indexes.push(index);return row(value);}}),document.body);
  items.set([]);const baseline=getGraphTelemetry().nodes;expect(indexes[0]()).toBe(0);expect(getGraphTelemetry().nodes).toBe(baseline);
  app.unmount();items.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('duplicate keys reject before modifying the committed list',()=>{
  const items=signal([1,2]);const app=mount(()=>For({each:items,children:row}),document.body);
  const nodes=Array.from(document.querySelectorAll('span'));expect(()=>items.set([2,2])).toThrow('Duplicate');
  expect(Array.from(document.querySelectorAll('span'))).toEqual(nodes);expect(document.body.textContent).toBe('12');
  items.set([2,3]);expect(document.body.textContent).toBe('23');app.unmount();items.dispose();
});

it('insertion failure removes staged rows and restores old row order and resources',()=>{
  const items=signal([1,2]),source=signal(0),runs=new Map<number,number>();let reject=true;
  const app=mount(()=>For({each:items,children:value=>{createGpuTensor([value]);effect(()=>{source();runs.set(value,(runs.get(value)??0)+1);});return row(value);}}),document.body);
  const nodes=Array.from(document.querySelectorAll('span')),baseline=getGraphTelemetry().nodes,insert=document.body.insertBefore.bind(document.body);
  vi.spyOn(document.body,'insertBefore').mockImplementation(function<T extends Node>(node:T,anchor:Node|null):T{if(reject&&node.textContent==='3')throw new Error('insert failed');return insert(node,anchor);});
  expect(()=>items.set([2,3,4])).toThrow('insert failed');expect(document.body.textContent).toBe('12');expect(Array.from(document.querySelectorAll('span'))).toEqual(nodes);expect(getGraphTelemetry().nodes).toBe(baseline);
  source.set(1);expect(runs.get(1)).toBe(2);reject=false;items.set([2,3,4]);expect(document.body.textContent).toBe('234');
  app.unmount();items.dispose();source.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('unmount during row rendering releases previously staged independent row scopes',()=>{
  const items=signal<number[]>([]);let app!:ReturnType<typeof mount>;
  app=mount(()=>For({each:items,children:value=>{createGpuTensor([value]);if(value===2)app.unmount();return row(value);}}),document.body);
  expect(()=>items.set([1,2])).toThrow('owner');expect(document.body.childNodes).toHaveLength(0);expect(getGraphTelemetry().nodes).toBe(1);items.dispose();
});

it('string-key lookup entries are published only after all rows prepare',()=>{
  const items=signal(['a']);let fail=true;
  const app=mount(()=>For({each:items,children:value=>{createGpuTensor([1]);if(value==='c'&&fail)throw new Error('row failed');return document.createTextNode(value);}}),document.body);
  const baseline=getGraphTelemetry().nodes;expect(()=>items.set(['a','b','c'])).toThrow('row failed');expect(document.body.textContent).toBe('a');expect(getGraphTelemetry().nodes).toBe(baseline);
  fail=false;items.set(['a','b','c']);expect(document.body.textContent).toBe('abc');app.unmount();items.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('a failed move of existing rows restores node order and reactive indexes',()=>{
  const items=signal([1,2,3]);let fail=true;
  const app=mount(()=>For({each:items,children:(value,index)=>{const node=row(value);effect(()=>{node.textContent=value+':'+index()+';';});return node;}}),document.body);
  const nodes=Array.from(document.querySelectorAll('span')),insert=document.body.insertBefore.bind(document.body);
  vi.spyOn(document.body,'insertBefore').mockImplementation(function<T extends Node>(node:T,anchor:Node|null):T{const result=insert(node,anchor);if(fail&&node.textContent?.startsWith('3:')){fail=false;throw new Error('move failed');}return result;});
  expect(()=>items.set([3,1,2])).toThrow('move failed');expect(Array.from(document.querySelectorAll('span'))).toEqual(nodes);expect(document.body.textContent).toBe('1:0;2:1;3:2;');
  items.set([3,1,2]);expect(document.body.textContent).toBe('3:0;1:1;2:2;');app.unmount();items.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('rendering snapshots list values before user callbacks can mutate the input array',()=>{
  const input=[1,2],items=signal(input);
  const app=mount(()=>For({each:items,children:value=>{if(value===1)input[1]=99;return row(value);}}),document.body);
  expect(document.body.textContent).toBe('12');items.set([1,99]);expect(document.body.textContent).toBe('199');app.unmount();items.dispose();
});

it('reset during row preparation releases stale rows without touching replacement state',()=>{
  const items=signal<number[]>([]);let fresh!:ReturnType<typeof signal<number>>;
  const app=mount(()=>For({each:items,children:value=>{createGpuTensor([1]);resetReactiveSystem();fresh=signal(10);return row(value);}}),document.body);
  expect(()=>items.set([1])).toThrow('reset');expect(document.body.childNodes).toHaveLength(0);expect(getGraphTelemetry().nodes).toBe(1);
  app.unmount();items.dispose();fresh.set(11);expect(fresh()).toBe(11);fresh.dispose();
});

it('duplicate initial keys fail before any row renderer or partial mount',()=>{
  const items=signal([1,1]);let renders=0;document.body.textContent='previous';
  expect(()=>mount(()=>For({each:items,children:value=>{renders++;return row(value);}}),document.body)).toThrow('Duplicate');
  expect(renders).toBe(0);expect(document.body.textContent).toBe('previous');expect(getGraphTelemetry().nodes).toBe(1);items.dispose();
});
