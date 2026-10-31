/** @vitest-environment jsdom */
import {beforeEach,expect,it} from 'vitest';
import {getGraphTelemetry,h,mount,resetReactiveSystem,signal} from '../src/engine';
beforeEach(()=>{resetReactiveSystem();document.body.replaceChildren();});
function foreignDocument(){const frame=document.createElement('iframe');document.body.append(frame);return frame.contentDocument!;}
function host(){const element=document.createElement('div');document.body.append(element);return element;}

it('inserts foreign nodes without stringification or cloning',()=>{
 const doc=foreignDocument(),button=doc.createElement('button');button.textContent='foreign';let calls=0;button.addEventListener('click',()=>{calls++;});
 expect(button instanceof Node).toBe(false);const target=host(),app=mount(()=>h('div',null,button),target);
 expect(target.querySelector('button')).toBe(button);button.click();expect(calls).toBe(1);expect(button.ownerDocument).toBe(document);
 app.unmount();expect(getGraphTelemetry().nodes).toBe(0);
});

it('reactive children accept foreign nodes and subsequent text/node replacements',()=>{
 const doc=foreignDocument(),first=doc.createElement('b'),second=doc.createElement('i');first.textContent='first';second.textContent='second';
 const child=signal<Node|string>('start'),target=host(),app=mount(()=>h('div',null,child),target);
 child.set(first);expect(target.querySelector('b')).toBe(first);child.set(second);expect(target.querySelector('i')).toBe(second);expect(first.parentNode).toBeNull();
 child.set('end');expect(target.textContent).toBe('end');expect(second.parentNode).toBeNull();app.unmount();child.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('recognizes adopted foreign nodes whose owner document no longer matches their prototype realm',()=>{
 const doc=foreignDocument(),node=doc.createElement('strong');node.textContent='adopted';document.adoptNode(node);
 expect(node.ownerDocument).toBe(document);expect(node instanceof Node).toBe(false);
 const target=host(),app=mount(()=>h('div',null,node),target);expect(target.querySelector('strong')).toBe(node);app.unmount();
});

it('foreign HTML title children receive the same SVG namespace upgrade as local children',()=>{
 const doc=foreignDocument(),title=doc.createElement('title');title.textContent='chart';title.setAttribute('id','label');
 const target=host(),app=mount(()=>h('svg',null,title),target),rendered=target.querySelector('title')!;
 expect(rendered).not.toBeNull();expect(rendered.namespaceURI).toBe('http://www.w3.org/2000/svg');expect(rendered.id).toBe('label');expect(rendered.textContent).toBe('chart');app.unmount();
});

it('node-shaped plain objects are still rendered as text rather than trusted as DOM nodes',()=>{
 const fake={nodeType:1,nodeName:'DIV',hasChildNodes:()=>false,toString:()=> 'plain object'};
 const target=host(),app=mount(()=>h('div',null,fake as unknown as Node),target);expect(target.textContent).toBe('plain object');app.unmount();
});
