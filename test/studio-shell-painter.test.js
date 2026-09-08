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
