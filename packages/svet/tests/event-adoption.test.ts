/** @vitest-environment jsdom */
import {afterEach,beforeEach,expect,it,vi} from 'vitest';
import {createRoot,For,getGraphTelemetry,h,insertChild,mount,Portal,resetReactiveSystem,signal} from '../src/engine';
let stops:Array<()=>void>=[];
beforeEach(()=>{resetReactiveSystem();document.body.replaceChildren();});
afterEach(()=>{for(const stop of stops.reverse())stop();stops=[];vi.restoreAllMocks();});
function destination(){const frame=document.createElement('iframe');document.body.append(frame);return frame.contentDocument!;}
it('mount adopts nested delegated handlers before the first destination event',()=>{
 const doc=destination();let calls=0;const app=mount(()=>h('section',null,h('button',{onClick(){calls++;}},'child')),doc.body);stops.push(app.unmount);
 doc.querySelector('button')!.click();expect(calls).toBe(1);app.unmount();expect(getGraphTelemetry().nodes).toBe(0);
});
it('Portal children route through their destination document',()=>{
 const doc=destination();let calls=0;createRoot(stop=>{stops.push(stop);Portal({mount:doc.body,children:()=>h('button',{onClick(){calls++;}},'portal')});});doc.querySelector('button')!.click();expect(calls).toBe(1);
});
it('dynamic and keyed children created after foreign mounting retain handlers',()=>{
 const doc=destination(),value=signal(false),rows=signal<number[]>([]);let calls=0;
 const app=mount(()=>h('div',null,()=>value()?h('button',{onClick(){calls++;}},'dynamic'):null,For({each:rows,keyFn:x=>x,children:x=>h('button',{onClick(){calls+=x;}},'row')})),doc.body);stops.push(()=>{app.unmount();value.dispose();rows.dispose();});
 value.set(true);rows.set([2]);for(const button of doc.querySelectorAll('button'))button.click();expect(calls).toBe(3);
});
it('static subtree insertion can move out and back without duplicate routing',()=>{
 const doc=destination();let calls=0;createRoot(stop=>{stops.push(stop);const node=h('div',null,h('button',{onClick(){calls++;}},'nested'));insertChild(doc.body,node);doc.querySelector('button')!.click();insertChild(document.body,node);document.querySelector('button')!.click();insertChild(doc.body,node);doc.querySelector('button')!.click();});expect(calls).toBe(3);
});
it('native insertion failure keeps the original document handler and releases staging',()=>{
 const doc=destination();let calls=0;createRoot(stop=>{stops.push(stop);const button=h('button',{onClick(){calls++;}}) as HTMLButtonElement;document.body.append(button);const failure=Error('append failed');vi.spyOn(doc.body,'appendChild').mockImplementation(()=>{throw failure;});expect(()=>insertChild(doc.body,button)).toThrow(failure);button.click();expect(calls).toBe(1);});
});
it('destination registration teardown cannot attach a late live handler',()=>{
 const doc=destination();let calls=0;const remove=vi.spyOn(doc,'removeEventListener'),add=doc.addEventListener.bind(doc);
 createRoot(stop=>{stops.push(stop);const button=h('button',{onClick(){calls++;}});vi.spyOn(doc,'addEventListener').mockImplementation((type,listener,options)=>{if(type==='click')stop();add(type,listener,options);});expect(()=>insertChild(doc.body,button)).toThrow(/disposed/);expect(button.parentNode).toBeNull();});expect(calls).toBe(0);expect(remove.mock.calls.some(([type])=>type==='click')).toBe(true);
});
it('destination dispatch during native adoption already reaches the handler once',()=>{
 const doc=destination();let calls=0;createRoot(stop=>{stops.push(stop);const button=h('button',{onClick(){calls++;}}) as HTMLButtonElement,append=doc.body.appendChild.bind(doc.body);vi.spyOn(doc.body,'appendChild').mockImplementation(<T extends Node>(node:T):T=>{append(node);button.click();return node;});insertChild(doc.body,button);});expect(calls).toBe(1);
});
it('shared document leases migrate once per event type and retire on owner teardown',()=>{
 const doc=destination(),oldRemove=vi.spyOn(document,'removeEventListener'),newAdd=vi.spyOn(doc,'addEventListener'),newRemove=vi.spyOn(doc,'removeEventListener');let dispose!:()=>void,calls=0;
 createRoot(stop=>{dispose=stop;stops.push(stop);const tree=h('section',null,h('button',{onClick(){calls++;}}),h('button',{onClick(){calls++;}}));insertChild(doc.body,tree);});
 expect(newAdd.mock.calls.filter(([type])=>type==='click')).toHaveLength(1);expect(oldRemove.mock.calls.filter(([type])=>type==='click')).toHaveLength(1);
 for(const button of doc.querySelectorAll('button'))button.click();expect(calls).toBe(2);dispose();expect(newRemove.mock.calls.filter(([type])=>type==='click')).toHaveLength(1);for(const button of doc.querySelectorAll('button'))button.click();expect(calls).toBe(2);
});
it('registration failure preserves the source binding and the original exception',()=>{
 const doc=destination(),failure=Error('destination add failed');let calls=0;createRoot(stop=>{stops.push(stop);const button=h('button',{onClick(){calls++;}}) as HTMLButtonElement;document.body.append(button);vi.spyOn(doc,'addEventListener').mockImplementation(()=>{throw failure;});expect(()=>insertChild(doc.body,button)).toThrow(failure);button.click();expect(calls).toBe(1);});
});
it('dynamic insertion rollback restores document routing of a moved caller node',()=>{
 const doc=destination(),source=signal<Node|string>('initial');let calls=0;const app=mount(()=>h('div',null,source),doc.body);stops.push(()=>{app.unmount();source.dispose();});
 createRoot(stop=>{stops.push(stop);const button=h('button',{onClick(){calls++;}}) as HTMLButtonElement;document.body.append(button);const target=doc.body.firstChild!,insert=target.insertBefore.bind(target),failure=Error('late insertion failed');let once=true;vi.spyOn(target,'insertBefore').mockImplementation(<T extends Node>(node:T,anchor:Node|null):T=>{insert(node,anchor);if((node as Node)===button&&once){once=false;throw failure;}return node;});expect(()=>source.set(button)).toThrow(failure);expect(button.parentNode).toBe(document.body);button.click();expect(calls).toBe(1);});
});
it('an inner adoption cannot release the outer insertion destination before publication',()=>{
 const outer=destination(),inner=destination();let calls=0;createRoot(stop=>{stops.push(stop);const button=h('button',{onClick(){calls++;}}) as HTMLButtonElement,append=outer.body.appendChild.bind(outer.body);vi.spyOn(outer.body,'appendChild').mockImplementation(<T extends Node>(node:T):T=>{insertChild(inner.body,node);append(node);button.click();return node;});insertChild(outer.body,button);expect(button.ownerDocument).toBe(outer);});expect(calls).toBe(1);
});
it('failed physical listener removal cannot duplicate callbacks after a return adoption',()=>{
 const doc=destination(),failure=Error('remove failed'),remove=document.removeEventListener.bind(document);let calls=0,once=true;
 createRoot(stop=>{stops.push(stop);const button=h('button',{onClick(){calls++;}}) as HTMLButtonElement;vi.spyOn(document,'removeEventListener').mockImplementation((type,listener,options)=>{if(type==='click'&&once){once=false;throw failure;}remove(type,listener,options);});expect(()=>insertChild(doc.body,button)).toThrow(failure);button.click();insertChild(document.body,button);button.click();});expect(calls).toBe(2);
});
it('listener removal reentry cannot erase a new live document registration',()=>{
 const doc=destination(),remove=document.removeEventListener.bind(document);let first=0,second=0,once=true;let replacement!:HTMLButtonElement;
 createRoot(stop=>{stops.push(stop);const button=h('button',{onClick(){first++;}}) as HTMLButtonElement;vi.spyOn(document,'removeEventListener').mockImplementation((type,listener,options)=>{if(type==='click'&&once){once=false;replacement=h('button',{onClick(){second++;}}) as HTMLButtonElement;document.body.append(replacement);}remove(type,listener,options);});insertChild(doc.body,button);button.click();replacement.click();});expect([first,second]).toEqual([1,1]);
});
