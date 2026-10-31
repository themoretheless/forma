/** @vitest-environment jsdom */
import {beforeEach,expect,it} from 'vitest';
import {For,Show,getGraphTelemetry,h,mount,resetReactiveSystem,signal} from '../src/engine';
beforeEach(()=>{resetReactiveSystem();document.body.replaceChildren();});
function group(...nodes:Node[]){const fragment=document.createDocumentFragment();fragment.append(...nodes);return fragment;}

it('replaces fragment ranges with text and preserves unrelated siblings',()=>{
 const a=h('b',null,'A'),b=h('i',null,'B'),value=signal<Node|string>(group(a,b));
 const app=mount(()=>h('div',null,'before:',value,':after'),document.body);
 expect(document.body.textContent).toBe('before:AB:after');value.set('text');
 expect(document.body.textContent).toBe('before:text:after');expect(a.parentNode).toBeNull();expect(b.parentNode).toBeNull();
 app.unmount();value.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('replaces text with a fragment and later replaces every old node',()=>{
 const value=signal<Node|string>('old'),a=h('b',null,'A'),b=h('i',null,'B'),c=h('u',null,'C');
 const app=mount(()=>h('div',null,value),document.body);value.set(group(a,b));expect(document.body.textContent).toBe('AB');
 value.set(group(c));expect(document.body.textContent).toBe('C');expect(document.querySelector('u')).toBe(c);expect(a.parentNode).toBeNull();expect(b.parentNode).toBeNull();
 app.unmount();value.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('empty fragments retain their insertion position across transitions',()=>{
 const value=signal<Node|string>(group()),app=mount(()=>h('div',null,'left',value,'right'),document.body);
 value.set(group(h('span',null,'middle')));expect(document.body.textContent).toBe('leftmiddleright');
 value.set(group());expect(document.body.textContent).toBe('leftright');value.set('again');expect(document.body.textContent).toBe('leftagainright');app.unmount();value.dispose();
});

it('returning the same consumed fragment preserves its existing live range',()=>{
 const tick=signal(0),node=h('b',null,'same'),fragment=group(node);
 const app=mount(()=>h('div',null,()=>{tick();return fragment;}),document.body);tick.set(1);
 expect(document.querySelector('b')).toBe(node);expect(document.body.textContent).toBe('same');app.unmount();tick.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('removal discovers nodes added by nested Show and For views',()=>{
 const show=signal(false),rows=signal(['one']),value=signal<Node|string>('initial');
 const app=mount(()=>{value.set(group(Show({when:show,children:()=>h('b',null,'shown')}),For({each:rows,children:item=>h('i',null,item)})));return h('div',null,value);},document.body);
 show.set(true);rows.set(['one','two']);expect(document.body.textContent).toBe('shownonetwo');value.set('replacement');expect(document.body.textContent).toBe('replacement');
 app.unmount();value.dispose();show.dispose();rows.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('foreign fragments also reconcile their consumed children as a range',()=>{
 const iframe=document.createElement('iframe');document.body.append(iframe);const doc=iframe.contentDocument!,fragment=doc.createDocumentFragment();fragment.append(doc.createTextNode('foreign A'),doc.createTextNode('foreign B'));
 const host=document.createElement('div');document.body.append(host);const value=signal<Node|string>(fragment),app=mount(()=>h('div',null,value),host);
 value.set('local');expect(host.textContent).toBe('local');app.unmount();value.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('broken range boundaries fail before removing unrelated siblings',()=>{
 const value=signal<Node|string>(group(h('b',null,'owned'))),app=mount(()=>h('div',null,value,'unrelated'),document.body),container=document.querySelector('div')!;
 const marker=Array.from(container.childNodes).find(node=>node.nodeType===8&&node.nodeValue==='svet-dyn')!;marker.parentNode!.removeChild(marker);
 expect(()=>value.set('new')).toThrow('Broken dynamic fragment range');expect(container.textContent).toBe('ownedunrelated');
 app.unmount();value.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});
