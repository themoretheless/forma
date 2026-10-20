import {resolve} from './language.js';
import {gridProperties,gridStyles} from './grid.js';
import {takesCoordinates,coordinateShift} from './positioning.js';
import {sizeProperties,sizeValue} from './sizing.js';
import {expandStructure} from './component-semantics.js';
import {writePreviewBinding,matchPreviewShortcut} from './preview-state.js';

const CSS_MAP={'padding':'padding','margin':'margin','gap':'gap','overflow':'overflow','background':'background','color':'color','width':'width','height':'height','radius':'borderRadius','font.size':'fontSize','fontSize':'fontSize','opacity':'opacity','transition':'transition','transitionDuration':'transitionDuration','borderColor':'borderColor','borderWidth':'borderWidth'};
for(const kind of ['padding','margin'])for(const side of ['top','right','bottom','left'])CSS_MAP[`${kind}.${side}`]=kind+side[0].toUpperCase()+side.slice(1);
const ELEMENT_TYPES={Frame:'div',Column:'div',Row:'div',Grid:'div',Stack:'div',Scroll:'div',Modal:'div',Text:'div',TextInput:'input',TextField:'input',Checkbox:'input',Slider:'input',Button:'button',Panel:'div',Badge:'span',Tag:'span',Chip:'span',Avatar:'div',Divider:'hr',Separator:'hr',ProgressBar:'div'};
const BADGE_TONES={accent:['#2a3a66','#9db5ff'],success:['#1d4233','#86cfa7'],warning:['#48381d','#f0c674'],danger:['#4b232a','#f2959f'],neutral:['#272f3f','#a7b4ca']};
const WEIGHT_STAR_RE=/^(?:\d+(?:\.\d+)?)?\*$/;

