import {createGpuPainter} from './vector-gpu.js';
let runtimePromise;

export function loadVectorRuntime() {
  if (!runtimePromise) {
    const url = import.meta.env.DEV ? '/__forma_vector/forma.js' : '/vector-pkg/forma.js';
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

export function createVectorPreview({runtime, onSelect, onAction, onError, onControlPointer, onControlKey, onBindingChange} = {}) {
  if (!runtime?.Runtime) throw new Error('Сначала загрузите векторный WASM runtime');
  const host = document.createElement('div');
  host.className = 'forma-vector-host';
  Object.assign(host.style, {position: 'relative', flexShrink: '0', lineHeight: '0'});
  const canvas = document.createElement('canvas');
  canvas.className = 'forma-vector-canvas';
  canvas.setAttribute('role', 'img');
  Object.assign(canvas.style, {display: 'block', width: '100%', height: 'auto', touchAction: 'none', outline: 'none'});
  const outline = document.createElement('div');
  outline.className = 'forma-vector-selection';
  outline.setAttribute('aria-hidden', 'true');
  Object.assign(outline.style, {position: 'absolute', pointerEvents: 'none', boxSizing: 'border-box', outline: '2px solid #92aaff', outlineOffset: '-2px'});
  outline.hidden = true;
  host.append(canvas, outline);
  // Browser input/clipboard/IME adapter; pixels and editing state stay in Rust.
  const input=document.createElement('textarea');
  input.setAttribute('aria-label','Поле ввода');input.tabIndex=-1;input.hidden=true;
  Object.assign(input.style,{position:'absolute',opacity:'0',width:'1px',height:'20px',padding:'0',border:'0',resize:'none',pointerEvents:'none'});
  host.append(input);
  let composing=false;

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
  let layoutModel=null,layoutRevision=null,layout=null;
  let pointerId = null, pressedKey = null, frame = null, lastFrameTime = 0;
  let cursorPosition = null;
  let reverseTabEntry = false;
  let controlDrag = null;
  let proximityPosition = null;
  let gpu=null,gpuFailure='';
  let paintedModel=null,paintedRevision=null,paintedWidth=0,paintedHeight=0,paintedScale=0,paintedGpu=null;
  function gpuFallback(error){
    if(destroyed)return;
    gpuFailure=String(error.message??error);gpu?.destroy();gpu=null;
    gpuCanvas.hidden=true;canvas.style.opacity='1';backendBadge.textContent='CPU fallback';backendBadge.title=gpuFailure;
    if(button&&!destroyed){try{paint();}catch(error){report(error);}}
  }
  createGpuPainter(gpuCanvas,gpuFallback).then(painter=>{
    if(destroyed){painter.destroy();return;}gpu=painter;
    if(button){try{paint();}catch(error){gpuFallback(error);}}
  }).catch(gpuFallback);
  const report = error => onError?.(error instanceof Error ? error : new Error(String(error)));
  const listen = (target, name, callback, options={}) => target.addEventListener(name, callback, {...options, signal: listeners.signal});
  const controlNodes=()=>{if(current?.previewControls)return current.previewControls;const children=current?.nodes?.[0]?.children??[];return children[0]?.type==='Scroll'?children[0].children:children;};
  const buttonNode=(index=0)=>controlNodes()[index];
  const frameNode = () => current?.nodes?.[0];
  const enabledControl = index => index >= 0 && button.control_interactive(index) && !button.control_disabled(index);
  function hasEnabledControl() {
    for (let i = 0; i < button.control_count(); i++) if (enabledControl(i)) return true;
    return false;
  }

  function raster(model) {
    const width = model.render_width(), height = model.render_height();
    if (!Number.isFinite(width) || !Number.isFinite(height) || width <= 0 || height <= 0) throw new Error('Некорректный размер векторного интерфейса');
    const dpr = Math.min(2, Math.max(1, window.devicePixelRatio || 1));
    const scale = Math.min(dpr, 4096 / width, 4096 / height, Math.sqrt(8_000_000 / (width * height)));
    const pixelWidth = Math.max(1, Math.floor(width * scale));
    const pixelHeight = Math.max(1, Math.floor(height * scale));
    const revision=model.visual_revision();
    // Selection, layout notifications and repeated renders can reuse the pixels
    // already on the canvas. Keep only the key, not a second full-size bitmap.
    if(paintedModel===model&&paintedRevision===revision&&paintedWidth===pixelWidth&&paintedHeight===pixelHeight&&paintedScale===scale&&paintedGpu===gpu)return null;
    const image={width:pixelWidth,height:pixelHeight,scale,model,revision};
    if(gpu)return image;
    const pixels = model.content_pixels(pixelWidth, pixelHeight, scale);
    if (pixels.length !== pixelWidth * pixelHeight * 4) throw new Error('Векторный renderer вернул некорректное изображение');
    // wasm-bindgen returns an owned Uint8Array; ImageData can share its backing
    // buffer instead of copying up to 32 MB of RGBA bytes on every CPU frame.
    image.data=new ImageData(new Uint8ClampedArray(pixels.buffer,pixels.byteOffset,pixels.byteLength),pixelWidth,pixelHeight);
    return image;
  }

  function paint(image = raster(button)) {
    if(!image)return;
    if(gpu){
      try{
        gpu.draw(image.model,image.width,image.height,image.scale);
        gpuCanvas.style.width=canvas.style.width;gpuCanvas.style.height=canvas.style.height;
        gpuCanvas.hidden=false;canvas.style.opacity='0';backendBadge.textContent='GPU · vector';backendBadge.title='WebGPU · геометрия и контуры из Rust';
        // CSS keeps input coordinates unchanged while releasing the unused CPU
        // canvas backing store once the GPU owns presentation.
        if(canvas.width!==1)canvas.width=1;if(canvas.height!==1)canvas.height=1;
      }catch(error){gpuFallback(error);return;}
    }else{
      if (canvas.width !== image.width) canvas.width = image.width;
      if (canvas.height !== image.height) canvas.height = image.height;
      context.putImageData(image.data, 0, 0);
    }
    paintedModel=image.model;paintedRevision=image.revision;paintedWidth=image.width;paintedHeight=image.height;paintedScale=image.scale;paintedGpu=gpu;
  }

  function updateSelection() {
    if (!button || !current) return;
    const selectedIndex=selectedStart==null?-1:controlNodes().findIndex(n=>n.start===selectedStart);
    const selectedButton=selectedIndex>=0;
    const selectedFrame = selectedStart != null && selectedStart === frameNode()?.start;
    outline.hidden = !current.designMode || (!selectedButton && !selectedFrame);
    const [x, y, width, height] = selectedButton ? button.control_bounds(selectedIndex) : [0, 0, button.width(), button.height()];
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
      updateCursor();
      syncTextInput();
      if(forcePaint||button.visual_revision()!==revision)paint();
      schedule();
      if (!current.designMode) {
        const activated=button.clicks()!==count, eventNode=activated?buttonNode(button.event_index()):null, action=activated?button.action():null;
        const changes=[];
        if(onBindingChange&&(activated||button.visual_revision()!==revision))controlNodes().forEach((node,index)=>{
          for(const [property,path]of Object.entries(node.bindings??{})){
            let value;
            if(property==='value'&&button.control_editable?.(index))value=button.text_value(index);
            else if(property==='value'&&node.type==='Slider'&&button.range_value){const range=button.range_value(index);if(Number.isFinite(range))value=range;}
            else if((property==='checked'||property==='selected')&&eventNode===node&&typeof node.props[property]==='boolean')value=property==='selected'?true:!node.props[property];
            if(value!==undefined&&value!==node.props[property])changes.push({node,property,path,value});
          }
        });
        // Consumers may recompile synchronously; capture event identity before
        // callbacks can replace/free the old WASM model.
        if(changes.length)onBindingChange(changes);
        if(action)onAction?.(action,eventNode);
      }
    } catch (error) { report(error); }
  }

  function syncTextInput() {
    if(!button)return;
    const editing=!current?.designMode&&button.text_editing?.();
    if(editing) {
      input.hidden=false;
      const bounds=button.text_caret_bounds();
      Object.assign(input.style,{left:`${Math.max(0,bounds[0]??0)}px`,top:`${Math.max(0,bounds[1]??0)}px`,height:`${bounds[3]??20}px`});
      input.setAttribute('aria-label',button.control_label(button.focused_index())||'Поле ввода');
      if(document.activeElement===canvas) input.focus({preventScroll:true});
    } else {if(document.activeElement===input) {if(!current?.designMode&&button.focused_index()>=0)canvas.focus({preventScroll:true});else input.blur?.();}input.hidden=true;}
  }
  const textInsert=text=>change(()=>button.text_insert?.(text));
  listen(input,'compositionstart',()=>{composing=true;});
  listen(input,'compositionupdate',event=>change(()=>button.text_preedit?.(event.data??'')));
  listen(input,'compositionend',event=>{composing=false;textInsert(event.data??'');input.value='';});
  listen(input,'beforeinput',event=>{
    if(composing||event.isComposing)return;
    event.preventDefault();
    if(event.inputType==='insertText'||event.inputType==='insertReplacementText')textInsert(event.data??'');
    else if(['insertLineBreak','insertParagraph'].includes(event.inputType))change(()=>button.text_key?.('Enter',false,false));
    else if(event.inputType==='deleteContentBackward')change(()=>button.text_key?.('Backspace',false,false));
    else if(event.inputType==='deleteContentForward')change(()=>button.text_key?.('Delete',false,false));
    else if(event.inputType==='historyUndo')change(()=>button.text_key?.('z',false,true));
    else if(event.inputType==='historyRedo')change(()=>button.text_key?.('z',true,true));
    input.value='';
  });
  listen(input,'paste',event=>{event.preventDefault();textInsert(event.clipboardData?.getData('text/plain')??'');});
  for(const name of ['copy','cut'])listen(input,name,event=>{
    const text=button?.text_selected?.();if(!text)return;
    event.preventDefault();event.clipboardData?.setData('text/plain',text);
    if(name==='cut')textInsert('');
  });
  listen(input,'blur',event=>{if(event.relatedTarget!==canvas){composing=false;cancel();}});
  function position(event) {
    const bounds = canvas.getBoundingClientRect();
    return [(event.clientX - bounds.left) * button.render_width() / bounds.width, (event.clientY - bounds.top) * button.render_height() / bounds.height];
  }

  function updateCursor(event) {
    if (event) cursorPosition = {clientX: event.clientX, clientY: event.clientY};
    if (!button || !current) return;
    const index=!current.designMode&&cursorPosition?button.hit_index(...position(cursorPosition)):-1;
    canvas.style.cursor = current.designMode ? 'crosshair'
      : enabledControl(index) ? (button.control_editable?.(index)?'text':'pointer') : 'default';
  }

  function releasePointer() {
    const id = pointerId;
    pointerId = null;
    if (id !== null && canvas.hasPointerCapture(id)) canvas.releasePointerCapture(id);
  }

  function cancel(event) {
    pressedKey = null;
    reverseTabEntry = false;
    controlDrag = null;
    if (event?.pointerType === 'touch') proximityPosition = null;
    releasePointer();
    change(() => { button.pointer(-1, -1, 3); button.focus(false); button.reveal_pointer?.(0, 0, false); });
  }

  function clearTouchReveal(event) {
    if (event.pointerType !== 'touch') return;
    proximityPosition = null;
    button.reveal_pointer?.(0, 0, false);
  }

  function forwardPointer(event, kind) {
    change(() => {
      button.pointer(...position(event), kind);
      // Runtime.pointer also updates Reveal; touch keeps its gesture and focus
      // while removing the pointer-specific paint before this frame is drawn.
      clearTouchReveal(event);
    });
  }

  function controlPointer(phase, event, index) {
    if (!onControlPointer || index < 0) return false;
    if (event.pointerType === 'touch') change(() => clearTouchReveal(event));
    const [px, py] = position(event), [x, y, width, height] = button.control_bounds(index);
    return onControlPointer(phase, {node: buttonNode(index), x: px - x, y: py - y, width, height, event}) === true;
  }

  function proximity(event) {
    if (!button || current.designMode || !button.reveal_pointer) return;
    proximityPosition = event && event.pointerType !== 'touch' ? {clientX:event.clientX, clientY:event.clientY} : null;
    change(() => button.reveal_pointer(...(proximityPosition ? position(proximityPosition) : [0, 0]), Boolean(proximityPosition)));
  }

  function activate() {
    if (!button || current.designMode || !hasEnabledControl()) return false;
    change(() => button.activate());
    return true;
  }

  listen(canvas, 'pointerdown', event => {
    reverseTabEntry = false;
    if (!button || event.button !== 0 || event.isPrimary === false) return;
    const [x, y] = position(event);
    updateCursor(event);
    if (current.designMode) {
      event.preventDefault();
      const index=button.hit_index(x,y);
      const node = index>=0 ? buttonNode(index) : frameNode();
      if (node) { selectedStart = node.start; updateSelection(); onSelect?.(node); }
      return;
    }
    if (pointerId !== null) return;
    if (!enabledControl(button.hit_index(x, y))) {
      event.preventDefault(); // Passive geometry and disabled buttons must not focus the canvas.
      return;
    }
    event.preventDefault();
    canvas.focus({preventScroll: true});
    pointerId = event.pointerId;
    canvas.setPointerCapture(pointerId);
    const index = button.hit_index(x, y);
    // Range/splitter models can own a drag without pretending to be a click.
    if (onControlPointer) {
      controlDrag = button.control_key(index);
      change(() => button.focus_control?.(index));
      if (controlPointer('start', event, index)) return;
      controlDrag = null;
    }
    forwardPointer(event, 1);
  });
  listen(canvas, 'pointermove', event => {
    if (!button || current.designMode || (pointerId !== null && event.pointerId !== pointerId)) return;
    updateCursor(event);
    if (controlDrag !== null) {
      const index = controlNodes().findIndex(n => n.props.key === controlDrag);
      controlPointer('move', event, index);
      return;
    }
    forwardPointer(event, 0);
  });
  listen(canvas, 'pointerup', event => {
    if (!button || current.designMode || event.pointerId !== pointerId) return;
    updateCursor(event);
    if (controlDrag !== null) {
      const index = controlNodes().findIndex(n => n.props.key === controlDrag);
      controlPointer('end', event, index);
      controlDrag = null;
      releasePointer();
      return;
    }
    releasePointer();
    forwardPointer(event, 2);
  });
  listen(canvas, 'pointerleave', event => {
    cursorPosition = null;
    updateCursor();
    if (!button || current.designMode) return;
    forwardPointer(event, pointerId === null ? 3 : 0);
  });
  listen(canvas, 'pointercancel', event => { if (event.pointerId === pointerId) cancel(event); });
  listen(canvas, 'lostpointercapture', event => { if (event.pointerId === pointerId) cancel(event); });
  listen(canvas, 'focus', () => {
    const reverse = reverseTabEntry;
    reverseTabEntry = false;
    if (button && !current.designMode && hasEnabledControl()) change(() => {
      if (reverse) {button.focus(false); button.focus_next(true);}
      else button.focus(true);
    });
  });
  listen(canvas, 'blur', event=>{if(event.relatedTarget!==input)cancel();});
  const listenKeys=(name,callback)=>{listen(canvas,name,callback);listen(input,name,callback);};
  listenKeys('keydown', event => {
    if(button&&!current.designMode&&button.text_editing?.()) {
      if(composing||event.isComposing)return;
      let handled=false;change(()=>{handled=button.text_key(event.key,event.shiftKey,event.metaKey||event.ctrlKey);});
      if(handled){event.preventDefault();return;}
      if(event.key!=='Tab')return;
    }
    if (button && !current.designMode && onControlKey?.(event, buttonNode(button.focused_index())) === true) {
      event.preventDefault(); return;
    }
    if(button && !current.designMode && !onControlKey) {
      let handled=false;change(()=>{handled=button.range_key?.(event.key)??false;});
      if(handled){event.preventDefault();return;}
    }

    if(button&&!current.designMode&&event.key==='Tab'){
      let moved=false;change(()=>{moved=button.focus_next(event.shiftKey);});
      if(moved){event.preventDefault();pressedKey=null;}return;
    }
    if (!button || current.designMode || !hasEnabledControl() || ![' ', 'Enter'].includes(event.key)) return;
    event.preventDefault();
    if (event.repeat || pressedKey !== null) return;
    pressedKey = event.key;
    change(() => button.key_event(event.key===' '?1:2,true,false));
  });
  listenKeys('keyup', event => {
    if (!button || current.designMode || event.key !== pressedKey) return;
    event.preventDefault();
    pressedKey = null;
    change(() => button.key_event(event.key===' '?1:2,false,false));
  });
  // Assistive technology can activate a role=button through a synthetic click.
  listen(canvas, 'click', event => { if (event.detail === 0) activate(); });
  canvas.addEventListener('wheel', event => {if(!button||!button.scrollable())return;event.preventDefault();const factor=event.deltaMode===1?16:event.deltaMode===2?button.height():1;change(()=>button.scroll(event.deltaX*factor,event.deltaY*factor));updateSelection();if(proximityPosition)proximity(proximityPosition);},{passive:false,signal:listeners.signal});
  // Tab's default action moves DOM focus after keydown bubbles to the window.
  listen(window, 'keydown', event => {reverseTabEntry = event.key === 'Tab' && event.shiftKey && !event.defaultPrevented;});
  listen(window, 'keyup', () => {reverseTabEntry = false;});
  listen(window, 'pointermove', proximity);
  listen(window, 'pointerout', event => { if (!event.relatedTarget) proximity(null); });
  listen(window, 'scroll', () => { if (proximityPosition) proximity(proximityPosition); }, {capture:true});
  listen(window, 'resize', () => { if (button && host.isConnected) {change(() => {},true);if(proximityPosition)proximity(proximityPosition);} });
  listen(window, 'blur', () => { proximityPosition=null; cancel(); });

  return {
    measureText:(value,fontSize)=>runtime.text_metrics(value,fontSize),
    render({container, source, template = '', designMode = true, nodes = [], previewControls, selectedStart: nextSelection = null, reducedMotion = false}) {
      let candidate = null;
      try {
        if (destroyed) throw new Error('Векторный предпросмотр уже закрыт');
        if (!container?.replaceChildren) throw new Error('Не найден контейнер предпросмотра');
        designMode = Boolean(designMode);
        const changed = !button || current.source !== source || current.template !== template;
        const modeChanged = !changed && current.designMode !== designMode;
        if (changed) {
          candidate = new runtime.Runtime();
          candidate.load_component(source, template);
          if(button)candidate.preserve_interaction(button);
        }
        if(changed||current.reducedMotion!==reducedMotion)(candidate ?? button).set_reduced_motion?.(reducedMotion);
        if (controlDrag !== null) {
          const model=candidate ?? button;
          let dragIndex=-1;
          for(let i=0,count=model.control_count();i<count;i++)if(model.control_key(i)===controlDrag){dragIndex=i;break;}
          if (designMode || dragIndex<0 || !model.control_interactive(dragIndex) || model.control_disabled(dragIndex)) {
            controlDrag=null;releasePointer();
          }
        }
        // Validate and rasterize before touching the last good DOM or runtime.
        const image = modeChanged ? null : raster(candidate ?? button);
        if (changed) {
          cancelAnimationFrame(frame);
          frame = null;
          if (controlDrag === null) releasePointer();
          pressedKey = null;
          button?.free();
          button = candidate;
          candidate = null;
        } else if (modeChanged) {
          controlDrag=null;
          releasePointer();
          pressedKey = null;
          button.pointer(-1, -1, 3);
          button.focus(false);
          button.tick(1000);
        }
        current = {source, template, designMode, nodes, previewControls, reducedMotion};
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
        const count=button.control_count(),labels=[];
        let interactive=0,enabled=false;
        for(let i=0;i<count;i++){
          labels.push(button.control_label(i));
          if(button.control_interactive(i)){interactive++;if(!button.control_disabled(i))enabled=true;}
        }
        canvas.tabIndex = enabled ? 0 : -1;
        canvas.setAttribute('role',count===1?(interactive?'button':'img'):'group');
        canvas.setAttribute('aria-label',labels.join(', '));
        if (interactive) canvas.setAttribute('aria-disabled', String(!enabled));
        else canvas.removeAttribute('aria-disabled');
        if(modeChanged)paint();else paint(image);
        if (host.parentElement !== container) container.replaceChildren(host);
        updateCursor();
        updateSelection();
        syncTextInput();
        schedule();
        if (proximityPosition) proximity(proximityPosition);
        return true;
      } catch (error) {
        candidate?.free();
        report(error);
        return false;
      }
    },
    select(start) {
      selectedStart=start;
      const i=controlNodes().findIndex(n=>n.start===start);
      if(button?.scrollable()&&i>=0){
        const [x,y,w,h]=button.control_bounds(i);
        const dx=x<0?x:x+w>button.width()?x+w-button.width():0;
        const dy=y<0?y:y+h>button.height()?y+h-button.height():0;
        if(dx||dy)change(()=>button.scroll(dx,dy));
      }
      updateSelection();
    },
    focusKey(key) {
      if (!button || current.designMode) return false;
      const index = controlNodes().findIndex(node => node.props.key === key);
      if (!enabledControl(index)) return false;
      canvas.focus({preventScroll:true});
      change(() => button.focus_control?.(index));
      return true;
    },
    activate,
    // Canvas tools need geometry, not strings, text values and interaction state
    // for every control. Reuse an immutable snapshot until the model changes.
    layoutSnapshot() {
      if(!button||!current)return null;
      const revision=button.layout_revision?.()??button.visual_revision();
      if(layoutModel===button&&layoutRevision===revision)return layout;
      layout=Object.freeze({width:button.width(),height:button.height(),clip:button.clipped(),
        scrollable:button.scrollable(),scrollOffset:Object.freeze(Array.from(button.scroll_offset())),
        controls:Object.freeze(Array.from({length:button.control_count()},(_,index)=>Object.freeze({index,bounds:Object.freeze(Array.from(button.control_bounds(index)))})))});
      layoutModel=button;layoutRevision=revision;
      return layout;
    },
    snapshot() {
      return button && current ? {...current, nodes: undefined, renderer: gpu?'rust-wasm-webgpu':'rust-wasm-cpu',gpuStats:gpu?.snapshot()??null,gpuFallbackReason:gpuFailure||null, rasterStats: Array.from(button.raster_stats()), selectedStart, focusedIndex:button.focused_index(),controls:Array.from({length:button.control_count()},(_,i)=>({index:i,key:button.control_key(i),label:button.control_label(i),bounds:Array.from(button.control_bounds(i)),disabled:button.control_disabled(i),interactive:button.control_interactive(i),editable:button.control_editable?.(i)??false,value:button.control_editable?.(i)?button.text_value(i):undefined,hovered:button.control_hovered(i),focused:button.control_focused(i),clicks:button.control_clicks(i),action:button.control_action(i)})), width: button.width(), height: button.height(), clip:button.clipped(),radius:button.frame_radius(),overflow:button.overflow(),scrollable:button.scrollable(),renderWidth:button.render_width(),renderHeight:button.render_height(),scrollOffset:Array.from(button.scroll_offset()), bounds: Array.from(button.bounds()), label: button.label(), key: button.key(), action: button.action(), disabled: button.disabled(), clicks: button.clicks()} : null;
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
      // Callers can retain the preview API after closing it; release canvas
      // allocations explicitly instead of waiting for the closure to be GC'd.
      canvas.width=canvas.height=1;gpuCanvas.width=gpuCanvas.height=1;
      button?.free();
      button = null;
      current = null;
      paintedModel=null;paintedGpu=null;
      layoutModel=null;layout=null;layoutRevision=null;
    },
  };
}
