/** @vitest-environment jsdom */
import {beforeEach,expect,it} from 'vitest';
import {getGraphTelemetry,h,mount,resetReactiveSystem,signal} from '../src/engine';

beforeEach(()=>{resetReactiveSystem();document.body.replaceChildren();});

it('reactive input types switch model output and accepted events',()=>{
  const type=signal('text'),value=signal<string|boolean|number>('hello');
  const app=mount(()=>h('input',{model:value,type}),document.body),input=document.querySelector('input')!;
  expect(input.type).toBe('text');expect(input.value).toBe('hello');
  value.set(true);type.set('checkbox');expect(input.type).toBe('checkbox');expect(input.checked).toBe(true);
  input.checked=false;input.dispatchEvent(new Event('change'));expect(value()).toBe(false);
  value.set(12);type.set('number');expect(input.type).toBe('number');expect(input.value).toBe('12');
  input.value='3.5';input.dispatchEvent(new Event('input'));expect(value()).toBe(3.5);
  type.set('text');input.value='new';input.dispatchEvent(new Event('input'));expect(value()).toBe('new');
  app.unmount();type.dispose();value.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('type changes reapply the model even when the model value is unchanged',()=>{
  const type=signal('text'),value=signal(true);
  const app=mount(()=>h('input',{type,model:value}),document.body),input=document.querySelector('input')!;
  expect(input.value).toBe('true');type.set('checkbox');expect(input.checked).toBe(true);
  app.unmount();type.dispose();value.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('model events follow the live native input type after external changes',()=>{
  const value=signal<string|number|boolean>('initial');
  const app=mount(()=>h('input',{type:'text',model:value}),document.body),input=document.querySelector('input')!;
  input.type='number';input.value='14';input.dispatchEvent(new Event('input'));expect(value()).toBe(14);
  input.type='checkbox';input.checked=true;input.dispatchEvent(new Event('change'));expect(value()).toBe(true);
  app.unmount();input.checked=false;input.dispatchEvent(new Event('change'));expect(value()).toBe(true);
  value.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('static text checkbox and number models retain their existing conversions',()=>{
  const text=signal('a'),checked=signal(false),number=signal(4);
  const app=mount(()=>h('div',null,h('input',{model:text}),h('input',{type:'checkbox',model:checked}),h('input',{type:'number',model:number})),document.body);
  const [a,b,c]=Array.from(document.querySelectorAll('input'));
  a.value='b';a.dispatchEvent(new Event('input'));b.checked=true;b.dispatchEvent(new Event('change'));c.value='';c.dispatchEvent(new Event('input'));
  expect([text(),checked(),number()]).toEqual(['b',true,0]);
  app.unmount();text.dispose();checked.dispose();number.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('type refresh does not subscribe the type binding to model values',()=>{
  const type=signal('text'),value=signal<string|boolean>('start');let typeReads=0;
  const app=mount(()=>h('input',{type:()=>{typeReads++;return type();},model:value}),document.body),input=document.querySelector('input')!;
  expect(typeReads).toBe(1);type.set('checkbox');expect(typeReads).toBe(2);
  value.set(false);expect(typeReads).toBe(2);expect(input.checked).toBe(false);
  input.checked=true;input.dispatchEvent(new Event('input'));expect(value()).toBe(false);
  input.dispatchEvent(new Event('change'));expect(value()).toBe(true);expect(typeReads).toBe(2);
  app.unmount();type.dispose();value.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});