// The HTML preview: the compiled tree becomes DOM inside `preview`. The editor passes the state
// the bindings read and write, the design patch lying over the base values, and four callbacks,
// so this module knows nothing about selection, the debugger, the journal or the file set.
//   interactive — clicks fire events instead of selecting; focus survives a re-render.
//   design      — the active design state's overrides by key, or null when running.
//   onSelect(sourceNode), onEvent(action,node,event), onLog(text), onChange() after a binding wrote state.
export function renderHtmlPreview({preview,compiled,state,interactive,design,onSelect,onEvent,onLog,onChange}){
preview.classList.remove('vector-artboard');preview.style.backgroundColor='';preview.style.borderRadius='';preview.style.minHeight='';
// Controls whose drawing the scene moves out of its flow slot, in document order so a moved
// container is measured after its own shift landed.
const freeNodes=[];
// One design file holds several states; the switcher index picks which patch lies over the base.
const activeDesign=design;
function make(source,parent=null,parentEl=null){const override=activeDesign&&Object.hasOwn(activeDesign,source.props.key)?activeDesign[source.props.key]:null;const n={...source,props:override?{...source.props,...override}:source.props};const tag=ELEMENT_TYPES[n.type];if(!tag)throw Error(`Компонент ${n.type} пока не поддерживается`);const el=document.createElement(tag);el.dataset.start=n.start;el.className='ui-node '+n.type.toLowerCase();if(n.type==='Checkbox')el.type='checkbox';if(n.type==='Slider')el.type='range';if(n.type==='Stack'){el.style.display='grid';}if(n.type==='Scroll')el.style.overflow='auto';if(n.type==='Modal'){const open=resolve(n.props.open??true,state);if(!open)el.hidden=true;el.style.display='flex';el.style.flexDirection='column';el.style.position='relative';el.style.zIndex='10';const backdrop=resolve(n.props.backdrop??'#00000080',state);el.style.boxShadow=`0 0 0 9999px ${backdrop}`;if((resolve(n.props.dismiss??true,state))&&n.events.dismissed)el.addEventListener('dblclick',e=>{if(interactive){e.stopPropagation();onEvent(n.events.dismissed,n,'dismissed');}});}if(n.type==='Frame'||n.type==='Column'||n.type==='Row'){el.style.display='flex';el.style.flexDirection=n.type==='Row'?'row':'column';}if(n.type==='Badge'||n.type==='Tag'||n.type==='Chip'){el.style.display='inline-flex';el.style.alignItems='center';el.style.justifyContent='center';el.style.padding='2px 10px';el.style.borderRadius='999px';el.style.fontSize='11px';el.style.fontWeight='600';const tone=resolve(n.props.tone??'neutral',state);const [bg,fg]=BADGE_TONES[tone]??BADGE_TONES.neutral;el.style.background=bg;el.style.color=fg;}if(n.type==='Avatar'){el.style.display='inline-flex';el.style.alignItems='center';el.style.justifyContent='center';el.style.width='36px';el.style.height='36px';el.style.borderRadius='18px';el.style.fontSize='12px';el.style.fontWeight='600';el.style.background='#8fa8ff';el.style.color='#14213b';}if(n.type==='Divider'||n.type==='Separator'){el.style.border='0';el.style.height='1px';el.style.width='100%';el.style.background='#30394a';el.style.margin='4px 0';}if(n.type==='ProgressBar'){el.style.width='100%';el.style.height='6px';el.style.borderRadius='3px';el.style.background='#272f3f';el.style.overflow='hidden';const bar=document.createElement('div');const rawVal=resolve(n.props.value??60,state);const pct=typeof rawVal==='string'&&rawVal.endsWith('%')?rawVal:Math.max(0,Math.min(100,Number(rawVal)||0))+'%';bar.style.width=pct;bar.style.height='100%';bar.style.background=resolve(n.props.fill??'#8fa8ff',state);bar.style.borderRadius='inherit';el.append(bar);}for(const [k,v]of Object.entries(n.props)){const x=resolve(v,state);if(k==='key'){el.dataset.key=x;continue;}if(n.type==='Modal'&&(k==='open'||k==='dismiss'||k==='backdrop'))continue;if((n.type==='Badge'||n.type==='Tag'||n.type==='Chip')&&k==='tone')continue;if(n.type==='ProgressBar'&&(k==='value'||k==='fill'))continue;if(k==='wrap'){if(n.type!=='Text'||typeof x!=='boolean')throw Error('Text.wrap ожидает true или false');el.style.whiteSpace=x?'pre-wrap':'pre';continue;}if(k==='clip'){if(n.type!=='Frame'&&n.type!=='Row'&&n.type!=='Column'&&n.type!=='Grid'&&n.type!=='Stack'||typeof x!=='boolean')throw Error('Frame.clip ожидает true или false');if('overflow' in n.props)throw Error('Используйте clip или overflow, не оба');el.style.overflow=x?'clip':'visible';continue;}if(gridProperties.has(k)||k==='x'||k==='y')continue;else if(sizeProperties.has(k)){if(typeof x==='string'&&WEIGHT_STAR_RE.test(x)&&((parent?.type==='Row'&&k==='width')||((parent?.type==='Column'||parent?.type==='Frame')&&k==='height'))){el.style.flex=`${x==='*'?1:Number(x.slice(0,-1))} 1 0px`;el.style[k==='width'?'minWidth':'minHeight']='0';}else el.style[k]=sizeValue(x);}else if(k==='text')el.textContent=x??'';else if(k==='value')el.value=x??'';else if(k==='placeholder')el.placeholder=x;else if(k==='disabled')el.disabled=!!x;else if(k==='checked'||k==='selected')el.checked=!!x;else if(k==='style')el.classList.add(String(x));else if(CSS_MAP[k]){if(k==='borderWidth'&&!el.style.borderStyle)el.style.borderStyle='solid';el.style[CSS_MAP[k]]=typeof x==='number'&&k!=='opacity'?x+'px':Array.isArray(v)?v.map(a=>typeof a==='number'?a+'px':resolve(a,state)).join(' '):x;}else throw Error(`Свойство ${k} пока не поддерживается`);}
for(const [key,path]of Object.entries(n.bindings)){if(!['value','checked','selected'].includes(key))throw Error(`Неподдерживаемая привязка ${key} <-> ${path}`);const property=key==='value'?'value':'checked';el[property]=n.props[key]??(property==='value'?'':false);el.addEventListener('input',()=>{const value=property==='checked'?el.checked:n.type==='Slider'?Number(el.value):el.value;if(writePreviewBinding(state,n,path,value,{createMissing:true})){onLog(`${path} = ${JSON.stringify(value)}`);if(n.events.changed)onEvent(n.events.changed,n,'changed');onChange();}});}
if((n.type==='TextInput'||n.type==='TextField')&&n.events.submitted){el.addEventListener('keydown',e=>{if(interactive&&e.key==='Enter'){e.preventDefault();e.stopPropagation();onEvent(n.events.submitted,n,'submitted');}});}
el.addEventListener('click',e=>{e.stopPropagation();if(!interactive){onSelect(source);return;}if(n.events.clicked)onEvent(n.events.clicked,n);});Object.assign(el.style,gridStyles(n,state,parent));
if(takesCoordinates(n,parent))freeNodes.push([n,parent,parentEl,el]);
for(const child of n.children)el.append(make(child,n,el));
return el;}
const rootP=compiled.nodes?.[0]?.props??{};const vpW=typeof rootP.width==='number'?rootP.width:360,vpH=typeof rootP.height==='number'?rootP.height:220;const vp={width:vpW,height:vpH,compact:vpW<640};
const stateDefaults=Object.fromEntries(Object.entries(compiled.stateDefinitions??{}).filter(([,d])=>d.default!==undefined).map(([k,d])=>[k,d.default]));
const previewEnv={enums:{...compiled.enums,...compiled.structs},tokens:compiled.tokens,i18n:compiled.i18n,styles:compiled.styles,stateDefaults,viewport:vp,allowMissingState:true};
const focused=document.activeElement;const focusIdentity=preview.contains(focused)?{key:focused.dataset.key,start:focused.dataset.start,selectionStart:focused.selectionStart,selectionEnd:focused.selectionEnd}:null;const fragment=document.createDocumentFragment();const expanded=expandStructure(compiled.nodes,compiled.defaults,state,previewEnv);for(const n of expanded)fragment.append(make(n));preview.replaceChildren(fragment);
if(preview._formaShortcutHandler){preview.removeEventListener('keydown',preview._formaShortcutHandler);preview._formaShortcutHandler=null;}
if(interactive&&compiled.shortcuts?.length){
  preview.tabIndex=preview.tabIndex>=0?preview.tabIndex:0;
  preview._formaShortcutHandler=e=>{
    for(const s of compiled.shortcuts){
      if(matchPreviewShortcut(e,s.key)){
        e.preventDefault();
        const syntheticNode={type:'Shortcut',start:s.from??0,props:{},events:{shortcut:s.action},eventArgs:{shortcut:(s.args??[]).map(a=>resolve(a,state))},eventArgExpressions:{shortcut:s.args??[]},environment:previewEnv};
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
