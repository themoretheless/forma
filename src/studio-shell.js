import {createCacheBudget} from './cache-budget.js';
import {createComponentCompiler} from './components.js';
import {loadVectorRuntime} from './vector-preview.js';
import {shellControl,shellScene} from './studio-shell-controls.js';
import {materializeTheme,themes} from '../vector-ui/controls/tokens.js';
import './studio-shell.css';
import {createShellPainter} from './studio-shell-painter.js';
import {canvasIcon,glyphIcon} from './studio-canvas-icons.js';
import {createEventScope} from './event-scope.js';
import {collapseText,glyphPrefix} from './control-label.js';
import {createKeyedPool} from './keyed-pool.js';

const raw=import.meta.glob('../vector-ui/controls/{components,assets}/*',{query:'?raw',import:'default',eager:true});
const library=materializeTheme(Object.fromEntries(Object.entries(raw).map(([path,value])=>[path.slice('../vector-ui/controls/'.length),value])),'dark');
// Surfaces that draw themselves: the design canvas, the code editor, pickers and native inspection.
const foreign='#preview,.cm-editor,.color-picker,#native-inspection';
// Milliseconds of shell painting per animation frame.
const frameBudget=8;
const selector='button,a.controls-link,.control-tree-row,label:has(>input[type="checkbox"]),input:not([type="checkbox"]):not([type="file"]):not([type="hidden"]),select,#filename,footer,#output>p.error,#output>p.ok';

