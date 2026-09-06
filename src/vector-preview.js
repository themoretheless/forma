import {createGpuPainter} from './vector-gpu.js';
let runtimePromise;

export function loadVectorRuntime() {
  if (!runtimePromise) {
    const url = import.meta.env.DEV ? '/__forma_vector/forma_vector.js' : '/vector-pkg/forma_vector.js';
    runtimePromise = import(/* @vite-ignore */ url).then(async runtime => {
      await runtime.default();
      return runtime;
    }).catch(error => {
      runtimePromise = null;
      throw error;
    });
  }
  return runtimePromise;
}

export function createVectorPreview({runtime, onSelect, onAction, onError} = {}) {
  if (!runtime?.Button) throw new Error('Сначала загрузите векторный WASM runtime');
  const host = document.createElement('div');
  host.className = 'forma-vector-host';
  Object.assign(host.style, {position: 'relative', maxWidth: '100%', lineHeight: '0'});
  const canvas = document.createElement('canvas');
  canvas.className = 'forma-vector-canvas';
  canvas.setAttribute('role', 'button');
  Object.assign(canvas.style, {display: 'block', width: '100%', height: 'auto', touchAction: 'none', outline: 'none'});
  const outline = document.createElement('div');
  outline.className = 'forma-vector-selection';
  outline.setAttribute('aria-hidden', 'true');
  Object.assign(outline.style, {position: 'absolute', pointerEvents: 'none', boxSizing: 'border-box', outline: '2px solid #92aaff', outlineOffset: '-2px'});
  outline.hidden = true;
  host.append(canvas, outline);
  const gpuCanvas=document.createElement('canvas');
  Object.assign(gpuCanvas.style,{position:'absolute',left:'0',top:'0',pointerEvents:'none'});
  gpuCanvas.setAttribute('aria-hidden','true');gpuCanvas.hidden=true;host.insertBefore(gpuCanvas,outline);
  const backendBadge=document.createElement('span');
  Object.assign(backendBadge.style,{position:'absolute',right:'4px',bottom:'4px',font:'10px monospace',lineHeight:'14px',color:'#a1aec8',pointerEvents:'none'});
  backendBadge.textContent='CPU';host.append(backendBadge);
  const context = canvas.getContext('2d');
  if (!context) throw new Error('Canvas 2D недоступен');
  const listeners = new AbortController();
  let button = null, current = null, selectedStart = null, destroyed = false;
  let pointerId = null, pressedKey = null, frame = null, lastFrameTime = 0;
  let gpu=null,gpuFailure='';
  function gpuFallback(error){
    gpuFailure=String(error.message??error);gpu?.destroy();gpu=null;
    gpuCanvas.hidden=true;canvas.style.opacity='1';backendBadge.textContent='CPU fallback';backendBadge.title=gpuFailure;
    if(button&&!destroyed){try{paint();}catch(error){report(error);}}
  }
  createGpuPainter(gpuCanvas,gpuFallback).then(painter=>{
    if(destroyed){painter.destroy();return;}gpu=painter;
    if(button){try{paint();}catch(error){gpuFallback(error);}}
  }).catch(gpuFallback);
  const report = error => onError?.(error instanceof Error ? error : new Error(String(error)));
  const listen = (target, name, callback) => target.addEventListener(name, callback, {signal: listeners.signal});
  const buttonNode = () => {const child=current?.nodes?.[0]?.children?.[0];return child?.type==='Scroll'?child.children[0]:child;};
  const frameNode = () => current?.nodes?.[0];

  function raster(model) {
    const width = model.render_width(), height = model.render_height();
    if (!Number.isFinite(width) || !Number.isFinite(height) || width <= 0 || height <= 0) throw new Error('Некорректный размер векторного интерфейса');
    const dpr = Math.min(2, Math.max(1, window.devicePixelRatio || 1));
    const scale = Math.min(dpr, 4096 / width, 4096 / height, Math.sqrt(8_000_000 / (width * height)));
    const pixelWidth = Math.max(1, Math.floor(width * scale));
    const pixelHeight = Math.max(1, Math.floor(height * scale));
    if(gpu)return {width:pixelWidth,height:pixelHeight,scale,model};
    const pixels = model.content_pixels(pixelWidth, pixelHeight, scale);
    if (pixels.length !== pixelWidth * pixelHeight * 4) throw new Error('Векторный renderer вернул некорректное изображение');
    return new ImageData(new Uint8ClampedArray(pixels), pixelWidth, pixelHeight);
  }

  function paint(image = raster(button)) {
    if (canvas.width !== image.width) canvas.width = image.width;
    if (canvas.height !== image.height) canvas.height = image.height;
    if(gpu&&image.model){
      try{
        gpu.draw(image.model,image.width,image.height,image.scale);
        gpuCanvas.style.width=canvas.style.width;gpuCanvas.style.height=canvas.style.height;
        gpuCanvas.hidden=false;canvas.style.opacity='0';backendBadge.textContent='GPU · vector';backendBadge.title='WebGPU · геометрия и контуры из Rust';
      }catch(error){gpuFallback(error);}
    }else{context.putImageData(image, 0, 0);}
  }

  function updateSelection() {
    if (!button || !current) return;
    const selectedButton = selectedStart != null && selectedStart === buttonNode()?.start;
    const selectedFrame = selectedStart != null && selectedStart === frameNode()?.start;
    outline.hidden = !current.designMode || (!selectedButton && !selectedFrame);
    const [x, y, width, height] = selectedButton ? button.bounds() : [0, 0, button.width(), button.height()];
    Object.assign(outline.style, {left: `${x / button.width() * 100}%`, top: `${y / button.height() * 100}%`, width: `${width / button.width() * 100}%`, height: `${height / button.height() * 100}%`});
    if (selectedStart != null) outline.dataset.start = String(selectedStart);
    else delete outline.dataset.start;
  }

  function schedule() {
    if (destroyed || !button || current.designMode || frame !== null || !button.is_animating()) return;
    lastFrameTime = performance.now();
    frame = requestAnimationFrame(animate);
  }

  function animate(now) {
    frame = null;
    if (destroyed || !button || current.designMode || !host.isConnected) return;
    try {
      const revision=button.visual_revision();
      const ongoing = button.tick(Math.max(0, Math.min(100, now - lastFrameTime)));
      lastFrameTime = now;
      if(button.visual_revision()!==revision)paint();
      if (ongoing) frame = requestAnimationFrame(animate);
    } catch (error) { report(error); }
  }

  function change(update, forcePaint=false) {
    if (!button || destroyed) return;
    try {
      const count = button.clicks();
      const revision=button.visual_revision();
      update();
      if(forcePaint||button.visual_revision()!==revision)paint();
      schedule();
      if (!current.designMode && button.clicks() !== count && button.action()) onAction?.(button.action(), buttonNode());
    } catch (error) { report(error); }
  }

  function position(event) {
    const bounds = canvas.getBoundingClientRect();
    return [(event.clientX - bounds.left) * button.render_width() / bounds.width, (event.clientY - bounds.top) * button.render_height() / bounds.height];
  }

  function releasePointer() {
    const id = pointerId;
    pointerId = null;
    if (id !== null && canvas.hasPointerCapture(id)) canvas.releasePointerCapture(id);
  }

  function cancel() {
    pressedKey = null;
    releasePointer();
    change(() => { button.pointer(-1, -1, 3); button.focus(false); });
  }

  function center() {
    const [x, y, width, height] = button.bounds();
    return [x + width / 2, y + height / 2];
  }

  function activate() {
    if (!button || current.designMode || button.disabled()) return false;
    change(() => {
      const [x, y] = center();
      button.pointer(x, y, 1);
      button.pointer(x, y, 2);
      button.pointer(-1, -1, 3);
    });
    return true;
  }

  listen(canvas, 'pointerdown', event => {
    if (!button || event.button !== 0 || event.isPrimary === false) return;
    const [x, y] = position(event);
    if (current.designMode) {
      event.preventDefault();
      const node = button.hit(x, y) ? buttonNode() : frameNode();
      if (node) { selectedStart = node.start; updateSelection(); onSelect?.(node); }
      return;
    }
    if (button.disabled() || !button.hit(x, y) || pointerId !== null) return;
    event.preventDefault();
    canvas.focus({preventScroll: true});
    pointerId = event.pointerId;
    canvas.setPointerCapture(pointerId);
    change(() => button.pointer(x, y, 1));
  });
  listen(canvas, 'pointermove', event => {
    if (!button || current.designMode || (pointerId !== null && event.pointerId !== pointerId)) return;
    change(() => button.pointer(...position(event), 0));
  });
  listen(canvas, 'pointerup', event => {
    if (!button || current.designMode || event.pointerId !== pointerId) return;
    releasePointer();
    change(() => button.pointer(...position(event), 2));
  });
  listen(canvas, 'pointerleave', event => {
    if (!button || current.designMode) return;
    change(() => button.pointer(...position(event), pointerId === null ? 3 : 0));
  });
  listen(canvas, 'pointercancel', event => { if (event.pointerId === pointerId) cancel(); });
  listen(canvas, 'lostpointercapture', event => { if (event.pointerId === pointerId) cancel(); });
  listen(canvas, 'focus', () => { if (button && !current.designMode) change(() => button.focus(true)); });
  listen(canvas, 'blur', cancel);
  listen(canvas, 'keydown', event => {
    if (!button || current.designMode || button.disabled() || ![' ', 'Enter'].includes(event.key)) return;
    event.preventDefault();
    if (event.repeat || pressedKey !== null) return;
    pressedKey = event.key;
    change(() => button.key_event(event.key===' '?1:2,true,false));
  });
  listen(canvas, 'keyup', event => {
    if (!button || current.designMode || event.key !== pressedKey) return;
    event.preventDefault();
    pressedKey = null;
    change(() => button.key_event(event.key===' '?1:2,false,false));
  });
  // Assistive technology can activate a role=button through a synthetic click.
  listen(canvas, 'click', event => { if (event.detail === 0) activate(); });
  canvas.addEventListener('wheel', event => {if(!button||!button.scrollable())return;event.preventDefault();const factor=event.deltaMode===1?16:event.deltaMode===2?button.height():1;change(()=>button.scroll(event.deltaX*factor,event.deltaY*factor));updateSelection();},{passive:false,signal:listeners.signal});
  listen(window, 'resize', () => { if (button && host.isConnected) change(() => {},true); });
  listen(window, 'blur', cancel);

  return {
    measureText:(value,fontSize)=>runtime.text_metrics(value,fontSize),
    render({container, source, template = '', designMode = true, nodes = [], selectedStart: nextSelection = null}) {
      let candidate = null;
      try {
        if (destroyed) throw new Error('Векторный предпросмотр уже закрыт');
        if (!container?.replaceChildren) throw new Error('Не найден контейнер предпросмотра');
        designMode = Boolean(designMode);
        const changed = !button || current.source !== source || current.template !== template;
        const modeChanged = !changed && current.designMode !== designMode;
        if (changed) {
          candidate = new runtime.Button();
          candidate.load_component(source, template);
          if(button)candidate.preserve_interaction(button);
        }
        // Validate and rasterize before touching the last good DOM or runtime.
        const image = raster(candidate ?? button);
        if (changed) {
          cancelAnimationFrame(frame);
          frame = null;
          releasePointer();
          pressedKey = null;
          button?.free();
          button = candidate;
          candidate = null;
        } else if (modeChanged) {
          releasePointer();
          pressedKey = null;
          button.pointer(-1, -1, 3);
          button.focus(false);
          button.tick(1000);
        }
        current = {source, template, designMode, nodes};
        container.classList.add('vector-artboard');
        container.style.backgroundColor=button.background_color();
        container.style.borderRadius=(button.frame_radius()+1)+'px';
        selectedStart = nextSelection;
        host.style.width = `${button.width()}px`;
        host.style.height = `${button.height()}px`;
        canvas.style.width = `${button.render_width()}px`;canvas.style.height = `${button.render_height()}px`;
        host.style.overflow = 'visible'; // Rust applies the rounded content clip once.
        if (frameNode()) host.dataset.start = String(frameNode().start);
        else delete host.dataset.start;
        canvas.tabIndex = !designMode && button.disabled() ? -1 : 0;
        canvas.setAttribute('aria-label', button.label());
        canvas.setAttribute('aria-disabled', String(button.disabled()));
        canvas.style.cursor = designMode ? 'crosshair' : button.disabled() ? 'default' : 'pointer';
        paint(modeChanged ? raster(button) : image);
        if (host.parentElement !== container) container.replaceChildren(host);
        updateSelection();
        schedule();
        return true;
      } catch (error) {
        candidate?.free();
        report(error);
        return false;
      }
    },
    select(start) { selectedStart = start; updateSelection(); },
    activate,
    snapshot() {
      return button && current ? {...current, nodes: undefined, renderer: gpu?'rust-wasm-webgpu':'rust-wasm-cpu',gpuStats:gpu?.snapshot()??null,gpuFallbackReason:gpuFailure||null, rasterStats: Array.from(button.raster_stats()), selectedStart, width: button.width(), height: button.height(), clip:button.clipped(),radius:button.frame_radius(),overflow:button.overflow(),scrollable:button.scrollable(),renderWidth:button.render_width(),renderHeight:button.render_height(),scrollOffset:Array.from(button.scroll_offset()), bounds: Array.from(button.bounds()), label: button.label(), key: button.key(), action: button.action(), disabled: button.disabled(), clicks: button.clicks()} : null;
    },
    destroy() {
      if (destroyed) return;
      destroyed = true;
      gpu?.destroy();gpu=null;
      listeners.abort();
      cancelAnimationFrame(frame);
      frame = null;
      releasePointer();
      host.remove();
      button?.free();
      button = null;
      current = null;
    },
  };
}
