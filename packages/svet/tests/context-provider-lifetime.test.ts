/** @vitest-environment jsdom */
import {afterEach,beforeEach,expect,it,vi} from 'vitest';
import {createContext,createRoot,effect,getGraphTelemetry,h,resetReactiveSystem,signal,useContext,type Signal} from '../src/engine';
let stop:(()=>void)|undefined;
beforeEach(()=>{resetReactiveSystem();document.body.replaceChildren();});afterEach(()=>{stop?.();stop=undefined;vi.restoreAllMocks();});

it('reset in the value getter prevents children execution and preserves fresh graph state',()=>{
 const context=createContext('default'),children=vi.fn(()=>document.createTextNode('stale'));let fresh!:Signal<number>;
 createRoot(dispose=>{stop=dispose;expect(()=>context.Provider({get value(){resetReactiveSystem();fresh=signal(17);return 'value';},children})).toThrow(/reset/);});expect(children).not.toHaveBeenCalled();fresh.set(18);expect(fresh()).toBe(18);fresh.dispose();
});

it('value getter failure retires provisional effects while preserving its caller',()=>{
 const context=createContext('default'),source=signal(0),failure=Error('value getter failed');let runs=0;
 createRoot(dispose=>{stop=dispose;const baseline=getGraphTelemetry().nodes;expect(()=>context.Provider({get value():string{effect(()=>{source();runs++;});throw failure;},children:null})).toThrow(failure);expect(getGraphTelemetry().nodes).toBe(baseline);});source.set(1);expect(runs).toBe(1);source.dispose();
});

it('reset in the children getter stops its factory before it executes',()=>{
 const context=createContext('default'),factory=vi.fn(()=>document.createTextNode('stale'));let reads=0;
 createRoot(dispose=>{stop=dispose;expect(()=>context.Provider({value:'value',get children(){reads++;resetReactiveSystem();return factory;}})).toThrow(/reset/);});expect(factory).not.toHaveBeenCalled();expect(reads).toBe(1);
});

it('caller teardown in the children getter prevents factory invocation',()=>{
 const context=createContext('default'),factory=vi.fn(()=>document.createTextNode('stale'));
 createRoot(dispose=>{stop=dispose;expect(()=>context.Provider({value:'value',get children(){dispose();return factory;}})).toThrow(/disposed/);});expect(factory).not.toHaveBeenCalled();expect(getGraphTelemetry().nodes).toBe(0);
});

it('children factory disposal prevents movement of an existing caller-owned node',()=>{
 const context=createContext('default'),host=document.createElement('div'),node=document.createTextNode('original');host.append(node);document.body.append(host);
 createRoot(dispose=>{stop=dispose;expect(()=>context.Provider({value:'value',children:()=>{dispose();return node;}})).toThrow(/disposed/);});expect(node.parentNode).toBe(host);
});

it('metadata is read once and the children getter already sees the provided context',()=>{
 const context=createContext('default');let valueReads=0,childReads=0,calls=0;const values:string[]=[];
 createRoot(dispose=>{stop=dispose;const fragment=context.Provider({get value(){valueReads++;return 'provided';},get children(){childReads++;values.push(useContext(context));return()=>{calls++;values.push(useContext(context));return h('span',null,'child');};}});expect(fragment.textContent).toBe('child');});
 expect([valueReads,childReads,calls]).toEqual([1,1,1]);expect(values).toEqual(['provided','provided']);expect(useContext(context)).toBe('default');
});

it('failed child rendering releases provider-owned resources',()=>{
 const context=createContext('default'),source=signal(0),failure=Error('child failed');let runs=0;
 createRoot(dispose=>{stop=dispose;expect(()=>context.Provider({value:'value',children:()=>{effect(()=>{source();runs++;});throw failure;}})).toThrow(failure);});source.set(1);expect(runs).toBe(1);source.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('disposed callers cannot read provider metadata',()=>{
 const context=createContext('default');let reads=0;
 createRoot(dispose=>{stop=dispose;dispose();expect(()=>context.Provider({get value(){reads++;return 'value';},children:null})).toThrow(/disposed/);});expect(reads).toBe(0);
});

it('nested providers retain shadowing and restore the outer value',()=>{
 const context=createContext('default'),values:string[]=[];
 createRoot(dispose=>{stop=dispose;context.Provider({value:'outer',children:()=>{values.push(useContext(context));const fragment=context.Provider({value:'inner',children:()=>{values.push(useContext(context));return document.createTextNode('inner');}});values.push(useContext(context));return fragment;}});});expect(values).toEqual(['outer','inner','outer']);expect(useContext(context)).toBe('default');
});

it('reset in child rendering rejects its output without moving existing nodes or retiring replacement effects',()=>{
 const context=createContext('default'),source=signal(0),host=document.createElement('div'),node=document.createTextNode('original');host.append(node);let fresh!:Signal<number>,observer!:ReturnType<typeof effect>;const values:number[]=[];
 createRoot(dispose=>{stop=dispose;expect(()=>context.Provider({value:'value',children:()=>{effect(()=>{source();});resetReactiveSystem();fresh=signal(17);observer=effect(()=>{values.push(fresh());});return node;}})).toThrow(/reset/);});expect(node.parentNode).toBe(host);fresh.set(18);expect(values).toEqual([17,18]);observer.dispose();fresh.dispose();source.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('native insertion teardown stops before moving the next node of a provider child array',()=>{
 const context=createContext('default'),host=document.createElement('div'),first=document.createTextNode('first'),second=document.createTextNode('second'),create=document.createDocumentFragment.bind(document);host.append(first,second);let inserted=0;
 createRoot(dispose=>{stop=dispose;vi.spyOn(document,'createDocumentFragment').mockImplementationOnce(()=>{const fragment=create(),append=fragment.appendChild.bind(fragment);vi.spyOn(fragment,'appendChild').mockImplementation(<T extends Node>(node:T):T=>{append(node);inserted++;dispose();return node;});return fragment;});expect(()=>context.Provider({value:'value',children:[first,second]})).toThrow(/disposed/);});expect(inserted).toBe(1);expect(second.parentNode).toBe(host);
});

it.each(['default','outer'])('value-getter effects retain their %s parent context on later runs',parent=>{
 const context=createContext('default'),source=signal(0),metadata:string[]=[],children:string[]=[];
 const render=()=>context.Provider({get value(){effect(()=>{source();metadata.push(useContext(context));});return 'inner';},children:()=>{children.push(useContext(context));return document.createTextNode('child');}});
 createRoot(dispose=>{stop=dispose;if(parent==='outer')context.Provider({value:'outer',children:render});else render();});
 source.set(1);expect(metadata).toEqual([parent,parent]);expect(children).toEqual(['inner']);stop!();source.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});
