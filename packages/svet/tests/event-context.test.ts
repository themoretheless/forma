/** @vitest-environment jsdom */
import {afterEach,beforeEach,expect,it,vi} from 'vitest';
import {bindProp,createContext,createRoot,effect,getCurrentOwner,getGraphTelemetry,h,mount,onCleanup,resetReactiveSystem,signal,useContext,type Signal} from '../src/engine';
import {createWasmCoreInstance,getWasmCore,installAsyncCoreResolver,withWasmCore} from '../src/engine/wasm-bridge';
let stops:Array<()=>void>=[],sources:Array<Signal<any>>=[];
beforeEach(()=>{resetReactiveSystem();document.body.replaceChildren();});
afterEach(()=>{installAsyncCoreResolver(()=>undefined);for(const stop of stops.reverse())stop();for(const source of sources)source.dispose();stops=[];sources=[];vi.restoreAllMocks();});
const kinds=[['onClick','click'],['onFocus','focus']] as const;
it.each(kinds)('%s callbacks enter their bound provider and restore outside context', (prop,eventName)=>{
 const context=createContext('default'),values:string[]=[];let button!:HTMLElement,hasOwner=false;createRoot(stop=>{stops.push(stop);context.Provider({value:'bound',children:()=>button=h('button',{[prop](){hasOwner=getCurrentOwner()!==null;values.push(useContext(context));}}) as HTMLElement});document.body.append(button);});button.dispatchEvent(new Event(eventName,{bubbles:true}));expect(values).toEqual(['bound']);expect(hasOwner).toBe(true);expect(useContext(context)).toBe('default');expect(getCurrentOwner()).toBeNull();
});
it.each(kinds)('%s callback effects and cleanups belong to their bound view', (prop,eventName)=>{
 const source=signal(0);sources.push(source);let button!:HTMLElement,runs=0,cleaned=0,dispose!:()=>void;createRoot(stop=>{dispose=stop;stops.push(stop);button=h('button',{[prop](){effect(()=>{source();runs++;});onCleanup(()=>{cleaned++;});}}) as HTMLElement;document.body.append(button);});button.dispatchEvent(new Event(eventName,{bubbles:true}));dispose();source.set(1);expect([runs,cleaned]).toEqual([1,1]);expect(getGraphTelemetry().nodes).toBe(1);
});
it.each(kinds)('%s reads do not subscribe an effect that synchronously dispatches the event', (prop,eventName)=>{
 const source=signal(0),trigger=signal(0);sources.push(source,trigger);let button!:HTMLElement,runs=0;createRoot(stop=>{stops.push(stop);button=h('button',{[prop](){source();}}) as HTMLElement;document.body.append(button);effect(()=>{trigger();runs++;button.dispatchEvent(new Event(eventName,{bubbles:true}));});});source.set(1);expect(runs).toBe(1);trigger.set(1);expect(runs).toBe(2);
});
it.each(kinds)('%s uses the bound owner rather than a different dispatching owner', (prop,eventName)=>{
 const context=createContext('default'),source=signal(0);sources.push(source);const values:string[]=[];let button!:HTMLElement,runs=0,stopCaller!:()=>void;
 createRoot(stop=>{stops.push(stop);context.Provider({value:'bound',children:()=>button=h('button',{[prop](){values.push(useContext(context));effect(()=>{source();runs++;});}}) as HTMLElement});document.body.append(button);});
 createRoot(stop=>{stopCaller=stop;stops.push(stop);context.Provider({value:'caller',children:()=>{button.dispatchEvent(new Event(eventName,{bubbles:true}));values.push(useContext(context));return document.createTextNode('caller');}});});stopCaller();source.set(1);expect(values).toEqual(['bound','caller']);expect(runs).toBe(2);
});
it('callback errors restore the caller owner and keep created resources owned until view teardown',()=>{
 const context=createContext('default'),source=signal(0),button=document.createElement('button'),failure=Error('handler failed'),add=vi.spyOn(button,'addEventListener');sources.push(source);let seen='',runs=0,dispose!:()=>void;
 createRoot(stop=>{dispose=stop;stops.push(stop);context.Provider({value:'bound',children:()=>{const node=h('span',null);const callback=()=>{seen=useContext(context);effect(()=>{source();runs++;});throw failure;};bindFocus(button,callback);return node;}});});
 const handler=add.mock.calls.find(([name])=>name==='focus')![1] as EventListener;
 createRoot(stop=>{stops.push(stop);context.Provider({value:'caller',children:()=>{const before=getCurrentOwner();expect(()=>handler.call(button,new Event('focus'))).toThrow(failure);expect(getCurrentOwner()).toBe(before);expect(useContext(context)).toBe('caller');return document.createTextNode('caller');}});});source.set(1);expect(seen).toBe('bound');expect(runs).toBe(2);dispose();source.set(2);expect(runs).toBe(2);
});
it('foreign-core dispatch pauses caller tracking and restores its core',()=>{
 const owned=getWasmCore(),caller=createWasmCoreInstance(),button=document.createElement('button'),add=vi.spyOn(button,'addEventListener');let callerSource!:Signal<number>,runs=0,seen:unknown;
 withWasmCore(caller,()=>{callerSource=signal(0);sources.push(callerSource);});createRoot(stop=>{stops.push(stop);bindFocus(button,()=>{seen=getWasmCore();callerSource();});});const handler=add.mock.calls.find(([name])=>name==='focus')![1] as EventListener;
 withWasmCore(caller,()=>createRoot(stop=>{stops.push(stop);effect(()=>{runs++;handler.call(button,new Event('focus'));});expect(getWasmCore()).toBe(caller);}));callerSource.set(1);expect(runs).toBe(1);expect(seen).toBe(owned);
});
it('captured event dispatch remains possible when an ambient request resolver rejects work',()=>{
 const owned=getWasmCore(),button=document.createElement('button'),add=vi.spyOn(button,'addEventListener');let calls=0;createRoot(stop=>{stops.push(stop);bindFocus(button,()=>{expect(getWasmCore()).toBe(owned);calls++;});});const handler=add.mock.calls.find(([name])=>name==='focus')![1] as EventListener;installAsyncCoreResolver(()=>{throw Error('request closed');});try{expect(()=>handler.call(button,new Event('focus'))).not.toThrow();expect(calls).toBe(1);}finally{installAsyncCoreResolver(()=>undefined);}
});
function bindFocus(node:HTMLElement,callback:()=>void){bindProp(node,'onFocus',callback);}
it('nested direct/delegated callbacks restore each bound provider in turn',()=>{
 const context=createContext('default'),values:string[]=[];let outer!:HTMLElement,inner!:HTMLElement;createRoot(stop=>{stops.push(stop);context.Provider({value:'inner',children:()=>inner=h('button',{onFocus(){values.push(useContext(context));}}) as HTMLElement});context.Provider({value:'outer',children:()=>outer=h('button',{onClick(){values.push(useContext(context));inner.dispatchEvent(new Event('focus'));values.push(useContext(context));}}) as HTMLElement});document.body.append(outer,inner);});outer.click();expect(values).toEqual(['outer','inner','outer']);expect(useContext(context)).toBe('default');
});
it('throwing foreign callbacks restore caller tracking without collecting callback reads',()=>{
 const owned=getWasmCore(),caller=createWasmCoreInstance(),button=document.createElement('button'),add=vi.spyOn(button,'addEventListener'),failure=Error('foreign callback failed');let callbackSource!:Signal<number>,after!:Signal<number>,runs=0,failures=0;
 withWasmCore(caller,()=>{callbackSource=signal(0);after=signal(0);sources.push(callbackSource,after);});createRoot(stop=>{stops.push(stop);bindFocus(button,()=>{expect(getWasmCore()).toBe(owned);callbackSource();throw failure;});});const handler=add.mock.calls.find(([name])=>name==='focus')![1] as EventListener;
 withWasmCore(caller,()=>createRoot(stop=>{stops.push(stop);effect(()=>{runs++;try{handler.call(button,new Event('focus'));}catch(error){if(error===failure)failures++;else throw error;}after();});}));callbackSource.set(1);expect([runs,failures]).toEqual([1,1]);after.set(1);expect([runs,failures]).toEqual([2,2]);
});

it('nodes created by a shadow event inherit the bound mount routing and lifetime',()=>{
 const host=document.createElement('div');document.body.append(host);const root=host.attachShadow({mode:'closed'});let child!:HTMLButtonElement,calls=0;const app=mount(()=>h('button',{onClick(){child=h('button',{onClick(){calls++;}},'child') as HTMLButtonElement;root.append(child);}},'parent'),root);stops.push(app.unmount);root.firstChild!.dispatchEvent(new MouseEvent('click',{bubbles:true,composed:false}));child.dispatchEvent(new MouseEvent('click',{bubbles:true,composed:false}));expect(calls).toBe(1);app.unmount();child.dispatchEvent(new MouseEvent('click',{bubbles:true,composed:false}));expect(calls).toBe(1);
});
