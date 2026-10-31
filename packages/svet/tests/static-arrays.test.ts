/** @vitest-environment jsdom */
import {beforeEach,expect,it} from 'vitest';
import {createRoot,getGraphTelemetry,h,insertChild,mount,resetReactiveSystem,signal,type ChildNodeValue} from '../src/engine';
beforeEach(()=>{resetReactiveSystem();document.body.replaceChildren();});

it('renders deeply nested static children without recursion overflow',()=>{
 let children:ChildNodeValue='deep';for(let i=0;i<12000;i++)children=[children];
 const app=mount(()=>h('div',null,children),document.body);expect(document.body.textContent).toBe('deep');app.unmount();expect(getGraphTelemetry().nodes).toBe(0);
});

it('rejects static cycles before inserting even the leading sibling',()=>{
 const target=document.createElement('div');target.textContent='existing';const cycle:ChildNodeValue[]=[];cycle.push(cycle);
 expect(()=>insertChild(target,['leading',cycle])).toThrow('Cyclic child array');expect(target.textContent).toBe('existing');expect(getGraphTelemetry().nodes).toBe(0);
});

it('repeated acyclic array references remain valid and preserve empty-value semantics',()=>{
 const shared:ChildNodeValue[]=['A',null,[false,undefined,2]],app=mount(()=>h('div',null,[shared,true,shared]),document.body);
 expect(document.body.textContent).toBe('A2A2');app.unmount();expect(getGraphTelemetry().nodes).toBe(0);
});

it('uses the prepared child list when a nested accessor mutates the source array',()=>{
 const children:ChildNodeValue[]=[()=>{children.push('late');return 'first';},'last'];
 const app=mount(()=>h('div',null,children),document.body);expect(document.body.textContent).toBe('firstlast');expect(children.length).toBe(3);app.unmount();expect(getGraphTelemetry().nodes).toBe(0);
});

it('nested native children and bindings retain their identity and lifecycle',()=>{
 const text=signal('old'),node=h('b',null,'node'),children:ChildNodeValue[]=[[node],[text]],app=mount(()=>h('div',null,children),document.body);
 expect(document.querySelector('b')).toBe(node);text.set('new');expect(document.body.textContent).toBe('nodenew');app.unmount();text.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('cycle preflight does not allocate leading reactive bindings',()=>{
 const text=signal('value'),target=document.createElement('div'),cycle:ChildNodeValue[]=[];cycle.push(cycle);const baseline=getGraphTelemetry().nodes;
 createRoot(()=>{expect(()=>insertChild(target,[text,cycle])).toThrow('Cyclic child array');expect(getGraphTelemetry().nodes).toBe(baseline);});
 expect(target.childNodes.length).toBe(0);text.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});
