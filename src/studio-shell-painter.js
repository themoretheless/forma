// Paint transport only. Runtime state and DOM command handling belong to the host.
// Inputs/selects cannot contain a canvas, so only those use a CSS image fallback.
export function createShellPainter(document){
 let scratch=null,scratchContext=null;
 const stats={paints:0,pngEncodes:0,resizes:0,liveBytes:0};
 function resize(canvas,width,height){
  if(canvas.width===width&&canvas.height===height)return;
  stats.liveBytes+=(width*height-canvas.width*canvas.height)*4;
  canvas.width=width;canvas.height=height;stats.resizes++;
 }
 function canvas(){const c=document.createElement('canvas');c.width=0;c.height=0;return c;}
 return {
  surface(el){
   const embedded=!['INPUT','SELECT'].includes(el.tagName);
   let target=null,context=null,url=null;
   function clear(){
    if(target)resize(target,0,0);
    el.classList.remove('forma-native-surface');el.style.removeProperty('--forma-surface');
   }
   return {
    paint(pixels,width,height){
     if(embedded){
      if(!target){target=canvas();target.className='forma-control-paint';target.setAttribute('aria-hidden','true');context=target.getContext('2d');}
      if(!el.contains(target))el.append(target);
     }else{
      if(!scratch){scratch=canvas();scratchContext=scratch.getContext('2d');}
      target=scratch;context=scratchContext;
     }
     if(!context)throw Error('Studio controls require Canvas 2D');
     resize(target,width,height);
     context.putImageData(new ImageData(new Uint8ClampedArray(pixels.buffer,pixels.byteOffset,pixels.byteLength),width,height),0,0);
     if(!embedded){url=`url("${target.toDataURL()}")`;el.style.setProperty('--forma-surface',url);stats.pngEncodes++;target=null;}
     if(!el.classList.contains('forma-native-surface'))el.classList.add('forma-native-surface');stats.paints++;
    },
    // The painted result, handed over whole to another host of the same kind: this one is left bare.
    take(){
     const paint=embedded?target?.width&&{canvas:target,context}:url&&{url};
     if(embedded)target?.remove();
     target=null;context=null;url=null;clear();
     return paint||null;
    },
    adopt(paint){
     if(embedded!==!!paint.canvas)return false;
     if(embedded){target=paint.canvas;context=paint.context;el.append(target);}
     else{url=paint.url;el.style.setProperty('--forma-surface',url);}
     el.classList.add('forma-native-surface');return true;
    },
    suspend:clear,
    destroy(){clear();target?.remove();target=null;context=null;},
   };
  },
  // Bytes a taken paint holds, and its release when nobody adopts it.
  bytes:paint=>paint.canvas?paint.canvas.width*paint.canvas.height*4:paint.url.length*2,
  discard(paint){if(paint.canvas)resize(paint.canvas,0,0);},
  snapshot:()=>({...stats}),
  destroy(){if(scratch)resize(scratch,0,0);scratch=null;scratchContext=null;},
 };
}
