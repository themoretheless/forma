/** @vitest-environment jsdom */
import {afterEach,beforeEach,expect,it,vi} from 'vitest';
import {bindProp,createRoot,getGraphTelemetry,h,mount,resetReactiveSystem,signal} from '../src/engine';
let stop:(()=>void)|undefined;
beforeEach(()=>{resetReactiveSystem();document.body.replaceChildren();});afterEach(()=>{stop?.();stop=undefined;vi.restoreAllMocks();});

it('disposal during object property enumeration stops all style writes',()=>{
 const el=document.createElement('div');el.style.color='black';
 createRoot(dispose=>{stop=dispose;expect(()=>bindProp(el,'style',{get color(){dispose();return 'red';},width:'3px'})).toThrow(/disposed/);});
 expect(el.style.color).toBe('black');expect(el.style.width).toBe('');expect(getGraphTelemetry().nodes).toBe(0);
});

it('graph reset during style value coercion cannot publish stale CSS',()=>{
 const el=document.createElement('div');let fresh!:ReturnType<typeof signal<number>>;
 createRoot(dispose=>{stop=dispose;expect(()=>bindProp(el,'style',{color:{toString(){resetReactiveSystem();fresh=signal(13);return 'red';}},width:'3px'})).toThrow(/reset/);});
 expect(el.style.color).toBe('');expect(el.style.width).toBe('');expect(fresh()).toBe(13);fresh.dispose();
});

it('failed static style construction disposes provisional nested subscriptions while caller survives',()=>{
 const el=document.createElement('div'),color=signal('red'),failure=Error('coercion failed');
 createRoot(dispose=>{stop=dispose;const baseline=getGraphTelemetry().nodes;expect(()=>bindProp(el,'style',{color,width:{toString(){throw failure;}}})).toThrow(failure);expect(getGraphTelemetry().nodes).toBe(baseline);});
 color.set('blue');expect(el.style.color).toBe('red');color.dispose();
});

it('disposal during a native property write prevents later properties from publishing',()=>{
 const el=document.createElement('div'),native=el.style.setProperty.bind(el.style);
 createRoot(dispose=>{stop=dispose;vi.spyOn(el.style,'setProperty').mockImplementation((...args)=>{native(...args);dispose();});expect(()=>bindProp(el,'style',{'--first':'one','--second':'two'})).toThrow(/disposed/);});
 expect(el.style.getPropertyValue('--first')).toBe('one');expect(el.style.getPropertyValue('--second')).toBe('');
});

it('a style resolver that disposes its owner cannot write its returned snapshot',()=>{
 const el=document.createElement('div');createRoot(dispose=>{stop=dispose;expect(()=>bindProp(el,'style',()=>{dispose();return 'color:red';})).toThrow(/disposed/);});expect(el.style.cssText).toBe('');expect(getGraphTelemetry().nodes).toBe(0);
});

it('reactive style replacement releases nested accessors and keeps a bounded graph',()=>{
 const first=signal('red'),second=signal('blue'),styles=signal<Record<string,unknown>>({color:first}),app=mount(()=>h('div',{style:styles}),document.body);stop=app.unmount;
 const el=document.querySelector('div')!,baseline=getGraphTelemetry().nodes;
 for(let i=0;i<30;i++){styles.set({color:i%2?first:second});expect(getGraphTelemetry().nodes).toBe(baseline);}
 styles.set({width:'3px'});first.set('green');second.set('yellow');expect(el.style.color).toBe('');expect(el.style.width).toBe('3px');app.unmount();first.dispose();second.dispose();styles.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('stops enumerating value getters immediately after owner invalidation',()=>{
 const el=document.createElement('div'),later=vi.fn(()=>'3px');
 createRoot(dispose=>{stop=dispose;expect(()=>bindProp(el,'style',{get color(){dispose();return 'red';},get width(){return later();}})).toThrow(/disposed/);});expect(later).not.toHaveBeenCalled();
});

it('failed reactive style updates retire provisional nested effects and can recover',()=>{
 const color=signal('red'),styles=signal<Record<string,unknown>>({width:'1px'}),app=mount(()=>h('div',{style:styles}),document.body);stop=app.unmount;
 const el=document.querySelector('div')!,baseline=getGraphTelemetry().nodes,failure=Error('update failed');
 expect(()=>styles.set({color,width:{toString(){throw failure;}}})).toThrow(failure);expect(getGraphTelemetry().nodes).toBe(baseline);
 color.set('blue');expect(el.style.color).toBe('red');styles.set({width:'3px'});expect(el.style.color).toBe('');expect(el.style.width).toBe('3px');app.unmount();color.dispose();styles.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('a matching native style read still rejects owner invalidation',()=>{
 const el=document.createElement('div');el.style.setProperty('--tone','red');const native=el.style.getPropertyValue.bind(el.style);
 createRoot(dispose=>{stop=dispose;vi.spyOn(el.style,'getPropertyValue').mockImplementation(name=>{if(name==='--tone'){dispose();return 'red';}return native(name);});expect(()=>bindProp(el,'style',{'--tone':'red'})).toThrow(/disposed/);});expect(getGraphTelemetry().nodes).toBe(0);
});
