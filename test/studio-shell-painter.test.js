import {test} from 'node:test';
import assert from 'node:assert/strict';
import {createShellPainter} from '../src/studio-shell-painter.js';
function fixture(t){
 const original=globalThis.ImageData;
 globalThis.ImageData=class {constructor(data,width,height){this.data=data;this.width=width;this.height=height;}};
 t.after(()=>{if(original)globalThis.ImageData=original;else delete globalThis.ImageData;});
 class Element{
  constructor(tagName){this.tagName=tagName;this.children=[];this.width=300;this.height=150;const classes=new Set();this.classList={add:c=>classes.add(c),remove:c=>classes.delete(c),contains:c=>classes.has(c)};this.style={setProperty(){},removeProperty(){}};}
  contains(c){return this.children.includes(c);}
  append(c){this.children.push(c);c.parent=this;}
  remove(){this.parent.children=this.parent.children.filter(c=>c!==this);}
  setAttribute(){}
  getContext(){return {putImageData:frame=>{this.last=frame.data;}};}
  toDataURL(){return 'data:image/png;base64,test';}
 }
 return {document:{createElement:()=>new Element('CANVAS')},element:tag=>new Element(tag)};
}
test('animated button paint avoids PNG encoding and repeated backing-store resize',t=>{
 const f=fixture(t),painter=createShellPainter(f.document),el=f.element('BUTTON'),surface=painter.surface(el);
 const pixels=new Uint8Array(160*32*4);pixels[3]=255;
 for(let i=0;i<120;i++)surface.paint(pixels,160,32);
 assert.deepEqual(painter.snapshot(),{paints:120,pngEncodes:0,resizes:1,liveBytes:160*32*4});
 assert.equal(el.children.length,1);
 const canvas=el.children[0];el.children=[];surface.paint(pixels,160,32);assert.equal(el.children[0],canvas,'textContent replacement reattaches the existing surface');
 assert.equal(el.children[0].last.buffer,pixels.buffer,'transport uses the existing pixel buffer');
 surface.suspend();assert.equal(painter.snapshot().liveBytes,0);assert.equal(el.classList.contains('forma-native-surface'),false);
 surface.paint(pixels,160,32);surface.destroy();painter.destroy();assert.equal(el.children.length,0);assert.equal(painter.snapshot().liveBytes,0);
});
test('void input elements retain CSS paint and share one bounded scratch canvas',t=>{
 const f=fixture(t),painter=createShellPainter(f.document),input=f.element('INPUT'),select=f.element('SELECT');
 const a=painter.surface(input),b=painter.surface(select),pixels=new Uint8Array(100*32*4);
 a.paint(pixels,100,32);b.paint(pixels,100,32);
 assert.equal(input.children.length,0);assert.equal(select.children.length,0);
 assert.equal(painter.snapshot().pngEncodes,2);assert.equal(painter.snapshot().resizes,1);
 a.destroy();b.destroy();painter.destroy();assert.equal(painter.snapshot().liveBytes,0);
});
test('a painted surface moves to another host of the same kind without repainting',t=>{
 const f=fixture(t),painter=createShellPainter(f.document),pixels=new Uint8Array(100*32*4);
 const from=f.element('BUTTON'),to=f.element('BUTTON'),a=painter.surface(from),b=painter.surface(to);
 a.paint(pixels,100,32);const canvas=from.children[0];
 const paint=a.take();assert.equal(paint.canvas,canvas);assert.equal(from.children.length,0);assert.equal(from.classList.contains('forma-native-surface'),false);
 assert.equal(painter.bytes(paint),100*32*4);assert.equal(painter.snapshot().liveBytes,100*32*4,'a taken canvas keeps its pixels');
 assert.equal(b.adopt(paint),true);assert.equal(to.children[0],canvas);assert.equal(to.classList.contains('forma-native-surface'),true);
 assert.equal(painter.snapshot().paints,1,'adoption does not paint');
 b.paint(pixels,100,32);assert.equal(to.children.length,1,'the adopted canvas is the one later paints use');
 const input=f.element('INPUT'),field=painter.surface(input);field.paint(pixels,100,32);
 const encoded=field.take();assert.equal(encoded.url,'url("data:image/png;base64,test")');
 assert.equal(b.adopt(encoded),false,'a CSS image never lands in a canvas host');
 assert.equal(painter.surface(f.element('SELECT')).adopt(encoded),true);assert.equal(painter.snapshot().pngEncodes,1);
 painter.discard(b.take());assert.equal(painter.snapshot().liveBytes,100*32*4,'only the shared scratch canvas is left');
 assert.equal(a.take(),null,'a bare surface has nothing to hand over');
 painter.destroy();assert.equal(painter.snapshot().liveBytes,0);
});
test('a host that swaps paints keeps its surface styling in between',t=>{
 const f=fixture(t),painter=createShellPainter(f.document),pixels=new Uint8Array(100*32*4);
 const el=f.element('BUTTON'),surface=painter.surface(el);surface.paint(pixels,100,32);
 const first=surface.take(true);assert.equal(el.children.length,0);assert.equal(el.classList.contains('forma-native-surface'),true);
 surface.paint(pixels,100,32);const second=surface.take(true);assert.notEqual(second.canvas,first.canvas,'the next paint draws on a canvas of its own');
 assert.equal(surface.adopt(first),true);assert.equal(el.children[0],first.canvas);
 painter.discard(second);surface.destroy();painter.destroy();assert.equal(painter.snapshot().liveBytes,0);
});
