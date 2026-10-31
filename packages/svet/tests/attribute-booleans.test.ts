/** @vitest-environment jsdom */
import {beforeEach,expect,it} from 'vitest';
import {getGraphTelemetry,h,mount,renderToString,resetReactiveSystem,signal} from '../src/engine';
beforeEach(()=>{resetReactiveSystem();document.body.replaceChildren();});

it('serializes ARIA booleans as explicit tokens rather than native boolean attributes',()=>{
 const button=h('button',{'aria-pressed':false,'aria-expanded':true,'aria-label':'Menu'}) as HTMLElement;
 expect(button.getAttribute('aria-pressed')).toBe('false');expect(button.getAttribute('aria-expanded')).toBe('true');expect(button.getAttribute('aria-label')).toBe('Menu');
});

it('preserves data boolean values in dataset',()=>{
 const element=h('div',{'data-enabled':false,'data-ready':true}) as HTMLElement;
 expect(element.dataset.enabled).toBe('false');expect(element.dataset.ready).toBe('true');
});

it('reactive ARIA and data values distinguish false from absent',()=>{
 const value=signal<boolean|null>(false),app=mount(()=>h('button',{'aria-pressed':value,'data-active':value}),document.body),button=document.querySelector('button')!;
 expect(button.getAttribute('aria-pressed')).toBe('false');expect(button.dataset.active).toBe('false');value.set(true);expect(button.getAttribute('aria-pressed')).toBe('true');expect(button.dataset.active).toBe('true');
 value.set(null);expect(button.hasAttribute('aria-pressed')).toBe(false);expect(button.hasAttribute('data-active')).toBe(false);value.set(false);expect(button.getAttribute('aria-pressed')).toBe('false');
 app.unmount();value.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('native boolean attributes retain presence and removal behavior',()=>{
 const disabled=signal(false),app=mount(()=>h('button',{disabled,'aria-disabled':disabled}),document.body),button=document.querySelector('button')!;
 expect(button.disabled).toBe(false);expect(button.getAttribute('aria-disabled')).toBe('false');disabled.set(true);expect(button.disabled).toBe(true);expect(button.getAttribute('disabled')).toBe('');expect(button.getAttribute('aria-disabled')).toBe('true');
 disabled.set(false);expect(button.hasAttribute('disabled')).toBe(false);app.unmount();disabled.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('SVG ARIA and data attributes also preserve boolean tokens',()=>{
 const svg=h('svg',{'aria-hidden':false,'data-interactive':true}) as Element;
 expect(svg.namespaceURI).toBe('http://www.w3.org/2000/svg');expect(svg.getAttribute('aria-hidden')).toBe('false');expect(svg.getAttribute('data-interactive')).toBe('true');
});

it('SSR retains explicit false and true attributes in generated HTML',()=>{
 const result=renderToString(()=>h('button',{'aria-pressed':false,'data-ready':true},'toggle')),parsed=document.createElement('div');parsed.innerHTML=result.html;
 const button=parsed.querySelector('button')!;expect(button.getAttribute('aria-pressed')).toBe('false');expect(button.dataset.ready).toBe('true');expect(getGraphTelemetry().nodes).toBe(0);
});
