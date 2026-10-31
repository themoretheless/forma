/** @vitest-environment jsdom */
import {afterEach,beforeEach,expect,it,vi} from 'vitest';
import * as first from '../src/engine';
let stops:Array<()=>void>=[];
beforeEach(()=>{first.resetReactiveSystem();document.body.replaceChildren();});
afterEach(()=>{for(const stop of stops.reverse())stop();stops=[];vi.restoreAllMocks();});
async function second(){vi.resetModules();return import('../src/engine/index.ts');}
it('independent engine instances invoke only once per delegated binding',async()=>{
 const other=await second();let a=0,b=0;first.createRoot(stop=>{stops.push(stop);document.body.append(first.h('button',{onClick(){a++;}},'first'));});other.createRoot(stop=>{stops.push(stop);document.body.append(other.h('button',{onClick(){b++;}},'second'));});for(const button of document.querySelectorAll('button'))button.click();expect([a,b]).toEqual([1,1]);
});
it('mixed nested instances preserve child-before-parent bubbling order',async()=>{
 const other=await second(),order:string[]=[];let outer!:Node,inner!:HTMLButtonElement;
 first.createRoot(stop=>{stops.push(stop);outer=first.h('div',{onClick(){order.push('outer');}});document.body.append(outer);});other.createRoot(stop=>{stops.push(stop);inner=other.h('button',{onClick(){order.push('inner');}}) as HTMLButtonElement;outer.appendChild(inner);});inner.click();expect(order).toEqual(['inner','outer']);
});
it('bindings from two instances on one node remain additive after one owner retires',async()=>{
 const other=await second(),button=document.createElement('button');document.body.append(button);let a=0,b=0,stopFirst!:()=>void;
 first.createRoot(stop=>{stopFirst=stop;stops.push(stop);first.bindProp(button,'onClick',()=>{a++;});});other.createRoot(stop=>{stops.push(stop);other.bindProp(button,'onClick',()=>{b++;});});button.click();stopFirst();button.click();expect([a,b]).toEqual([1,2]);
});
it('independent instances retain one physical document listener until the last owner retires',async()=>{
 const other=await second(),add=vi.spyOn(document,'addEventListener'),remove=vi.spyOn(document,'removeEventListener');let a!:()=>void,b!:()=>void;
 first.createRoot(stop=>{a=stop;stops.push(stop);first.h('button',{onClick(){}});});other.createRoot(stop=>{b=stop;stops.push(stop);other.h('button',{onClick(){}});});expect(add.mock.calls.filter(([type])=>type==='click')).toHaveLength(1);a();expect(remove.mock.calls.filter(([type])=>type==='click')).toHaveLength(0);b();expect(remove.mock.calls.filter(([type])=>type==='click')).toHaveLength(1);expect(first.getGraphTelemetry().nodes).toBe(0);expect(other.getGraphTelemetry().nodes).toBe(0);
});
