/** @vitest-environment jsdom */
import {beforeEach,expect,it} from 'vitest';
import {getGraphTelemetry,h,mount,renderToString,resetReactiveSystem,signal} from '../src/engine';
beforeEach(()=>{resetReactiveSystem();document.body.replaceChildren();});

it.each(['contentEditable','draggable','spellCheck'])('%s preserves explicit boolean tokens',name=>{
 const disabled=h('div',{[name]:false}) as HTMLElement,enabled=h('div',{[name]:true}) as HTMLElement;
 expect(disabled.getAttribute(name.toLowerCase())).toBe('false');expect(enabled.getAttribute(name.toLowerCase())).toBe('true');
 if(name==='draggable'){expect(disabled.draggable).toBe(false);expect(enabled.draggable).toBe(true);}
});

it('translate booleans become yes/no while string values remain unchanged',()=>{
 const enabled=h('div',{translate:true}) as HTMLElement,disabled=h('div',{translate:false}) as HTMLElement,explicit=h('div',{translate:'no'}) as HTMLElement;
 expect(enabled.getAttribute('translate')).toBe('yes');expect(disabled.getAttribute('translate')).toBe('no');expect(explicit.getAttribute('translate')).toBe('no');
});

it('reactive enumerated attributes distinguish disabled values from absent attributes',()=>{
 const enabled=signal<boolean|null>(false),app=mount(()=>h('div',{contentEditable:enabled,draggable:enabled,spellCheck:enabled,translate:enabled}),document.body),element=document.querySelector('div')!;
 for(const name of ['contenteditable','draggable','spellcheck'])expect(element.getAttribute(name)).toBe('false');expect(element.getAttribute('translate')).toBe('no');
 enabled.set(true);for(const name of ['contenteditable','draggable','spellcheck'])expect(element.getAttribute(name)).toBe('true');expect(element.getAttribute('translate')).toBe('yes');
 enabled.set(null);for(const name of ['contenteditable','draggable','spellcheck','translate'])expect(element.hasAttribute(name)).toBe(false);
 app.unmount();enabled.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('SSR preserves enumerated attribute tokens with native boolean attributes',()=>{
 const {html}=renderToString(()=>h('input',{contentEditable:false,draggable:true,spellCheck:false,translate:false,disabled:true,readOnly:false})),parsed=document.createElement('div');parsed.innerHTML=html;const input=parsed.querySelector('input')!;
 expect(input.getAttribute('contenteditable')).toBe('false');expect(input.draggable).toBe(true);expect(input.getAttribute('spellcheck')).toBe('false');expect(input.getAttribute('translate')).toBe('no');expect(input.disabled).toBe(true);expect(input.readOnly).toBe(false);expect(getGraphTelemetry().nodes).toBe(0);
});