// The DOM remains the layout, keyboard, accessibility and browser-input host.
// Every decorated surface is rasterized from the shared Forma component files.
// Editor content, the design canvas, popups and native file choosers are excluded.
export async function mountStudioShell(root){
 for(const [name,value]of Object.entries(themes.dark))document.documentElement.style.setProperty('--guide-'+name,value);
 document.body.classList.add('forma-studio-ui');
 const wasm=await loadVectorRuntime(),cache=createCacheBudget(),compile=createComponentCompiler({cache});
 // A control's compiled scene depends only on its description: the inspector rebuilds its rows on
 // every selection, so the same field, button or tree row comes back with the same signature and
 // skips the compiler. Its own budget keeps these from evicting the library's parsed documents.
 const scenes=createCacheBudget({maxBytes:4*1024*1024,maxEntries:512});
 function compiledScene(spec,signature){
  let out=scenes.get(signature);
  if(!out){compileFiles['ui/StudioControl.ui']=shellScene(spec);const compiled=compile(compileFiles,'ui/StudioControl.ui',{},metrics);out={source:compiled.source,template:compiled.template};scenes.set(signature,out);}
  return out;
 }
 const previewTools=root.querySelector('.preview-tools'),canvasTools=root.querySelector('.canvas-tools');
 let compactToolbar=null;if(previewTools&&canvasTools){compactToolbar=document.createElement('div');compactToolbar.className='studio-canvas-toolbar';previewTools.before(compactToolbar);compactToolbar.append(previewTools,canvasTools);}
 const records=new Map();let dirty=new Set(),spare=new Set();const scope=createEventScope();let frame=0,lastTime=0,stopped=false,proximityPosition=null,proximityDirty=false;
 const painter=createShellPainter(document),compileFiles={...library},metrics={measureText:wasm.text_metrics},motion=matchMedia('(prefers-reduced-motion: reduce)');
 const listen=scope.listen;
 // Inspector, tree and toolbar rows come back as new elements on every selection, mostly with the
 // descriptions they had. A control that leaves the page or the viewport parks its model and its
 // painted surface, at rest, and the next control with the same description takes both instead of
 // loading a model and rasterizing it again. A control that stays keeps only its own last state
 // (`previous`), so one whose text changes on every keystroke cannot crowd out the pool.
 const parked=createKeyedPool({maxEntries:256,maxBytes:16*1024*1024,discard:({model,paint})=>{model.free();painter.discard(paint);}});
 function eligible(el){return el.matches(selector)&&!el.closest(foreign)&&!el.hidden&&!(el.tagName==='INPUT'&&el.closest('.source-editor'));}
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
  const glyph=el.tagName==='BUTTON'?glyphIcon(el.textContent):null,accent=el.matches('.accent,#native-run');
  if(glyph)return glyph.text?(accent?'primaryLeading':'leading'):(accent?'accentIcon':'icon');
  if(accent)return 'primary';
  if(el.matches('button')&&/^[+−⊞⊟■☷]$/.test(el.textContent.trim()))return 'icon';
  if(el.matches('.controls-link,.layout-parts button,.layout-breadcrumbs button'))return 'ghost';
  return 'button';
 }
 function describe(el,bounds){
  const k=kind(el);
  let text=collapseText(el.textContent);
  const glyph=glyphIcon(el.textContent);
  if(k==='leading'||k==='primaryLeading')text=glyph.text;
  else if(k!=='tree')text=text.replace(glyphPrefix,'');
  if(k==='tree')text=el.matches('.file')?el.dataset.path.split('/').at(-1):[el.querySelector('.control-tree-label')?.textContent,el.querySelector('.control-tree-detail')?.textContent].filter(Boolean).join(' ');
  if(k==='select')text=el.selectedOptions[0]?.textContent??'';
  if(k==='status')text=[el.querySelector('#status')?.textContent,'Forma UI · UTF-8',el.querySelector('#position')?.textContent].join('\n');
  const input=k==='check'?el.querySelector('input'):el;
  return shellControl({kind:k,text:k==='icon'||k==='accentIcon'?'':text,icon:canvasIcon(el)?`assets/${canvasIcon(el)[1]}.svg`:glyph?`assets/${glyph.icon}.svg`:/[⊟−]/.test(el.textContent)?'assets/minus.svg':el.textContent.includes('■')?'assets/close.svg':el.textContent.includes('☷')?'assets/folder.svg':'assets/plus.svg',tone:el.matches('.error,.bad')?'danger':'success',width:Math.ceil(bounds.width),height:Math.ceil(bounds.height),selected:el.matches('.active,.chosen')||el.getAttribute('aria-selected')==='true'||el.getAttribute('aria-pressed')==='true'||el.getAttribute('aria-checked')==='true'||(el.matches('.layout-toggle,#toggle-control-tree')&&el.getAttribute('aria-expanded')==='true'),disabled:!!input.disabled||el.getAttribute('aria-disabled')==='true',checked:!!input.checked,branch:el.hasAttribute('aria-expanded'),expanded:el.getAttribute('aria-expanded')==='true',indent:Math.max(0,(Number(el.getAttribute('aria-level')??1)-1)*14)});
 }
 function request(el,invalidate=false){if(stopped)return;const rec=records.get(el);if(rec&&invalidate)rec.specDirty=true;if(rec?.visible)dirty.add(el);if(!frame&&dirty.size)frame=requestAnimationFrame(draw);}
 function release(rec){rec.model?.free();rec.model=null;rec.signature='';rec.paintKey=null;}
 const forget=item=>{if(item){item.model.free();painter.discard(item.paint);}};
 const park=item=>{if(item)parked.put(item.signature,item,painter.bytes(item.paint));};
 // A control's model and painted surface, reset to rest and lifted off it, or null when they are
 // not worth keeping: never painted in this description, or still animating.
 function stash(rec,keepHost){
  const model=rec.model;
  if(!model||rec.paintedSignature!==rec.signature)return null;
  model.pointer(-1000,-1000,3);model.focus(false);
  const paint=!model.is_animating()&&rec.surface.take(keepHost);
  if(!paint)return null;
  model.release_cpu_cache();
  const item={signature:rec.signature,model,paint,paintKey:rec.paintKey};rec.model=null;release(rec);return item;
 }
 // A control that leaves the page or the viewport parks what it showed, and what it showed before
 // that, for the next control with the same description.
 function retire(rec){
  const item=stash(rec,false);if(item)park(item);else{release(rec);rec.surface.suspend();}
  park(rec.previous);rec.previous=null;
 }
 function fail(el,rec,error){release(rec);forget(rec.previous);rec.previous=null;rec.surface.suspend();el.classList.remove('forma-native-surface');el.style.removeProperty('--forma-surface');delete el.dataset.formaComponent;rec.failed=String(error);}
 function updateReveal(model,bounds){
  const point=proximityPosition;
  model.reveal_pointer(point?point.x-bounds.left:0,point?point.y-bounds.top:0,!!point);
 }
 function proximity(event){
  proximityPosition=event&&event.pointerType!=='touch'?{x:event.clientX,y:event.clientY}:null;
  proximityDirty=true;if(!frame&&!stopped)frame=requestAnimationFrame(draw);
 }
 function draw(time){
  frame=0;const dt=Math.min(50,lastTime?time-lastTime:16);lastTime=time;const started=performance.now();
  if(proximityDirty){proximityDirty=false;for(const [el,rec]of records){
   if(!rec.visible||!rec.model)continue;
   const revision=rec.model.visual_revision();updateReveal(rec.model,el.getBoundingClientRect());
   if(rec.model.visual_revision()!==revision||rec.model.is_animating())dirty.add(el);
  }}
  const batch=dirty;dirty=spare;dirty.clear();
  // Reads before writes: every box is measured before the first adopted or painted surface touches
  // the DOM, so a rebuilt panel costs one layout instead of one per control.
  for(const el of batch){const rec=records.get(el);if(rec?.visible&&el.isConnected)rec.bounds=el.getBoundingClientRect();}
  for(const el of batch){const rec=records.get(el);if(!rec?.visible||!el.isConnected)continue;
   // A cold start or a rebuilt panel can need dozens of compiles: past the frame budget the rest
   // waits for the next frame instead of stretching this one into a long task.
   if(performance.now()-started>frameBudget){dirty.add(el);continue;}
   try{
    if(rec.specDirty||!rec.spec){rec.spec=describe(el,rec.bounds);rec.nextSignature=JSON.stringify(rec.spec);rec.specDirty=false;}
    const spec=rec.spec,signature=rec.nextSignature;if(spec.props.width<=1||spec.props.height<=1)continue;
    if(signature!==rec.signature){
     // The model and paint for this description come from the state the control showed last, such
     // as a label alternating between two texts, or from a control like it that was parked; only
     // without either is a model loaded. The model it leaves carries its interaction over and is
     // kept as its last state in turn.
     const item=rec.previous?.signature===signature?rec.previous:parked.take(signature);
     if(item&&item===rec.previous)rec.previous=null;
     let next=item?.model;
     if(!next){const out=compiledScene(spec,signature);next=new wasm.Runtime();try{next.load_component(out.source,out.template);}catch(error){next.free();throw error;}}
     if(rec.model){next.preserve_interaction(rec.model);const last=stash(rec,true);if(last){forget(rec.previous);rec.previous=last;}else release(rec);}
     if(item&&rec.surface.adopt(item.paint)){rec.paintKey=item.paintKey;rec.paintedSignature=signature;el.dataset.formaComponent=spec.type;}
     else if(item)painter.discard(item.paint);
     rec.model=next;rec.signature=signature;next.set_reduced_motion(motion.matches);updateReveal(next,rec.bounds);
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
 // Both observers report every new control once with what it already is; only a change is news.
 const resize=scope.resize(entries=>{for(const entry of entries){const size=entry.borderBoxSize?.[0],props=records.get(entry.target)?.spec?.props;
  if(!size||!props||Math.ceil(size.inlineSize)!==props.width||Math.ceil(size.blockSize)!==props.height)request(entry.target,true);}});
 const visible=scope.intersect(entries=>{for(const entry of entries){const rec=records.get(entry.target);if(!rec||rec.visible===entry.isIntersecting)continue;rec.visible=entry.isIntersecting;if(rec.visible)request(entry.target,true);else retire(rec);}});
 function register(el){if(records.has(el)||!eligible(el))return;const icon=canvasIcon(el);if(icon){el.classList.add('studio-canvas-icon');el.setAttribute('aria-label',icon[2]);el.title=icon[2]+(el.title&&el.title!==icon[2]?' — '+el.title:'');}
 // A new control is drawn in the next frame, with the rest of the change that added it, rather than
 // one frame later once the visibility observer has reported; the observer retires it if it is not.
 records.set(el,{visible:true,model:null,signature:'',specDirty:true,surface:painter.surface(el)});el.classList.add('forma-host-control');if(kind(el)==='field')el.classList.add('forma-host-input');resize.observe(el);visible.observe(el);request(el);}
 // A control that left the document gives back its Rust model, surface and observers.
 function prune(){for(const [el,rec]of records)if(!el.isConnected){retire(rec);rec.surface.destroy();resize.unobserve(el);visible.unobserve(el);records.delete(el);dirty.delete(el);}}
 function scan(){
  root.querySelectorAll(selector).forEach(register);
  prune();
 }
 // Classes the shell puts on its own hosts. A change to them alone says nothing new about a control,
 // and answering it would describe every control the shell has just painted once more.
 const ownClasses=new Set(['forma-host-control','forma-host-input','forma-native-surface','studio-canvas-icon']);
 const others=classes=>classes.filter(name=>name&&!ownClasses.has(name));
 function ownClassChange(entry){
  if(entry.attributeName!=='class')return false;
  const before=new Set(others(String(entry.oldValue??'').split(/\s+/))),after=others([...entry.target.classList]);
  return after.length===before.size&&after.every(name=>before.has(name));
 }
 const mutations=scope.observe(root,entries=>{
  let removed=false,changed=false;
  for(const entry of entries){
   const target=entry.target.nodeType===1?entry.target:entry.target.parentElement;
   if(target?.closest(foreign)||entry.type==='attributes'&&ownClassChange(entry))continue;
   if(entry.type==='childList'){
    for(const node of entry.addedNodes){if(node.nodeType!==1||node.matches('.forma-control-paint'))continue;register(node);node.querySelectorAll(selector).forEach(register);}
    if(entry.removedNodes.length)removed=true;
    if([...entry.addedNodes,...entry.removedNodes].every(n=>n.nodeType===1&&n.matches('.forma-control-paint')))continue;
   }
   changed=true;
   for(let el=target;el&&el!==root;el=el.parentElement)if(records.has(el)){request(el,true);break;}
  }
  if(removed)prune();
  // A change can move controls under a pointer that stays put, and their proximity paint follows
  // the pointer's place relative to each control, so it is worked out again from the new boxes.
  if(changed&&proximityPosition&&!stopped){proximityDirty=true;if(!frame)frame=requestAnimationFrame(draw);}
  },{subtree:true,childList:true,characterData:true,attributes:true,attributeOldValue:true,attributeFilter:['disabled','aria-disabled','aria-selected','aria-pressed','aria-checked','aria-expanded','class','value']},{immediate:false});
 function owner(target){for(let el=target;el&&el!==root;el=el.parentElement)if(records.has(el))return el;}
 function pointer(event,phase){const el=owner(event.target),rec=records.get(el);if(!rec?.model||kind(el)==='field')return;
  const bounds=el.getBoundingClientRect();rec.model.pointer(event.clientX-bounds.left,event.clientY-bounds.top,phase);if(event.pointerType==='touch')rec.model.reveal_pointer(0,0,false);request(el);
 }
 listen(window,'pointermove',proximity);
 listen(window,'pointerout',e=>{if(!e.relatedTarget)proximity(null);});
 listen(window,'blur',()=>proximity(null));
 listen(window,'resize',()=>{for(const el of records.keys())request(el,true);proximityDirty=true;if(!frame&&!stopped)frame=requestAnimationFrame(draw);});
 listen(root,'scroll',()=>{proximityDirty=true;if(!frame&&!stopped)frame=requestAnimationFrame(draw);},{capture:true});
 listen(root,'pointermove',e=>pointer(e,0));listen(root,'pointerdown',e=>pointer(e,1));listen(root,'pointerup',e=>pointer(e,2));
 listen(root,'pointerout',e=>{const el=owner(e.target);if(el&&!el.contains(e.relatedTarget)){const model=records.get(el)?.model;model?.pointer(-1000,-1000,3);if(model)updateReveal(model,el.getBoundingClientRect());request(el);}});
 listen(root,'focusin',e=>{const el=owner(e.target);if(el&&kind(el)!=='field')records.get(el)?.model?.focus(true);request(el);});
 listen(root,'focusout',e=>{const el=owner(e.target);records.get(el)?.model?.focus(false);request(el);});
 for(const event of ['keydown','keyup'])listen(root,event,e=>{const el=owner(e.target);if(!el?.matches('button,a.controls-link')||![' ','Enter'].includes(e.key))return;records.get(el)?.model?.key_event(e.key===' '?1:2,event==='keydown',e.repeat);request(el);});
 for(const event of ['change','input','click'])listen(root,event,e=>{const el=owner(e.target);request(el,true);});
 listen(motion,'change',()=>{for(const [el,rec]of records){rec.model?.set_reduced_motion(motion.matches);request(el);}});
 scan();
 return {destroy(){stopped=true;cancelAnimationFrame(frame);scope.dispose();for(const [el,rec]of records){release(rec);forget(rec.previous);rec.surface.destroy();el.classList.remove('forma-native-surface','forma-host-control','forma-host-input');el.style.removeProperty('--forma-surface');delete el.dataset.formaComponent;}records.clear();dirty.clear();spare.clear();parked.clear();cache.clear();painter.destroy();if(compactToolbar)compactToolbar.replaceWith(...compactToolbar.childNodes);document.body.classList.remove('forma-studio-ui');},snapshot(){return [...records].map(([el,rec])=>({type:el.dataset.formaComponent,visible:rec.visible,error:rec.failed??null}));}};
}
