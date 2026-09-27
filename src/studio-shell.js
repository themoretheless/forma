import {createCacheBudget} from './cache-budget.js';
import {createComponentCompiler} from './components.js';
import {loadVectorRuntime} from './vector-preview.js';
import {shellControl,shellScene} from './studio-shell-controls.js';
import {materializeTheme,themes} from '../vector-ui/controls/tokens.js';
import './studio-shell.css';
import {createShellPainter} from './studio-shell-painter.js';
import {canvasIcon} from './studio-canvas-icons.js';

const raw=import.meta.glob('../vector-ui/controls/{components,assets}/*',{query:'?raw',import:'default',eager:true});
const library=materializeTheme(Object.fromEntries(Object.entries(raw).map(([path,value])=>[path.slice('../vector-ui/controls/'.length),value])),'dark');
const selector='button,a.controls-link,.control-tree-row,label:has(>input[type="checkbox"]),input:not([type="checkbox"]):not([type="file"]):not([type="hidden"]),select,#filename,footer,#output>p.error,#output>p.ok';

// The DOM remains the layout, keyboard, accessibility and browser-input host.
// Every decorated surface is rasterized from the shared Forma component files.
// Editor content, the design canvas, popups and native file choosers are excluded.
export async function mountStudioShell(root){
 for(const [name,value]of Object.entries(themes.dark))document.documentElement.style.setProperty('--guide-'+name,value);
 document.body.classList.add('forma-studio-ui');
 const wasm=await loadVectorRuntime(),cache=createCacheBudget(),compile=createComponentCompiler({cache});
 const previewTools=root.querySelector('.preview-tools'),canvasTools=root.querySelector('.canvas-tools');
 let compactToolbar=null;if(previewTools&&canvasTools){compactToolbar=document.createElement('div');compactToolbar.className='studio-canvas-toolbar';previewTools.before(compactToolbar);compactToolbar.append(previewTools,canvasTools);}
 const records=new Map();let dirty=new Set(),spare=new Set();const listeners=new AbortController();let frame=0,lastTime=0,stopped=false,proximityPosition=null,proximityDirty=false;
 const painter=createShellPainter(document),compileFiles={...library},metrics={measureText:wasm.text_metrics},motion=matchMedia('(prefers-reduced-motion: reduce)');
 const listen=(target,event,callback)=>target.addEventListener(event,callback,{signal:listeners.signal});
 function eligible(el){return el.matches(selector)&&!el.closest('#preview,.cm-editor,.color-picker,#native-inspection')&&!el.hidden&&!(el.tagName==='INPUT'&&el.closest('.source-editor'));}
 function kind(el){
  if(canvasIcon(el))return 'icon';
  if(el.matches('.element-tools button'))return 'menu';
  if(el.matches('#output>p'))return 'notice';
  if(el.tagName==='FOOTER')return 'status';
  if(el.tagName==='INPUT')return 'field';
  if(el.tagName==='SELECT')return 'select';
  if(el.tagName==='LABEL')return 'check';
  if(el.matches('.file,.control-tree-row'))return 'tree';
  if(el.matches('#filename,[data-tab],.control-tree-scopes button,[aria-pressed],#desktop,#mobile,#toggle-control-tree'))return 'tab';
  if(el.matches('.accent,#native-run'))return 'primary';
  if(el.matches('button')&&/^[+−⊞⊟■☷]$/.test(el.textContent.trim()))return 'icon';
  if(el.matches('.controls-link,.layout-parts button,.layout-breadcrumbs button'))return 'ghost';
  return 'button';
 }
 function describe(el){
  const k=kind(el),bounds=el.getBoundingClientRect();
  let text=el.textContent.replace(/\s+/g,' ').trim();
  if(k!=='tree')text=text.replace(/^[▶⌖↻↺☷◐◇↑↓✓●]+\s*/u,'');
  if(k==='tree')text=el.matches('.file')?el.dataset.path.split('/').at(-1):[el.querySelector('.control-tree-label')?.textContent,el.querySelector('.control-tree-detail')?.textContent].filter(Boolean).join(' ');
  if(k==='select')text=el.selectedOptions[0]?.textContent??'';
  if(k==='status')text=[el.querySelector('#status')?.textContent,'Forma UI · UTF-8',el.querySelector('#position')?.textContent].join('\n');
  const input=k==='check'?el.querySelector('input'):el;
  return shellControl({kind:k,text:k==='icon'?'':text,icon:canvasIcon(el)?`assets/${canvasIcon(el)[1]}.svg`:/[⊟−]/.test(el.textContent)?'assets/minus.svg':el.textContent.includes('■')?'assets/close.svg':el.textContent.includes('☷')?'assets/folder.svg':'assets/plus.svg',tone:el.classList.contains('error')?'danger':'success',width:Math.ceil(bounds.width),height:Math.ceil(bounds.height),selected:el.matches('.active,.chosen')||el.getAttribute('aria-selected')==='true'||el.getAttribute('aria-pressed')==='true'||el.getAttribute('aria-checked')==='true'||(el.matches('.layout-toggle,#toggle-control-tree')&&el.getAttribute('aria-expanded')==='true'),disabled:!!input.disabled||el.getAttribute('aria-disabled')==='true',checked:!!input.checked,branch:el.hasAttribute('aria-expanded'),expanded:el.getAttribute('aria-expanded')==='true',indent:Math.max(0,(Number(el.getAttribute('aria-level')??1)-1)*14)});
 }
 function request(el,invalidate=false){if(stopped)return;const rec=records.get(el);if(rec&&invalidate)rec.specDirty=true;if(rec?.visible)dirty.add(el);if(!frame&&dirty.size)frame=requestAnimationFrame(draw);}
 function release(rec){rec.model?.free();rec.model=null;rec.signature='';rec.paintKey=null;}
 function fail(el,rec,error){release(rec);rec.surface.suspend();el.classList.remove('forma-native-surface');el.style.removeProperty('--forma-surface');delete el.dataset.formaComponent;rec.failed=String(error);}
 function updateReveal(el,model){
  const bounds=el.getBoundingClientRect(),point=proximityPosition;
  model.reveal_pointer(point?point.x-bounds.left:0,point?point.y-bounds.top:0,!!point);
 }
 function proximity(event){
  proximityPosition=event&&event.pointerType!=='touch'?{x:event.clientX,y:event.clientY}:null;
  proximityDirty=true;if(!frame&&!stopped)frame=requestAnimationFrame(draw);
 }
 function draw(time){
  frame=0;const dt=Math.min(50,lastTime?time-lastTime:16);lastTime=time;
  if(proximityDirty){proximityDirty=false;for(const [el,rec]of records){
   if(!rec.visible||!rec.model)continue;
   const revision=rec.model.visual_revision();updateReveal(el,rec.model);
   if(rec.model.visual_revision()!==revision||rec.model.is_animating())dirty.add(el);
  }}
  const batch=dirty;dirty=spare;dirty.clear();
  for(const el of batch){const rec=records.get(el);if(!rec?.visible||!el.isConnected)continue;
   try{
    if(rec.specDirty||!rec.spec){rec.spec=describe(el);rec.nextSignature=JSON.stringify(rec.spec);rec.specDirty=false;}
    const spec=rec.spec,signature=rec.nextSignature;if(spec.props.width<=1||spec.props.height<=1)continue;
    if(signature!==rec.signature){
     compileFiles['ui/StudioControl.ui']=shellScene(spec);
     const out=compile(compileFiles,'ui/StudioControl.ui',{},metrics);
     const next=new wasm.Runtime();try{next.load_component(out.source,out.template);}catch(error){next.free();throw error;}
     if(rec.model)next.preserve_interaction(rec.model);release(rec);rec.model=next;rec.signature=signature;
     next.set_reduced_motion(motion.matches);updateReveal(el,next);
    }
    const model=rec.model;model.tick(dt);
    const scale=Math.min(2,devicePixelRatio||1),width=Math.ceil(spec.props.width*scale),height=Math.ceil(spec.props.height*scale);
    const revision=model.visual_revision(),paintKey=`${revision}:${width}:${height}`;
    if(signature!==rec.paintedSignature||paintKey!==rec.paintKey){
     rec.surface.paint(model.content_pixels(width,height,scale),width,height);el.dataset.formaComponent=spec.type;
     rec.paintKey=paintKey;rec.paintedSignature=signature;
    }
    if(model.is_animating())dirty.add(el);
   }catch(error){fail(el,rec,error);}
  }
  batch.clear();spare=batch;
  if(dirty.size)frame=requestAnimationFrame(draw);
 }
 const resize=new ResizeObserver(entries=>{for(const {target}of entries)request(target,true);});
 const visible=new IntersectionObserver(entries=>{for(const entry of entries){const rec=records.get(entry.target);if(!rec)continue;rec.visible=entry.isIntersecting;if(rec.visible)request(entry.target,true);else{release(rec);rec.surface.suspend();}}});
 function register(el){if(records.has(el)||!eligible(el))return;const icon=canvasIcon(el);if(icon){el.classList.add('studio-canvas-icon');el.setAttribute('aria-label',icon[2]);el.title=icon[2]+(el.title&&el.title!==icon[2]?' — '+el.title:'');}
 records.set(el,{visible:false,model:null,signature:'',specDirty:true,surface:painter.surface(el)});el.classList.add('forma-host-control');if(kind(el)==='field')el.classList.add('forma-host-input');resize.observe(el);visible.observe(el);}
 function scan(){
  root.querySelectorAll(selector).forEach(register);
  for(const [el,rec]of records)if(!el.isConnected){release(rec);rec.surface.destroy();resize.unobserve(el);visible.unobserve(el);records.delete(el);dirty.delete(el);}
 }
 const mutations=new MutationObserver(entries=>{
  let removed=false;
  for(const entry of entries){
   const target=entry.target.nodeType===1?entry.target:entry.target.parentElement;
   if(target?.closest('#preview,.cm-editor,.color-picker,#native-inspection'))continue;
   if(entry.type==='childList'){
    for(const node of entry.addedNodes){if(node.nodeType!==1||node.matches('.forma-control-paint'))continue;register(node);node.querySelectorAll(selector).forEach(register);}
    if(entry.removedNodes.length)removed=true;
    if([...entry.addedNodes,...entry.removedNodes].every(n=>n.nodeType===1&&n.matches('.forma-control-paint')))continue;
   }
   for(let el=target;el&&el!==root;el=el.parentElement)if(records.has(el)){request(el,true);break;}
  }
  if(removed)for(const [el,rec]of records)if(!el.isConnected){release(rec);rec.surface.destroy();resize.unobserve(el);visible.unobserve(el);records.delete(el);dirty.delete(el);}
 });
 mutations.observe(root,{subtree:true,childList:true,characterData:true,attributes:true,attributeFilter:['disabled','aria-disabled','aria-selected','aria-pressed','aria-checked','aria-expanded','class','value']});
 function owner(target){for(let el=target;el&&el!==root;el=el.parentElement)if(records.has(el))return el;}
 function pointer(event,phase){const el=owner(event.target),rec=records.get(el);if(!rec?.model||kind(el)==='field')return;
  const bounds=el.getBoundingClientRect();rec.model.pointer(event.clientX-bounds.left,event.clientY-bounds.top,phase);if(event.pointerType==='touch')rec.model.reveal_pointer(0,0,false);request(el);
 }
 listen(window,'pointermove',proximity);
 listen(window,'pointerout',e=>{if(!e.relatedTarget)proximity(null);});
 listen(window,'blur',()=>proximity(null));
 listen(window,'resize',()=>{for(const el of records.keys())request(el,true);proximityDirty=true;if(!frame&&!stopped)frame=requestAnimationFrame(draw);});
 root.addEventListener('scroll',()=>{proximityDirty=true;if(!frame&&!stopped)frame=requestAnimationFrame(draw);},{capture:true,signal:listeners.signal});
 listen(root,'pointermove',e=>pointer(e,0));listen(root,'pointerdown',e=>pointer(e,1));listen(root,'pointerup',e=>pointer(e,2));
 listen(root,'pointerout',e=>{const el=owner(e.target);if(el&&!el.contains(e.relatedTarget)){const model=records.get(el)?.model;model?.pointer(-1000,-1000,3);if(model)updateReveal(el,model);request(el);}});
 listen(root,'focusin',e=>{const el=owner(e.target);if(el&&kind(el)!=='field')records.get(el)?.model?.focus(true);request(el);});
 listen(root,'focusout',e=>{const el=owner(e.target);records.get(el)?.model?.focus(false);request(el);});
 for(const event of ['keydown','keyup'])listen(root,event,e=>{const el=owner(e.target);if(!el?.matches('button,a.controls-link')||![' ','Enter'].includes(e.key))return;records.get(el)?.model?.key_event(e.key===' '?1:2,event==='keydown',e.repeat);request(el);});
 for(const event of ['change','input','click'])listen(root,event,e=>{const el=owner(e.target);request(el,true);});
 listen(motion,'change',()=>{for(const [el,rec]of records){rec.model?.set_reduced_motion(motion.matches);request(el);}});
 scan();
 return {destroy(){stopped=true;cancelAnimationFrame(frame);listeners.abort();mutations.disconnect();resize.disconnect();visible.disconnect();for(const [el,rec]of records){release(rec);rec.surface.destroy();el.classList.remove('forma-native-surface','forma-host-control','forma-host-input');el.style.removeProperty('--forma-surface');delete el.dataset.formaComponent;}records.clear();dirty.clear();spare.clear();cache.clear();painter.destroy();if(compactToolbar)compactToolbar.replaceWith(...compactToolbar.childNodes);document.body.classList.remove('forma-studio-ui');},snapshot(){return [...records].map(([el,rec])=>({type:el.dataset.formaComponent,visible:rec.visible,error:rec.failed??null}));}};
}
