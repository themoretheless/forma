import {resolve} from './language.js';
import {gridProperties,gridStyles} from './grid.js';
import {takesCoordinates,coordinateShift} from './positioning.js';
import {sizeProperties,sizeValue} from './sizing.js';
import {expandStructure} from './component-semantics.js';
import {writePreviewBinding} from './preview-state.js';

// The HTML preview: the compiled tree becomes DOM inside `preview`. The editor passes the state
// the bindings read and write, the design patch lying over the base values, and four callbacks,
// so this module knows nothing about selection, the debugger, the journal or the file set.
//   interactive — clicks fire events instead of selecting; focus survives a re-render.
//   design      — the active design state's overrides by key, or null when running.
//   onSelect(sourceNode), onEvent(action,node,event), onLog(text), onChange() after a binding wrote state.
export function renderHtmlPreview({preview,compiled,state,interactive,design,onSelect,onEvent,onLog,onChange}){
preview.classList.remove('vector-artboard');preview.style.backgroundColor='';preview.style.borderRadius='';preview.style.minHeight='';const cssMap={'padding':'padding','margin':'margin','gap':'gap','overflow':'overflow','background':'background','color':'color','width':'width','height':'height','radius':'borderRadius','font.size':'fontSize','fontSize':'fontSize','opacity':'opacity','transition':'transition','transitionDuration':'transitionDuration'};
// Controls whose drawing the scene moves out of its flow slot, in document order so a moved
// container is measured after its own shift landed.
const freeNodes=[];
// One design file holds several states; the switcher index picks which patch lies over the base.
const activeDesign=design;
for(const kind of ['padding','margin'])for(const side of ['top','right','bottom','left'])cssMap[`${kind}.${side}`]=kind+side[0].toUpperCase()+side.slice(1);
function make(source,parent=null,parentEl=null){const override=activeDesign&&Object.hasOwn(activeDesign,source.props.key)?activeDesign[source.props.key]:null;const n={...source,props:{...source.props,...override}};const types={Frame:'div',Column:'div',Row:'div',Grid:'div',Stack:'div',Scroll:'div',Modal:'div',Text:'div',TextInput:'input',TextField:'input',Checkbox:'input',Slider:'input',Button:'button',Panel:'div'};if(!types[n.type])throw Error(`Компонент ${n.type} пока не поддерживается`);const el=document.createElement(types[n.type]);el.dataset.start=n.start;el.className='ui-node '+n.type.toLowerCase();if(n.type==='Checkbox')el.type='checkbox';if(n.type==='Slider')el.type='range';if(n.type==='Stack'){el.style.display='grid';}if(n.type==='Scroll')el.style.overflow='auto';if(n.type==='Modal'){const open=resolve(n.props.open??true,state);if(!open)el.hidden=true;el.style.display='flex';el.style.flexDirection='column';el.style.position='relative';el.style.zIndex='10';const backdrop=resolve(n.props.backdrop??'#00000080',state);el.style.boxShadow=`0 0 0 9999px ${backdrop}`;if((resolve(n.props.dismiss??true,state))&&n.events.dismissed)el.addEventListener('dblclick',e=>{if(interactive){e.stopPropagation();onEvent(n.events.dismissed,n,'dismissed');}});}if(n.type==='Frame'||n.type==='Column'||n.type==='Row'){el.style.display='flex';el.style.flexDirection=n.type==='Row'?'row':'column';}for(const [k,v]of Object.entries(n.props)){const x=resolve(v,state);if(k==='key'){el.dataset.key=x;continue;}if(n.type==='Modal'&&['open','dismiss','backdrop'].includes(k))continue;if(k==='wrap'){if(n.type!=='Text'||typeof x!=='boolean')throw Error('Text.wrap ожидает true или false');el.style.whiteSpace=x?'pre-wrap':'pre';continue;}if(k==='clip'){if(!['Frame','Row','Column','Grid','Stack'].includes(n.type)||typeof x!=='boolean')throw Error('Frame.clip ожидает true или false');if('overflow' in n.props)throw Error('Используйте clip или overflow, не оба');el.style.overflow=x?'clip':'visible';continue;}if(gridProperties.has(k)||k==='x'||k==='y')continue;else if(sizeProperties.has(k)){if(typeof x==='string'&&/^(?:\d+(?:\.\d+)?)?\*$/.test(x)&&((parent?.type==='Row'&&k==='width')||(['Column','Frame'].includes(parent?.type)&&k==='height'))){el.style.flex=`${x==='*'?1:Number(x.slice(0,-1))} 1 0px`;el.style[k==='width'?'minWidth':'minHeight']='0';}else el.style[k]=sizeValue(x);}else if(k==='text')el.textContent=x??'';else if(k==='value')el.value=x??'';else if(k==='placeholder')el.placeholder=x;else if(k==='disabled')el.disabled=!!x;else if(k==='checked'||k==='selected')el.checked=!!x;else if(k==='style')el.classList.add(String(x));else if(cssMap[k])el.style[cssMap[k]]=typeof x==='number'&&k!=='opacity'?x+'px':Array.isArray(v)?v.map(a=>typeof a==='number'?a+'px':resolve(a,state)).join(' '):x;else throw Error(`Свойство ${k} пока не поддерживается`);}
for(const [key,path]of Object.entries(n.bindings)){if(!['value','checked','selected'].includes(key))throw Error(`Неподдерживаемая привязка ${key} <-> ${path}`);const property=key==='value'?'value':'checked';el[property]=n.props[key]??(property==='value'?'':false);el.addEventListener('input',()=>{const value=property==='checked'?el.checked:n.type==='Slider'?Number(el.value):el.value;if(writePreviewBinding(state,n,path,value,{createMissing:true})){onLog(`${path} = ${JSON.stringify(value)}`);if(n.events.changed)onEvent(n.events.changed,n,'changed');onChange();}});}
if((n.type==='TextInput'||n.type==='TextField')&&n.events.submitted){el.addEventListener('keydown',e=>{if(interactive&&e.key==='Enter'){e.preventDefault();e.stopPropagation();onEvent(n.events.submitted,n,'submitted');}});}
el.addEventListener('click',e=>{e.stopPropagation();if(!interactive){onSelect(source);return;}if(n.events.clicked)onEvent(n.events.clicked,n);});Object.assign(el.style,gridStyles(n,state,parent));
if(takesCoordinates(n,parent))freeNodes.push([n,parent,parentEl,el]);
for(const child of n.children)el.append(make(child,n,el));
return el;}
const focused=document.activeElement;const focusIdentity=preview.contains(focused)?{key:focused.dataset.key,start:focused.dataset.start,selectionStart:focused.selectionStart,selectionEnd:focused.selectionEnd}:null;const fragment=document.createDocumentFragment();const expanded=expandStructure(compiled.nodes,compiled.defaults,state,{enums:{...compiled.enums,...compiled.structs},tokens:compiled.tokens,allowMissingState:true});for(const n of expanded)fragment.append(make(n));preview.replaceChildren(fragment);
if(preview._formaShortcutHandler){preview.removeEventListener('keydown',preview._formaShortcutHandler);preview._formaShortcutHandler=null;}
if(interactive&&compiled.shortcuts?.length){
  preview.tabIndex=preview.tabIndex>=0?preview.tabIndex:0;
  const matchShortcut=(e,combo)=>{
    const parts=String(combo).split('+').map(p=>p.trim().toLowerCase());
    const keyPart=parts.at(-1);
    const mods=new Set(parts.slice(0,-1));
    const wantCmdOrCtrl=mods.has('cmdorctrl')||mods.has('mod');
    const wantCtrl=mods.has('ctrl')||mods.has('control');
    const wantMeta=mods.has('cmd')||mods.has('meta');
    const wantShift=mods.has('shift');
    const wantAlt=mods.has('alt')||mods.has('option');
    if(wantCmdOrCtrl&&!(e.metaKey||e.ctrlKey))return false;
    if(!wantCmdOrCtrl&&wantCtrl!==!!e.ctrlKey)return false;
    if(!wantCmdOrCtrl&&wantMeta!==!!e.metaKey)return false;
    if(wantShift!==!!e.shiftKey)return false;
    if(wantAlt!==!!e.altKey)return false;
    return (e.key||'').toLowerCase()===keyPart;
  };
  preview._formaShortcutHandler=e=>{
    for(const s of compiled.shortcuts){
      if(matchShortcut(e,s.key)){
        e.preventDefault();
        const syntheticNode={type:'Shortcut',start:s.from??0,props:{},events:{shortcut:s.action},eventArgs:{shortcut:(s.args??[]).map(a=>resolve(a,state))},eventArgExpressions:{shortcut:s.args??[]},environment:{enums:{...compiled.enums,...compiled.structs},tokens:compiled.tokens,allowMissingState:true}};
        onEvent(s.action,syntheticNode,'shortcut');
        break;
      }
    }
  };
  preview.addEventListener('keydown',preview._formaShortcutHandler);
}
// A free control keeps its flow slot, so its offset is the distance between where the flow put
// it and where the scene puts it — measurable only once the nodes are in the document.
for(const [node,parent,parentEl,el]of freeNodes){
  const shift=coordinateShift(node,parent,state,parentEl.getBoundingClientRect(),el.getBoundingClientRect());
  if(shift){el.style.position='relative';for(const [css,px]of Object.entries(shift))el.style[css]=px+'px';}
}
if(focusIdentity&&interactive){const restored=Array.from(preview.querySelectorAll('[data-start]')).find(el=>focusIdentity.key?el.dataset.key===focusIdentity.key:el.dataset.start===focusIdentity.start);if(restored){restored.focus({preventScroll:true});if(typeof focusIdentity.selectionStart==='number'&&restored.setSelectionRange)restored.setSelectionRange(focusIdentity.selectionStart,focusIdentity.selectionEnd);}}}
