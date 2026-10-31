/** @vitest-environment jsdom */
import {beforeEach,expect,it} from 'vitest';
import {effect,getGraphTelemetry,h,mount,resetReactiveSystem,signal} from '../src/engine';
beforeEach(()=>{resetReactiveSystem();document.body.replaceChildren();});

it.each(['input','textarea'])('%s publishes composition only at completion',tag=>{
 const value=signal('before'),seen:string[]=[];
 const app=mount(()=>{effect(()=>{seen.push(value());});return h(tag,{model:value});},document.body),input=document.querySelector(tag)! as HTMLInputElement;
 input.dispatchEvent(new CompositionEvent('compositionstart'));input.value='仮';input.dispatchEvent(new InputEvent('input',{isComposing:true}));
 expect(value()).toBe('before');expect(seen).toEqual(['before']);
 input.value='仮名';input.dispatchEvent(new Event('input'));expect(value()).toBe('before');
 input.dispatchEvent(new CompositionEvent('compositionend'));expect(value()).toBe('仮名');expect(seen).toEqual(['before','仮名']);
 input.dispatchEvent(new InputEvent('input'));expect(seen).toEqual(['before','仮名']);
 app.unmount();value.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('external model writes do not overwrite active composing DOM and composition wins at completion',()=>{
 const value=signal('initial'),app=mount(()=>h('input',{model:value}),document.body),input=document.querySelector('input')!;
 input.dispatchEvent(new CompositionEvent('compositionstart'));input.value='途中';value.set('external');
 expect(value()).toBe('external');expect(input.value).toBe('途中');
 input.value='完成';input.dispatchEvent(new CompositionEvent('compositionend'));expect(value()).toBe('完成');
 value.set('later');expect(input.value).toBe('later');app.unmount();value.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('an isComposing input is ignored even without a compositionstart event',()=>{
 const value=signal('before'),app=mount(()=>h('input',{model:value}),document.body),input=document.querySelector('input')!;
 input.value='partial';input.dispatchEvent(new InputEvent('input',{isComposing:true}));expect(value()).toBe('before');
 input.value='final';input.dispatchEvent(new InputEvent('input'));expect(value()).toBe('final');
 app.unmount();value.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('type switching to a checkbox during composition does not publish stale text at completion',()=>{
 const type=signal('text'),value=signal<string|boolean>('before'),app=mount(()=>h('input',{type,model:value}),document.body),input=document.querySelector('input')!;
 input.dispatchEvent(new CompositionEvent('compositionstart'));input.value='partial';type.set('checkbox');value.set(true);
 expect(input.checked).toBe(true);input.dispatchEvent(new CompositionEvent('compositionend'));expect(value()).toBe(true);
 app.unmount();type.dispose();value.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('retained composition listeners cannot write after unmount',()=>{
 const value=signal('before'),app=mount(()=>h('input',{model:value}),document.body),input=document.querySelector('input')!;
 input.dispatchEvent(new CompositionEvent('compositionstart'));input.value='late';app.unmount();input.dispatchEvent(new CompositionEvent('compositionend'));input.dispatchEvent(new Event('input'));
 expect(value()).toBe('before');value.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});
