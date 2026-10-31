/** @vitest-environment jsdom */
import {beforeEach,expect,it} from 'vitest';
import {disposeResumedContainer,getGraphTelemetry,resetReactiveSystem,resumeContainer} from '../src/engine';
beforeEach(()=>{resetReactiveSystem();document.body.replaceChildren();});
const snapshot=(signals:Record<string,unknown>,bindings?:unknown[])=>JSON.stringify({signals,bindings,timestamp:0});
function container(html:string){const element=document.createElement('div');element.innerHTML=html;document.body.append(element);return element;}
const finish=(element:HTMLElement)=>{disposeResumedContainer(element);expect(getGraphTelemetry().nodes).toBe(0);};

it('resumed checkbox models use checked and boolean change values',()=>{
 const host=container('<input type="checkbox" checked data-svet-model="value">'),state=resumeContainer(host,snapshot({value:false})),input=host.querySelector('input')!;
 expect(input.checked).toBe(false);input.checked=true;input.dispatchEvent(new Event('change'));expect(state.value()).toBe(true);finish(host);
});

it('resumed numeric models preserve number and empty-input conversions',()=>{
 const host=container('<input type="number" data-svet-model="value">'),state=resumeContainer(host,snapshot({value:12})),input=host.querySelector('input')!;
 input.value='3.5';input.dispatchEvent(new Event('input'));expect(state.value()).toBe(3.5);input.value='';input.dispatchEvent(new Event('input'));expect(state.value()).toBe(0);finish(host);
});

it('resumed radio groups keep option values and synchronize selection',()=>{
 const host=container('<input type="radio" name="choice" value="a" data-svet-model="value"><input type="radio" name="choice" value="b" data-svet-model="value">'),state=resumeContainer(host,snapshot({value:'b'})),[a,b]=Array.from(host.querySelectorAll('input'));
 expect([a.value,b.value,a.checked,b.checked]).toEqual(['a','b',false,true]);a.click();expect(state.value()).toBe('a');expect(b.checked).toBe(false);finish(host);
});

it('resumed multiple selects support arrays and later option insertion',async()=>{
 const host=container('<select multiple data-svet-model="value"><option value="a">A</option><option value="b">B</option><option value="c">C</option></select>'),state=resumeContainer(host,snapshot({value:['a','c']})),select=host.querySelector('select')!;
 expect(Array.from(select.selectedOptions,option=>option.value)).toEqual(['a','c']);select.options[0].selected=false;select.options[1].selected=true;select.dispatchEvent(new Event('change'));expect(state.value()).toEqual(['b','c']);
 state.value.set(['x']);const option=document.createElement('option');option.value='x';select.append(option);await Promise.resolve();await Promise.resolve();expect(option.selected).toBe(true);finish(host);
});

it('resumed text models defer IME composition and remove listeners on disposal',()=>{
 const host=container('<input data-svet-model="value">'),state=resumeContainer(host,snapshot({value:'old'})),input=host.querySelector('input')!;
 input.dispatchEvent(new CompositionEvent('compositionstart'));input.value='途中';input.dispatchEvent(new InputEvent('input',{isComposing:true}));expect(state.value()).toBe('old');input.value='完成';input.dispatchEvent(new CompositionEvent('compositionend'));expect(state.value()).toBe('完成');
 finish(host);input.value='late';input.dispatchEvent(new Event('input'));expect(state.value()).toBe('完成');
});

it('resumed attributes use presence booleans, explicit tokens and null removal',()=>{
 const host=container('<button id="toggle" disabled>toggle</button>'),state=resumeContainer(host,snapshot({value:false},['disabled','aria-pressed','data-ready','contenteditable'].map(attrName=>({elementId:'toggle',signal:'value',type:'attr',attrName})))),button=host.querySelector('button')!;
 expect(button.disabled).toBe(false);for(const name of ['aria-pressed','data-ready','contenteditable'])expect(button.getAttribute(name)).toBe('false');state.value.set(null);for(const name of ['disabled','aria-pressed','data-ready','contenteditable'])expect(button.hasAttribute(name)).toBe(false);finish(host);
});

it('resumed type attributes refresh model output without a model value change',()=>{
 const host=container('<input id="field" type="text">'),state=resumeContainer(host,snapshot({value:true,type:'text'},[{elementId:'field',signal:'value',type:'model'},{elementId:'field',signal:'type',type:'attr',attrName:'type'}])),input=host.querySelector('input')!;
 state.type.set('checkbox');expect(input.type).toBe('checkbox');expect(input.checked).toBe(true);finish(host);
});

it('attribute bindings named model or ref remain literal attributes',()=>{
 const host=container('<input id="field">');resumeContainer(host,snapshot({value:'literal'},['model','ref','children'].map(attrName=>({elementId:'field',signal:'value',type:'attr',attrName}))));const input=host.querySelector('input')!;
 for(const name of ['model','ref','children'])expect(input.getAttribute(name)).toBe('literal');finish(host);
});

it('shared attribute application preserves resumption event-name and URL filtering',()=>{
 const host=container('<a id="link" href="/safe">link</a>'),state=resumeContainer(host,snapshot({value:'javascript:alert(1)'},['href','onclick','onClick','bad name'].map(attrName=>({elementId:'link',signal:'value',type:'attr',attrName})))),link=host.querySelector('a')!;
 expect(link.getAttribute('href')).toBe('/safe');expect(link.hasAttribute('onclick')).toBe(false);
 state.value.set('/updated');expect(link.getAttribute('href')).toBe('/updated');expect(link.hasAttribute('onclick')).toBe(false);
 state.value.set(' \n javascript:alert(1)');expect(link.getAttribute('href')).toBe('/updated');finish(host);
});
