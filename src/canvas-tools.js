import {createEventScope} from './event-scope.js';
import {layoutDistances} from './layout-distances.js';
export const fitScale=(viewport,width,height)=>Math.max(.1,Math.min(2,(viewport.width-48)/Math.max(1,width),(viewport.height-48)/Math.max(1,height)));
export function createCanvasTools({viewport,artboard,toolbar,getScene,getSelection,getMode,setMode,onChange}) {
 const events=createEventScope(),listen=events.listen;
  let scale=1,autoFit=false,space=false,drag=null;
  const controls=document.createElement('div');controls.className='canvas-tools';
  controls.innerHTML='<div class="canvas-modes" role="group" aria-label="Режим холста"><button data-mode="design">Дизайн</button><button data-mode="interact">Взаимодействие</button></div><button data-zoom="out" aria-label="Уменьшить масштаб">−</button><button data-zoom="reset" title="Исходный масштаб">100%</button><button data-zoom="in" aria-label="Увеличить масштаб">+</button><button data-fit>Вписать</button><button data-selection>К выделению</button><button data-guides aria-pressed="false">Размеры</button>';
  toolbar.after(controls);
  controls.querySelector('[data-fit]').title='Весь Frame; автоматически подстраивается под размер панели';
  controls.querySelector('[data-selection]').title='Приблизить выбранный компонент';
  controls.querySelector('[data-guides]').title='Логические размеры и границы; не зависят от масштаба';
  viewport.title='Пробел + перетаскивание — перемещение в режиме дизайна; Ctrl/⌘ + колесо — масштаб';
  const overlay=document.createElement('div');overlay.className='canvas-measurements';overlay.hidden=true;viewport.append(overlay);
  let guides=false;
  const scene=()=>getScene()??{width:artboard.offsetWidth,height:artboard.offsetHeight};
  function zoom(next,anchor) {
    const old=scale;scale=Math.max(.1,Math.min(4,next));
    const point=anchor??[viewport.clientWidth/2,viewport.clientHeight/2];
    artboard.style.zoom=String(scale);
    viewport.scrollLeft=(viewport.scrollLeft+point[0])*scale/old-point[0];
    viewport.scrollTop=(viewport.scrollTop+point[1])*scale/old-point[1];
    controls.querySelector('[data-zoom="reset"]').textContent=Math.round(scale*100)+'%';
    onChange?.();draw();
  }
  function fit(){const s=scene();zoom(fitScale({width:viewport.clientWidth,height:viewport.clientHeight},s.width,s.height));viewport.scrollLeft=0;viewport.scrollTop=0;}
  function selectedBounds(s=scene()){
    const selected=getSelection();
    const c=s.controls?.find(c=>c.index===selected?.index);
    return c?.bounds??(selected?.root?[0,0,s.width,s.height]:null);
  }
  function draw(){
    if(!guides||getMode()!=='design'){overlay.hidden=true;return;}
    const s=scene(),b=selectedBounds(s);overlay.replaceChildren();overlay.hidden=!b;
    if(overlay.hidden)return;
    const v=viewport.getBoundingClientRect(),r=artboard.getBoundingClientRect();
    const [x,y,w,h]=b,left=r.left-v.left+viewport.scrollLeft+x*scale,top=r.top-v.top+viewport.scrollTop+y*scale;
    const box=document.createElement('div');box.className='canvas-measure-box';
    Object.assign(box.style,{left:left+'px',top:top+'px',width:w*scale+'px',height:h*scale+'px'});
    const label=document.createElement('span');label.textContent=`${Math.round(w)} × ${Math.round(h)} · x ${Math.round(x)}, y ${Math.round(y)}`;
    box.append(label);overlay.append(box);
    const index=getSelection()?.index;
    const others=(s.controls??[]).filter(c=>c.index!==index).map(c=>c.bounds);
    for(const distance of layoutDistances(b,others,[s.width,s.height])){
      const [a,b]=distance.from,[c,d]=distance.to;
      const line=document.createElement('div');line.className='canvas-distance';
      Object.assign(line.style,{left:r.left-v.left+viewport.scrollLeft+a*scale+'px',top:r.top-v.top+viewport.scrollTop+b*scale+'px',width:Math.max(1,(c-a)*scale)+'px',height:Math.max(1,(d-b)*scale)+'px'});
      const text=document.createElement('span');text.textContent=Math.round(distance.value*10)/10+' · '+distance.target;line.append(text);overlay.append(line);
    }
    const outside=x<0||y<0||x+w>s.width||y+h>s.height;
    if(outside){const note=document.createElement('span');note.className='canvas-clip-note';note.textContent=s.scrollable?'За пределами видимой области прокрутки':s.clip?'Часть элемента обрезана Frame':'Элемент выходит за Frame';box.append(note);}
  }
  listen(controls,'click',event=>{
    const button=event.target.closest('button');if(!button)return;
    if(button.dataset.mode){setMode(button.dataset.mode);update();return;}
    if(button.hasAttribute('data-guides')){guides=!guides;button.setAttribute('aria-pressed',String(guides));draw();return;}
    if(button.hasAttribute('data-fit')){autoFit=true;fit();return;}
    if(button.hasAttribute('data-selection')){
      const b=selectedBounds();if(!b)return;autoFit=false;
      zoom(fitScale({width:viewport.clientWidth,height:viewport.clientHeight},b[2],b[3]));
      const r=artboard.getBoundingClientRect(),v=viewport.getBoundingClientRect();
      viewport.scrollLeft+=r.left-v.left+(b[0]+b[2]/2)*scale-viewport.clientWidth/2;
      viewport.scrollTop+=r.top-v.top+(b[1]+b[3]/2)*scale-viewport.clientHeight/2;draw();return;
    }
    if(button.dataset.zoom){autoFit=false;zoom(button.dataset.zoom==='reset'?1:scale*(button.dataset.zoom==='in'?1.25:.8));}
  });
  const editing=target=>target?.closest?.('input,textarea,select,[contenteditable="true"]');
  listen(window,'keydown',event=>{if(event.code==='Space'&&!editing(event.target)&&getMode()==='design'){space=true;viewport.classList.add('canvas-pan-ready');event.preventDefault();}});
  function release(){if(drag&&viewport.hasPointerCapture(drag.id))viewport.releasePointerCapture(drag.id);drag=null;space=false;viewport.classList.remove('canvas-pan-ready','canvas-panning');}
  listen(window,'keyup',event=>{if(event.code==='Space')release();});listen(window,'blur',release);
  listen(viewport,'pointerdown',event=>{
    if(!(space&&event.button===0)&&event.button!==1)return;
    event.preventDefault();event.stopImmediatePropagation();drag={id:event.pointerId,x:event.clientX,y:event.clientY,left:viewport.scrollLeft,top:viewport.scrollTop};
    viewport.setPointerCapture(event.pointerId);viewport.classList.add('canvas-panning');
  },true);
  listen(viewport,'pointermove',event=>{if(!drag)return;event.preventDefault();event.stopImmediatePropagation();viewport.scrollLeft=drag.left+drag.x-event.clientX;viewport.scrollTop=drag.top+drag.y-event.clientY;},true);
  listen(viewport,'pointerup',event=>{if(drag){event.stopImmediatePropagation();release();}},true);
  listen(viewport,'pointercancel',release);
  listen(viewport,'wheel',event=>{if(!event.ctrlKey&&!event.metaKey)return;event.preventDefault();event.stopImmediatePropagation();autoFit=false;const r=viewport.getBoundingClientRect();zoom(scale*Math.exp(-event.deltaY*.005),[event.clientX-r.left,event.clientY-r.top]);},{passive:false,capture:true});
  function update(){
    for(const b of controls.querySelectorAll('[data-mode]'))b.setAttribute('aria-pressed',String(b.dataset.mode===getMode()));
    controls.querySelector('[data-selection]').disabled=!selectedBounds();
    viewport.dataset.mode=getMode();if(autoFit)fit();else draw();
  }
  const resize=new ResizeObserver(()=>{if(autoFit)fit();else draw();});resize.observe(viewport);
  listen(viewport,'scroll',draw);
  update();return {update,destroy(){events.dispose();resize.disconnect();release();controls.remove();overlay.remove();}};
}
