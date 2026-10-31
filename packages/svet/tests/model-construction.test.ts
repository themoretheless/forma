/** @vitest-environment jsdom */
import {afterEach,beforeEach,expect,it,vi} from 'vitest';
import {bindProp,createRoot,getGraphTelemetry,resetReactiveSystem,signal} from '../src/engine';
beforeEach(()=>{resetReactiveSystem();document.body.replaceChildren();});afterEach(()=>vi.restoreAllMocks());

it('failed listener installation releases its effect and already installed handlers inside a live owner',()=>{
 const value=signal('before'),input=document.createElement('input'),native=input.addEventListener.bind(input),failure=Error('listener failed');let stop!:()=>void;
 const add=vi.spyOn(input,'addEventListener').mockImplementation((type,listener,options)=>{if(type==='change')throw failure;native(type,listener,options);});
 createRoot(dispose=>{stop=dispose;expect(()=>bindProp(input,'model',value)).toThrow(failure);expect(getGraphTelemetry().nodes).toBe(1);});
 add.mockRestore();input.value='late';input.dispatchEvent(new Event('input'));expect(value()).toBe('before');stop();value.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('listener insertion after owner teardown is removed even when cleanup ran before native add',()=>{
 const value=signal('before'),input=document.createElement('input'),native=input.addEventListener.bind(input);let stop!:()=>void;
 const add=vi.spyOn(input,'addEventListener').mockImplementation((type,listener,options)=>{if(type==='input')stop();native(type,listener,options);});
 createRoot(dispose=>{stop=dispose;expect(()=>bindProp(input,'model',value)).toThrow('disposed');});
 add.mockRestore();input.value='late';input.dispatchEvent(new Event('input'));expect(value()).toBe('before');value.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('initial value writes cannot finish construction after their owner is disposed',()=>{
 const value=signal('before'),input=document.createElement('input'),descriptor=Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,'value')!;let stop!:()=>void;
 Object.defineProperty(input,'value',{configurable:true,get(){return descriptor.get!.call(this);},set(next){stop();descriptor.set!.call(this,next);}});
 createRoot(dispose=>{stop=dispose;expect(()=>bindProp(input,'model',value)).toThrow('disposed');});
 delete (input as any).value;input.value='late';input.dispatchEvent(new Event('input'));expect(value()).toBe('before');value.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('graph reset during initial output rejects stale setup and preserves new signals',()=>{
 const value=signal('before'),input=document.createElement('input'),descriptor=Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,'value')!;let fresh!:ReturnType<typeof signal<number>>,stop!:()=>void;
 Object.defineProperty(input,'value',{configurable:true,get(){return descriptor.get!.call(this);},set(next){resetReactiveSystem();fresh=signal(7);descriptor.set!.call(this,next);}});
 createRoot(dispose=>{stop=dispose;expect(()=>bindProp(input,'model',value)).toThrow('reset');});
 delete (input as any).value;stop();value.dispose();expect(fresh()).toBe(7);fresh.set(8);expect(fresh()).toBe(8);fresh.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('type changes after reset ignore and detach stale model synchronizers',()=>{
 const value=signal('before'),input=document.createElement('input');let stop!:()=>void;
 createRoot(dispose=>{stop=dispose;bindProp(input,'model',value);});resetReactiveSystem();const fresh=signal(10);
 expect(()=>bindProp(input,'type','checkbox')).not.toThrow();expect(input.checked).toBe(false);input.checked=true;input.dispatchEvent(new Event('change'));expect(fresh()).toBe(10);
 stop();value.dispose();fresh.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('failed observer setup disconnects queued selection work and releases the partial model',async()=>{
 const value=signal(['a']),select=document.createElement('select');select.multiple=true;select.innerHTML='<option value="a">A</option><option value="b">B</option>';const native=MutationObserver.prototype.observe,failure=Error('observe failed');let stop!:()=>void;
 const observe=vi.spyOn(MutationObserver.prototype,'observe').mockImplementation(function(this:MutationObserver,...args:Parameters<MutationObserver['observe']>){native.apply(this,args);throw failure;});
 createRoot(dispose=>{stop=dispose;expect(()=>bindProp(select,'model',value)).toThrow(failure);expect(getGraphTelemetry().nodes).toBe(1);});
 observe.mockRestore();select.options[0].selected=false;select.options[1].selected=true;select.append(document.createElement('option'));await Promise.resolve();await Promise.resolve();expect(select.options[1].selected).toBe(true);
 stop();value.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('cleanup errors cannot mask the installation failure or leave active callbacks',()=>{
 const value=signal('before'),input=document.createElement('input'),native=input.addEventListener.bind(input),failure=Error('install failed');let stop!:()=>void;
 const add=vi.spyOn(input,'addEventListener').mockImplementation((type,listener,options)=>{if(type==='change')throw failure;native(type,listener,options);});
 const remove=vi.spyOn(input,'removeEventListener').mockImplementation(()=>{throw Error('remove failed');});
 createRoot(dispose=>{stop=dispose;expect(()=>bindProp(input,'model',value)).toThrow(failure);expect(getGraphTelemetry().nodes).toBe(1);});
 input.value='late';input.dispatchEvent(new Event('input'));expect(value()).toBe('before');add.mockRestore();remove.mockRestore();stop();value.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});
