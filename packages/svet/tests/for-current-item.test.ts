/** @vitest-environment jsdom */
import {beforeEach,expect,it} from 'vitest';
import {For,h,mount,effect,signal,compileFormaComponent,getGraphTelemetry,resetReactiveSystem} from '../src/engine';

beforeEach(()=>{resetReactiveSystem();document.body.innerHTML='';});

it('same-key object replacement updates current bindings without recreating the row or text node',()=>{
  const items=signal([{id:1,label:'old'}]);let renders=0;
  const app=mount(()=>For({each:items,keyFn:item=>item.id,children:(_initial,index,current)=>{renders++;return h('span',null,()=>current().label+'@'+index());}}),document.body);
  const span=document.querySelector('span')!,text=span.firstChild,baseline=getGraphTelemetry().nodes;
  items.set([{id:1,label:'new'}]);expect(span.textContent).toBe('new@0');expect(document.querySelector('span')).toBe(span);expect(span.firstChild).toBe(text);expect(renders).toBe(1);expect(getGraphTelemetry().nodes).toBe(baseline);
  app.unmount();items.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('replacement and reorder publish current data and indexes together',()=>{
  const items=signal([{id:1,label:'A'},{id:2,label:'B'}]);
  const app=mount(()=>For({each:items,keyFn:item=>item.id,children:(_initial,index,current)=>h('span',null,()=>current().label+'@'+index()+';')}),document.body);
  const nodes=Array.from(document.querySelectorAll('span'));items.set([{id:2,label:'new B'},{id:1,label:'new A'}]);
  expect(document.body.textContent).toBe('new B@0;new A@1;');expect(Array.from(document.querySelectorAll('span'))).toEqual([nodes[1],nodes[0]]);
  app.unmount();items.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('failed preparation does not publish replacements to an existing row current accessor',()=>{
  const items=signal([{id:1,label:'old'}]);let fail=true;
  const app=mount(()=>For({each:items,keyFn:item=>item.id,children:(initial,_index,current)=>{if(initial.id===2&&fail)throw new Error('row failed');return h('span',null,()=>current().label);}}),document.body);
  expect(()=>items.set([{id:1,label:'new'},{id:2,label:'second'}])).toThrow('row failed');expect(document.body.textContent).toBe('old');
  fail=false;items.set([{id:1,label:'new'},{id:2,label:'second'}]);expect(document.body.textContent).toBe('newsecond');app.unmount();items.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it.each([false,true])('retained current accessors freeze without allocating after disposal (used=%s)',used=>{
  const items=signal([1]);let current!:()=>number;
  const app=mount(()=>For({each:items,keyFn:()=>1,children:(initial,_index,value)=>{current=value;if(used)value();return document.createTextNode(String(initial));}}),document.body);
  items.set([2]);items.set([]);const baseline=getGraphTelemetry().nodes;expect(current()).toBe(2);expect(getGraphTelemetry().nodes).toBe(baseline);
  app.unmount();items.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('function-valued items are stored without being treated as signal updaters',()=>{
  let firstCalls=0,secondCalls=0;const first=()=>{firstCalls++;return 'first';},second=()=>{secondCalls++;return 'second';},items=signal([first]);
  const app=mount(()=>For({each:items,keyFn:()=>1,children:(_initial,_index,current)=>h('span',null,()=>current()())}),document.body);
  items.set([second]);expect(document.body.textContent).toBe('second');expect([firstCalls,secondCalls]).toEqual([1,1]);app.unmount();items.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('Forma same-key replacements update text and actions while preserving the DOM',()=>{
  const instance=compileFormaComponent(`component LiveList { state rows: List = []; Column { for row in state.rows key row.id { Text { text: row.label; } Button { text: 'select'; clicked -> events.selected(row.label); } } } }`);
  instance.state.rows.set([{id:1,label:'old'}]);const app=instance.mount(document.body),text=document.querySelector('[data-forma-type="Text"]')!,button=document.querySelector('button')!,events:string[]=[];
  instance.onEvent('selected',value=>events.push(value));instance.state.rows.set([{id:1,label:'new'}]);
  expect(text.textContent).toBe('new');expect(document.querySelector('[data-forma-type="Text"]')).toBe(text);button.dispatchEvent(new MouseEvent('click',{bubbles:true}));expect(events).toEqual(['new']);
  app.unmount();instance.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('nested Forma loop locals retain live outer and inner values',()=>{
  const instance=compileFormaComponent(`component NestedList { state groups: List = []; Column { for group in state.groups key group.id { for child in group.children key child.id { Text { text: group.title + ':' + child.label; } } } } }`);
  instance.state.groups.set([{id:1,title:'old group',children:[{id:1,label:'old child'}]}]);const app=instance.mount(document.body),text=document.querySelector('[data-forma-type="Text"]')!;
  instance.state.groups.set([{id:1,title:'new group',children:[{id:1,label:'new child'}]}]);expect(text.textContent).toBe('new group:new child');expect(document.querySelector('[data-forma-type="Text"]')).toBe(text);
  app.unmount();instance.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('pure reorder of identical objects does not rerun current-only bindings',()=>{
  const values=[{id:1,label:'A'},{id:2,label:'B'}],items=signal(values),runs=new Map<number,number>();
  const app=mount(()=>For({each:items,keyFn:item=>item.id,children:(initial,_index,current)=>{const node=document.createElement('span');effect(()=>{runs.set(initial.id,(runs.get(initial.id)??0)+1);node.textContent=current().label;});return node;}}),document.body);
  items.set([values[1],values[0]]);expect(document.body.textContent).toBe('BA');expect([...runs.values()]).toEqual([1,1]);app.unmount();items.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('Forma positional loops update replaced primitive values without rebuilding elements',()=>{
  const instance=compileFormaComponent(`component PositionalList { state rows: List = []; Column { for row in state.rows { Text { text: row; } } } }`);
  instance.state.rows.set(['old A','old B']);const app=instance.mount(document.body),nodes=Array.from(document.querySelectorAll('[data-forma-type="Text"]'));
  instance.state.rows.set(['new A','new B']);expect(nodes.map(node=>node.textContent)).toEqual(['new A','new B']);expect(Array.from(document.querySelectorAll('[data-forma-type="Text"]'))).toEqual(nodes);
  app.unmount();instance.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('Forma current loop values propagate through nested component props',()=>{
  const instance=compileFormaComponent(`component ParentList { state rows: List = []; Column { for row in state.rows key row.id { ItemView { label: row.label; } } } }`,{components:{ItemView:`component ItemView { prop label: String = ''; Text { text: props.label; } }`}});
  instance.state.rows.set([{id:1,label:'old'}]);const app=instance.mount(document.body),node=document.querySelector('[data-forma-component="ItemView"] [data-forma-type="Text"]')!;
  instance.state.rows.set([{id:1,label:'new'}]);expect(node.textContent).toBe('new');expect(document.querySelector('[data-forma-component="ItemView"] [data-forma-type="Text"]')).toBe(node);
  app.unmount();instance.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it.each(['default', 'rest'] as const)('reactive indexes work with %s callback parameters',style=>{
  const items=signal(['A','B']);
  const renderDefault=(item:string,index:()=>number=()=>-1)=>h('span',null,()=>item+index());
  const renderRest=(item:string,...args:[()=>number,()=>string])=>h('span',null,()=>item+args[0]());
  const app=mount(()=>For({each:items,children:style==='default'?renderDefault:renderRest}),document.body);
  const nodes=Array.from(document.querySelectorAll('span'));
  expect(document.body.textContent).toBe('A0B1');
  items.set(['B','A']);
  expect(document.body.textContent).toBe('B0A1');
  expect(Array.from(document.querySelectorAll('span'))).toEqual([nodes[1],nodes[0]]);
  app.unmount();items.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});
