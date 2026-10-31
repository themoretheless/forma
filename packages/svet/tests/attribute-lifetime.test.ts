/** @vitest-environment jsdom */
import {afterEach,beforeEach,expect,it,vi} from 'vitest';
import {bindProp,createRoot,effect,getGraphTelemetry,h,mount,resetReactiveSystem,signal} from '../src/engine';
let stop:(()=>void)|undefined;
beforeEach(()=>{resetReactiveSystem();document.body.replaceChildren();});afterEach(()=>{stop?.();stop=undefined;vi.restoreAllMocks();});

it('an attribute resolver cannot publish after disposing its owner',()=>{
 const el=document.createElement('div');createRoot(dispose=>{stop=dispose;expect(()=>bindProp(el,'title',()=>{dispose();return 'stale';})).toThrow(/disposed/);});expect(el.hasAttribute('title')).toBe(false);expect(getGraphTelemetry().nodes).toBe(0);
});

it('graph reset during ordinary attribute coercion rejects stale output and preserves fresh signals',()=>{
 const el=document.createElement('div');let fresh!:ReturnType<typeof signal<number>>;
 createRoot(dispose=>{stop=dispose;expect(()=>bindProp(el,'title',{toString(){resetReactiveSystem();fresh=signal(17);return 'stale';}})).toThrow(/reset/);});expect(el.hasAttribute('title')).toBe(false);expect(fresh()).toBe(17);fresh.dispose();
});

it('value and type coercion cannot write after owner teardown',()=>{
 for(const key of ['value','type']){
  const input=document.createElement('input');createRoot(dispose=>{stop=dispose;expect(()=>bindProp(input,key,{toString(){dispose();return key==='type'?'checkbox':'stale';}})).toThrow(/disposed/);});expect(input.type).toBe('text');expect(input.value).toBe('');
 }
});

it('namespace attribute coercion is guarded before native setAttributeNS',()=>{
 const el=document.createElementNS('http://www.w3.org/2000/svg','use') as unknown as HTMLElement;
 createRoot(dispose=>{stop=dispose;expect(()=>bindProp(el,'xlinkHref',{toString(){dispose();return '#stale';}})).toThrow(/disposed/);});expect(el.hasAttributeNS('http://www.w3.org/1999/xlink','href')).toBe(false);
});

it('a native boolean presence read that invalidates its owner cannot write',()=>{
 const el=document.createElement('button');createRoot(dispose=>{stop=dispose;vi.spyOn(el,'hasAttribute').mockImplementation(()=>{dispose();return false;});expect(()=>bindProp(el,'disabled',true)).toThrow(/disposed/);});expect(el.disabled).toBe(false);
});

it('matching native attribute reads must still reject invalidation',()=>{
 const el=document.createElement('div');el.title='same';createRoot(dispose=>{stop=dispose;vi.spyOn(el,'getAttribute').mockImplementation(()=>{dispose();return 'same';});expect(()=>bindProp(el,'title','same')).toThrow(/disposed/);});
});

it('owner teardown during native setAttribute rejects completion after its single write',()=>{
 const el=document.createElement('div'),native=el.setAttribute.bind(el);createRoot(dispose=>{stop=dispose;vi.spyOn(el,'setAttribute').mockImplementation((...args)=>{native(...args);dispose();});expect(()=>bindProp(el,'title','written while live')).toThrow(/disposed/);});expect(el.title).toBe('written while live');
});

it('failed reactive coercion retires provisional effects and later valid values recover',()=>{
 const source=signal('initial'),nested=signal(0),failure=Error('coercion failed');let runs=0;
 const app=mount(()=>h('div',{title:()=>{const value=source();if(value==='bad'){effect(()=>{nested();runs++;});return {toString(){throw failure;}};}return value;}}),document.body);stop=app.unmount;
 const baseline=getGraphTelemetry().nodes;expect(()=>source.set('bad')).toThrow(failure);expect(getGraphTelemetry().nodes).toBe(baseline);nested.set(1);expect(runs).toBe(1);source.set('recovered');expect(document.querySelector('div')!.title).toBe('recovered');app.unmount();source.dispose();nested.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('static object coercion failure cleans user-created effects without killing its caller',()=>{
 const nested=signal(0),el=document.createElement('div'),failure=Error('coercion failed');let runs=0;
 createRoot(dispose=>{stop=dispose;const baseline=getGraphTelemetry().nodes;expect(()=>bindProp(el,'title',{toString(){effect(()=>{nested();runs++;});throw failure;}})).toThrow(failure);expect(getGraphTelemetry().nodes).toBe(baseline);});nested.set(1);expect(runs).toBe(1);nested.dispose();
});
