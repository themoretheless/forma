/** @vitest-environment jsdom */
import {beforeEach,expect,it} from 'vitest';
import {getGraphTelemetry,onCleanup,renderToString,resetReactiveSystem,serializeDomNodeToHtml} from '../src/engine';
beforeEach(()=>resetReactiveSystem());
function deepTree(depth:number){const root=document.createElement('div');let leaf=root;for(let i=1;i<depth;i++){const child=document.createElement('div');leaf.append(child);leaf=child;}leaf.append(document.createTextNode('leaf & <text>'));return root;}

it('serializes a real deeply nested DOM tree without recursive stack frames',()=>{
 const depth=4000,root=deepTree(depth);
 expect(serializeDomNodeToHtml(root)).toBe('<div>'.repeat(depth)+'leaf &amp; &lt;text&gt;'+'</div>'.repeat(depth));
},20000);

it('SSR releases its request scope after serializing a deep view',()=>{
 const depth=3000,root=deepTree(depth);let cleanups=0;
 const result=renderToString(()=>{onCleanup(()=>{cleanups++;});return root;});
 expect(result.html).toBe('<div>'.repeat(depth)+'leaf &amp; &lt;text&gt;'+'</div>'.repeat(depth));expect(cleanups).toBe(1);expect(getGraphTelemetry().nodes).toBe(0);
},20000);

it('iterative traversal preserves fragment sibling order, comments and empty elements',()=>{
 const fragment=document.createDocumentFragment(),a=document.createElement('p'),b=document.createElement('section');a.textContent='first';b.append(document.createElement('span'));
 fragment.append(document.createTextNode('start'),a,document.createComment('svet-dyn'),document.createComment('keep --> safe'),b,document.createTextNode('end'));
 expect(serializeDomNodeToHtml(fragment)).toBe('start<p>first</p><!--keep --&gt; safe--><section><span></span></section>end');
});

it('wide siblings preserve order together with live form and void-element output',()=>{
 const fragment=document.createDocumentFragment();for(let i=0;i<1000;i++){const span=document.createElement('span');span.textContent=String(i);fragment.append(span);}
 const input=document.createElement('input');input.type='checkbox';input.setAttribute('checked','');input.checked=false;input.value='live & value';fragment.append(input);
 const textarea=document.createElement('textarea');textarea.value='\n<&';fragment.append(textarea,document.createElement('br'));
 expect(serializeDomNodeToHtml(fragment)).toBe(Array.from({length:1000},(_,i)=>`<span>${i}</span>`).join('')+'<input type="checkbox" value="live &amp; value" /><textarea>\n\n&lt;&amp;</textarea><br />');
});
