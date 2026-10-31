/** @vitest-environment jsdom */
import {beforeEach,expect,it} from 'vitest';
import {getGraphTelemetry,h,mount,resetReactiveSystem,signal} from '../src/engine';

beforeEach(()=>{resetReactiveSystem();document.body.replaceChildren();});

it('radio models preserve option values and synchronize a group in both directions',()=>{
  const selected=signal('b');
  const app=mount(()=>h('div',null,...['a','b','c'].map(value=>h('input',{model:selected,value,name:'choices',type:'radio'}))),document.body);
  const inputs=Array.from(document.querySelectorAll('input'));
  expect(inputs.map(input=>input.value)).toEqual(['a','b','c']);expect(inputs.map(input=>input.checked)).toEqual([false,true,false]);
  inputs[2].click();expect(selected()).toBe('c');expect(inputs.map(input=>input.checked)).toEqual([false,false,true]);
  selected.set('a');expect(inputs.map(input=>input.checked)).toEqual([true,false,false]);
  selected.set('missing');expect(inputs.every(input=>!input.checked)).toBe(true);
  app.unmount();inputs[1].checked=true;inputs[1].dispatchEvent(new Event('change'));expect(selected()).toBe('missing');
  selected.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('unchecked radio change events do not replace the selected model',()=>{
  const selected=signal('chosen');
  const app=mount(()=>h('input',{type:'radio',value:'other',model:selected}),document.body),input=document.querySelector('input')!;
  input.checked=false;input.dispatchEvent(new Event('change'));expect(selected()).toBe('chosen');
  input.checked=true;input.dispatchEvent(new Event('input'));expect(selected()).toBe('chosen');
  input.dispatchEvent(new Event('change'));expect(selected()).toBe('other');
  app.unmount();selected.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('reactive radio option values refresh checked state without subscribing to the model',()=>{
  const selected=signal('a'),option=signal('b');let optionReads=0;
  const app=mount(()=>h('input',{model:selected,type:'radio',value:()=>{optionReads++;return option();}}),document.body),input=document.querySelector('input')!;
  expect(input.value).toBe('b');expect(input.checked).toBe(false);
  option.set('a');expect(input.checked).toBe(true);expect(input.value).toBe('a');expect(optionReads).toBe(2);
  selected.set('c');expect(input.checked).toBe(false);expect(input.value).toBe('a');expect(optionReads).toBe(2);
  app.unmount();selected.dispose();option.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('radio models compare string values and leave null models unchecked',()=>{
  const selected=signal<number|null>(2);
  const app=mount(()=>h('input',{type:'radio',value:'2',model:selected}),document.body),input=document.querySelector('input')!;
  expect(input.checked).toBe(true);selected.set(null);expect(input.checked).toBe(false);expect(input.value).toBe('2');
  app.unmount();selected.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});
