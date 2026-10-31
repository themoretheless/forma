/** @vitest-environment jsdom */
import {afterEach,beforeEach,expect,it,vi} from 'vitest';
import {bindProp,createRoot,getCurrentOwner,getGraphTelemetry,h,mount,resetReactiveSystem,signal} from '../src/engine';
import {createWasmCoreInstance,getWasmCore,withWasmCore} from '../src/engine/wasm-bridge';
let stop:(()=>void)|undefined;
beforeEach(()=>{resetReactiveSystem();document.body.replaceChildren();});afterEach(()=>{stop?.();stop=undefined;vi.restoreAllMocks();});

it('graph reset prevents retained delegated callbacks and detaches their document lease',()=>{
 const handler=vi.fn(),app=mount(()=>h('button',{onClick:handler},'old'),document.body);stop=app.unmount;
 const button=document.querySelector('button')!,remove=vi.spyOn(document,'removeEventListener');resetReactiveSystem();const fresh=signal(9);button.click();
 expect(handler).not.toHaveBeenCalled();expect(fresh()).toBe(9);expect(remove.mock.calls.filter(([name])=>name==='click')).toHaveLength(1);fresh.dispose();
});

it('graph reset prevents retained direct callbacks and detaches the native listener',()=>{
 const handler=vi.fn(),app=mount(()=>h('input',{onFocus:handler}),document.body);stop=app.unmount;
 const input=document.querySelector('input')!,remove=vi.spyOn(input,'removeEventListener');resetReactiveSystem();input.dispatchEvent(new Event('focus'));
 expect(handler).not.toHaveBeenCalled();expect(remove.mock.calls.filter(([name])=>name==='focus')).toHaveLength(1);
});

it('reset in one callback suppresses old peers but preserves a freshly registered callback',()=>{
 const calls:string[]=[];let button!:HTMLElement,stopFresh=()=>{};
 createRoot(dispose=>{stop=()=>{stopFresh();dispose();};button=h('button',{onClick:()=>{calls.push('first');resetReactiveSystem();createRoot(finish=>{stopFresh=finish;bindProp(button,'onClick',()=>calls.push('fresh'));});}}) as HTMLElement;bindProp(button,'onClick',()=>calls.push('old peer'));document.body.append(button);});
 button.click();expect(calls).toEqual(['first']);button.click();expect(calls).toEqual(['first','fresh']);
});

it('direct event construction under a disposed owner registers nothing',()=>{
 const input=document.createElement('input'),add=vi.spyOn(input,'addEventListener');
 createRoot(dispose=>{stop=dispose;dispose();expect(()=>bindProp(input,'onFocus',()=>{})).toThrow(/owner.*disposed/i);});expect(add).not.toHaveBeenCalled();
});

it.each(['onClick','onFocus'])('%s callbacks run synchronously in their captured core',key=>{
 const registered=createWasmCoreInstance(),other=createWasmCoreInstance();let input!:HTMLElement,observed:unknown;
 withWasmCore(registered,()=>createRoot(dispose=>{stop=dispose;input=h('input',{[key]:()=>{observed=getWasmCore();}}) as HTMLElement;document.body.append(input);}));
 withWasmCore(other,()=>input.dispatchEvent(new Event(key==='onClick'?'click':'focus',{bubbles:true})));
 expect(observed).toBe(registered);expect(other.svet_get_node_count()).toBe(0);
});

it('direct registration disposal reentry cannot leave a late native callback',()=>{
 const input=document.createElement('input'),native=input.addEventListener.bind(input),handler=vi.fn();
 createRoot(dispose=>{stop=dispose;vi.spyOn(input,'addEventListener').mockImplementation((name,listener,options)=>{dispose();native(name,listener,options);});expect(()=>bindProp(input,'onFocus',handler)).toThrow(/disposed/);});
 input.dispatchEvent(new Event('focus'));expect(handler).not.toHaveBeenCalled();expect(getGraphTelemetry().nodes).toBe(0);
});

it('failed direct registration removes its callback and provisional cleanup hook',()=>{
 const input=document.createElement('input'),native=input.addEventListener.bind(input),handler=vi.fn();
 vi.spyOn(input,'addEventListener').mockImplementation((name,listener,options)=>{native(name,listener,options);throw Error('native registration failed');});
 createRoot(dispose=>{stop=dispose;const owner=getCurrentOwner()!,baseline=owner.disposables.length;expect(()=>bindProp(input,'onFocus',handler)).toThrow('native registration failed');expect(owner.disposables).toHaveLength(baseline);});
 input.dispatchEvent(new Event('focus'));expect(handler).not.toHaveBeenCalled();
});
