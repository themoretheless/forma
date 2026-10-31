/** @vitest-environment jsdom */
import {afterEach,beforeEach,expect,it,vi} from 'vitest';
import {bindProp,createRoot,effect,getGraphTelemetry,h,onCleanup,resetReactiveSystem,signal,type Signal} from '../src/engine';
let stops:Array<()=>void>=[],sources:Array<Signal<any>>=[];
beforeEach(()=>{resetReactiveSystem();document.body.replaceChildren();});
afterEach(()=>{for(const stop of stops.reverse())stop();for(const source of sources)source.dispose();stops=[];sources=[];vi.restoreAllMocks();});
function source(){const value=signal(0);sources.push(value);return value;}
it('h rejects disposed callers before reading prop getters',()=>{
 let reads=0;createRoot(stop=>{stops.push(stop);stop();expect(()=>h('div',{get title(){reads++;return 'stale';}})).toThrow(/disposed/);});expect(reads).toBe(0);
});
it('prop getter teardown prevents later getter reads',()=>{
 let later=0;createRoot(stop=>{stops.push(stop);expect(()=>h('div',{get title(){stop();return 'stale';},get id(){later++;return 'later';}})).toThrow(/disposed/);});expect(later).toBe(0);
});
it('throwing prop metadata retires provisional effects without retiring the caller',()=>{
 const value=source(),failure=Error('getter failed');let runs=0;createRoot(stop=>{stops.push(stop);const before=getGraphTelemetry().nodes;expect(()=>h('div',{get title(){effect(()=>{value();runs++;});throw failure;}})).toThrow(failure);expect(getGraphTelemetry().nodes).toBe(before);});value.set(1);expect(runs).toBe(1);
});
it('failed ref releases earlier element bindings and its own provisional effects',()=>{
 const value=source(),failure=Error('ref failed');let runs=0,retained!:HTMLElement,calls=0;createRoot(stop=>{stops.push(stop);const before=getGraphTelemetry().nodes;expect(()=>h('input',{model:value,onClick(){calls++;},ref(node:HTMLElement){retained=node;effect(()=>{value();runs++;});throw failure;}})).toThrow(failure);expect(getGraphTelemetry().nodes).toBe(before);});document.body.append(retained);retained.click();value.set(1);expect([calls,runs]).toEqual([0,1]);
});
it('ref teardown prevents moving existing children into the rejected element',()=>{
 const child=document.createTextNode('original'),host=document.createElement('div');host.append(child);createRoot(stop=>{stops.push(stop);expect(()=>h('div',{ref(){stop();}},child)).toThrow(/disposed/);});expect(child.parentNode).toBe(host);
});
it('component factory output is rejected after caller teardown',()=>{
 const child=document.createTextNode('original');createRoot(stop=>{stops.push(stop);expect(()=>h(()=>{stop();return child;},null)).toThrow(/disposed/);});
});
it('component failure retires resources when its caller catches the error',()=>{
 const value=source(),failure=Error('component failed');let runs=0;createRoot(stop=>{stops.push(stop);expect(()=>h(()=>{effect(()=>{value();runs++;});throw failure;},null)).toThrow(failure);});value.set(1);expect(runs).toBe(1);
});
it('public ref binding rejects a disposed caller before callback execution',()=>{
 const callback=vi.fn();createRoot(stop=>{stops.push(stop);stop();expect(()=>bindProp(document.createElement('div'),'ref',callback)).toThrow(/disposed/);});expect(callback).not.toHaveBeenCalled();
});
it('public ref binding failure retires its provisional resources',()=>{
 const value=source(),failure=Error('binding failed');let runs=0;createRoot(stop=>{stops.push(stop);expect(()=>bindProp(document.createElement('div'),'ref',()=>{effect(()=>{value();runs++;});throw failure;})).toThrow(failure);});value.set(1);expect(runs).toBe(1);
});
it('graph reset during prop metadata preserves independent replacement lifetimes',()=>{
 let fresh!:Signal<number>,observer!:ReturnType<typeof effect>,later=0;const values:number[]=[];createRoot(stop=>{stops.push(stop);expect(()=>h('div',{get title(){resetReactiveSystem();fresh=signal(17);observer=effect(()=>{values.push(fresh());});return 'stale';},get id(){later++;return 'later';}})).toThrow(/reset/);});fresh.set(18);expect(later).toBe(0);expect(values).toEqual([17,18]);observer.dispose();fresh.dispose();
});
it('prop snapshots retain enumerable symbols, own __proto__ data and child override semantics',()=>{
 const symbol=Symbol('prop'),prototype={inherited:'ignored'},ownProto={marker:true},child=document.createTextNode('override');let reads=0,childrenReads=0,captured!:Record<PropertyKey,any>;
 const props=Object.create(prototype);Object.defineProperties(props,{title:{enumerable:true,get(){reads++;return 'title';}},children:{enumerable:true,get(){childrenReads++;return document.createTextNode('ignored');}},hidden:{value:'hidden'}});Object.defineProperty(props,'__proto__',{value:ownProto,enumerable:true});props[symbol]='symbol';
 createRoot(stop=>{stops.push(stop);expect(h(p=>{captured=p;return p.children;},props,child)).toBe(child);});expect(reads).toBe(1);expect(childrenReads).toBe(1);expect(captured[symbol]).toBe('symbol');expect(captured.inherited).toBeUndefined();expect(captured.hidden).toBeUndefined();expect(Object.getPrototypeOf(captured)).toBe(Object.prototype);expect(Object.prototype.hasOwnProperty.call(captured,'__proto__')).toBe(true);expect(captured.__proto__).toBe(ownProto);
});
it('child coercion failure retires element bindings without disposing a caller-owned source',()=>{
 const value=source(),failure=Error('child coercion failed');createRoot(stop=>{stops.push(stop);const before=getGraphTelemetry().nodes;expect(()=>h('input',{model:value},{toString(){throw failure;}} as unknown as string)).toThrow(failure);expect(getGraphTelemetry().nodes).toBe(before);});value.set(1);expect(value()).toBe(1);
});
it('native element construction teardown rejects an otherwise empty element',()=>{
 const create=document.createElement.bind(document);createRoot(stop=>{stops.push(stop);vi.spyOn(document,'createElement').mockImplementationOnce((tag:string)=>{const element=create(tag);stop();return element;});expect(()=>h('div',null)).toThrow(/disposed/);});
});
it('ref graph reset rejects continuation and preserves replacement graph effects',()=>{
 let fresh!:Signal<number>,observer!:ReturnType<typeof effect>;const values:number[]=[];createRoot(stop=>{stops.push(stop);expect(()=>bindProp(document.createElement('div'),'ref',()=>{resetReactiveSystem();fresh=signal(17);observer=effect(()=>{values.push(fresh());});})).toThrow(/reset/);});fresh.set(18);expect(values).toEqual([17,18]);observer.dispose();fresh.dispose();
});
it('prop ownKeys teardown prevents descriptor traps and getter evaluation',()=>{
 let descriptors=0;createRoot(stop=>{stops.push(stop);const props=new Proxy({title:'stale'},{ownKeys(){stop();return ['title'];},getOwnPropertyDescriptor(target,key){descriptors++;return Reflect.getOwnPropertyDescriptor(target,key);}});expect(()=>h('div',props)).toThrow(/disposed/);});expect(descriptors).toBe(0);
});
it('metadata reads retain caller tracking while component factories remain untracked',()=>{
 const value=source(),metadataValues:number[]=[];let componentRuns=0;createRoot(stop=>{stops.push(stop);effect(()=>{h('div',{get title(){metadataValues.push(value());return 'title';}});});effect(()=>{h(()=>{value();componentRuns++;return document.createTextNode('component');},null);});});value.set(1);expect(metadataValues).toEqual([0,1]);expect(componentRuns).toBe(1);
});

it('earlier sibling cleanup cannot invoke handlers or effects under a stopping ancestor',()=>{
 const value=source(),modelValue=signal('kept');sources.push(modelValue);let button!:HTMLButtonElement,input!:HTMLInputElement,calls=0,runs=0;createRoot(stop=>{stops.push(stop);h('div',{ref(){onCleanup(()=>{button.click();input.value='stale';input.dispatchEvent(new Event('input',{bubbles:true}));value.set(1);});}});button=h('button',{onClick(){calls++;},ref(){effect(()=>{value();runs++;});}}) as HTMLButtonElement;input=h('input',{model:modelValue}) as HTMLInputElement;document.body.append(button,input);});stops[0]();expect([calls,runs,modelValue()]).toEqual([0,1,'kept']);
});
