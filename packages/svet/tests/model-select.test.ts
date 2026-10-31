/** @vitest-environment jsdom */
import {beforeEach,expect,it} from 'vitest';
import {For,getGraphTelemetry,h,mount,resetReactiveSystem,signal} from '../src/engine';
beforeEach(()=>{resetReactiveSystem();document.body.replaceChildren();});
const options=()=>['a','b','c'].map(value=>h('option',{value},value));
const selected=(select:HTMLSelectElement)=>Array.from(select.selectedOptions,option=>option.value);
const mutations=async()=>{await Promise.resolve();await Promise.resolve();};

it('single-select applies its initial model after option insertion',()=>{
 const value=signal('b'),app=mount(()=>h('select',{model:value},options()),document.body),select=document.querySelector('select')!;
 expect(select.value).toBe('b');select.value='c';select.dispatchEvent(new Event('change'));expect(value()).toBe('c');
 app.unmount();value.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('multiple-select maps arrays in both directions and ignores input events',()=>{
 const value=signal<readonly string[]>(['a','c']),app=mount(()=>h('select',{model:value,multiple:true},options()),document.body),select=document.querySelector('select')!;
 expect(selected(select)).toEqual(['a','c']);value.set(['b']);expect(selected(select)).toEqual(['b']);
 select.options[0].selected=true;select.options[1].selected=false;select.dispatchEvent(new Event('input'));expect(value()).toEqual(['b']);
 select.dispatchEvent(new Event('change'));expect(value()).toEqual(['a']);
 value.set([]);expect(selected(select)).toEqual([]);app.unmount();value.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('dynamic options reapply the current model after a mutation checkpoint',async()=>{
 const rows=signal(['a']),value=signal('b'),app=mount(()=>h('select',{model:value},For({each:rows,children:item=>h('option',{value:item},item)})),document.body),select=document.querySelector('select')!;
 expect(select.selectedIndex).toBe(-1);rows.set(['a','b']);await mutations();expect(select.value).toBe('b');
 rows.set(['b','a']);await mutations();expect(select.value).toBe('b');
 app.unmount();rows.dispose();value.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('implicit and reactive option value changes refresh multiple selection',async()=>{
 const label=signal('a'),explicit=signal('b'),value=signal(['target']),app=mount(()=>h('select',{multiple:true,model:value},h('option',null,label),h('option',{value:explicit},'explicit')),document.body),select=document.querySelector('select')!;
 expect(selected(select)).toEqual([]);label.set('target');await mutations();expect(selected(select)).toEqual(['target']);
 explicit.set('target');await mutations();expect(selected(select)).toEqual(['target','target']);
 app.unmount();label.dispose();explicit.dispose();value.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('queued option mutations do not revive an unmounted model',async()=>{
 const value=signal(['b']),app=mount(()=>h('select',{multiple:true,model:value},options()),document.body),select=document.querySelector('select')!;
 select.append(h('option',{value:'b'},'late'));app.unmount();value.set(['c']);await mutations();
 select.options[0].selected=true;select.dispatchEvent(new Event('change'));expect(value()).toEqual(['c']);
 value.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('reactive multiple changes reapply the model at the mutation checkpoint',async()=>{
 const multiple=signal(false),value=signal<string|string[]>('b'),app=mount(()=>h('select',{model:value,multiple},options()),document.body),select=document.querySelector('select')!;
 expect(select.value).toBe('b');multiple.set(true);value.set(['a','c']);await mutations();expect(selected(select)).toEqual(['a','c']);
 multiple.set(false);value.set('b');await mutations();expect(select.value).toBe('b');
 app.unmount();multiple.dispose();value.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('queued observers ignore stale graph generations',async()=>{
 const value=signal(['b']),app=mount(()=>h('select',{multiple:true,model:value},options()),document.body),select=document.querySelector('select')!;
 select.append(h('option',{value:'b'},'queued'));resetReactiveSystem();const fresh=signal(7);await mutations();
 expect(fresh()).toBe(7);expect(getGraphTelemetry().nodes).toBe(1);app.unmount();fresh.set(8);expect(fresh()).toBe(8);fresh.dispose();
});
