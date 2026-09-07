// Component linking is shared by Studio preview and native launch. It emits the
// same concrete primitive template; there is no DOM rendering of button content.
import {parse} from './language.js';
import {propertyOrigins} from './property-origins.js';
import {similarName,sourceError} from './diagnostics.js';
import {svgShapes} from './svg-shapes.js';
import {attachSources,createElementTree} from './element-tree.js';
import {createCacheBudget} from './cache-budget.js';
import {evaluate,evaluateProperties,selectedProperties,selectedPropertySources,validateContract,validateTypeName,matchesType,expandStructure} from './component-semantics.js';
import {containerTypes,arrange,naturalContainer,intrinsicLength,length,constrained} from './component-layout.js';
export {evaluate} from './component-semantics.js';

const own=(o,k)=>Object.hasOwn(o,k);
const clone=v=>structuredClone(v);
const standard=new Set('key x y width height text fontSize font.size color radius disabled background hoverBackground pressedBackground disabledBackground borderWidth borderColor focusBorderColor transitionDuration'.split(' '));
const layoutProps=new Set('key cell row column row.span column.span width height minWidth maxWidth minHeight maxHeight'.split(' '));
const visualTypes=new Set([...containerTypes,'Text','TextInput','Image','Rectangle']);
const primitiveTypes=new Set([...visualTypes,'ContentPresenter','Brush','Border','Reveal','PointerArea','ContentText','ContentShape','ContentClip','ContentClipEnd']);
// Scene-level text controls use the same primitive rendering as component
// content. A project may replace these defaults with components/Text*.ui.
const builtinSources={
  Text:`component Text {
    width: 260; height: 30; text: ''; fontSize: 16; color: #ffffff;
    background: #00000000; borderWidth: 0; radius: 0;
    Rectangle { background: props.background;
      Text { text: props.text; color: props.color; fontSize: props.fontSize; }
    }
  }`,
  TextInput:`component TextInput {
    width: 240; height: 38; value: ''; placeholder: ''; multiline: false;
    text: props.value; fontSize: 15; color: #ffffff; placeholderColor: #999999;
    background: #00000000; borderWidth: 0; radius: 0;
    Rectangle { background: props.background;
      TextInput { forward props { value, placeholder, multiline, fontSize, color, placeholderColor }; }
      PointerArea { clicked -> events.clicked(); }
    }
  }`,
};
const contentProps=new Map(Object.entries({Frame:['columns','rows','gap','padding','clip','radius'],Row:['gap','padding','clip','radius'],Column:['gap','padding','clip','radius'],Grid:['columns','rows','gap','padding','clip','radius'],Stack:['gap','padding','clip','radius'],Rectangle:['background','radius'],TextInput:['value','placeholder','color','placeholderColor','fontSize','multiline'],Text:['text','color','fontSize','font.size'],Image:['source','color']}).map(([type,props])=>[type,new Set([...layoutProps,...props])]));
const templateEncoder=new TextEncoder();
const fallback={width:260,height:70,text:'',fontSize:20,color:{expr:'#14213b'},radius:16,disabled:false,background:{expr:'#8ca5ff'},hoverBackground:{expr:'#a8baff'},pressedBackground:{expr:'#6a83da'},disabledBackground:{expr:'#596273'},borderWidth:1,borderColor:{expr:'#bed0ff'},focusBorderColor:{expr:'#ffffff'},transitionDuration:{expr:'140ms'}};
function visit(nodes,fn){for(const n of nodes){fn(n);visit(n.children??[],fn);visit(n.elseChildren??[],fn);visit(n.emptyChildren??[],fn);}}
function substitute(v,key,base){
  if(v?.expr?.startsWith('base.')){if(v.expr!==`base.${key}`)throw Error(`В override ${key} допустим только base.${key}`);return clone(base);}
  if(Array.isArray(v))return v.map(x=>substitute(x,key,base));
  if(v&&typeof v==='object')return Object.fromEntries(Object.entries(v).map(([k,x])=>[k,substitute(x,key,base)]));
  return v;
}
function literal(v){
  if(v?.expr)return v.expr;
  if(v?.type)return `${v.type} { ${Object.entries(v.props).map(([k,x])=>`${k}: ${literal(x)};`).join(' ')} ${(v.children??[]).map(serialize).join(' ')} }`;
  if(Array.isArray(v))return v.map(literal).join(' ');
  if(typeof v==='string')return `'${v.replace(/\\/g,'\\\\').replace(/'/g,"\\'").replace(/\n/g,'\\n')}'`;
  if(typeof v==='number'&&!Number.isFinite(v))throw Error('Нечисловой размер');
  if(typeof v==='number'||typeof v==='boolean')return String(v);
  throw Error('Значение не удалось вычислить');
}
function serialize(n){return serializeNode(n,false);}
function serializeNode(n,omitKeys){if(Object.keys(n.bindings??{}).length||Object.keys(n.slots??{}).length)throw Error(`Необработанная привязка или override в ${n.type}`);return `${n.type} { ${Object.entries(n.props).filter(([k])=>!omitKeys||k!=='key').map(([k,v])=>`${k}: ${literal(v)};`).join(' ')} ${Object.entries(n.events??{}).map(([k,v])=>`${k} -> ${v}();`).join(' ')} ${(n.children??[]).map(child=>serializeNode(child,omitKeys)).join(' ')} }`;}
const num=(v,label)=>{if(typeof v==='object'&&v?.expr?.endsWith('px'))v=Number(v.expr.slice(0,-2));if(typeof v!=='number'||!Number.isFinite(v)||v<0||v>4096)throw Error(`${label}: ожидается размер 0…4096`);return v;};
const color=v=>v?.expr??v;
function insets(v=0){const a=Array.isArray(v)?v:[v];if(a.length<1||a.length>4)throw Error('padding: от 1 до 4 размеров');const x=a.map(v=>num(v,'padding'));return x.length===1?[x[0],x[0],x[0],x[0]]:x.length===2?[x[0],x[1],x[0],x[1]]:x.length===3?[x[0],x[1],x[2],x[1]]:x;}
function tracks(v){return v===undefined?[{expr:'*'}]:Array.isArray(v)?v:[v];}
function cell(n,axis,count){const c=n.props.cell;if(c&&(!Array.isArray(c)||c.length!==2||own(n.props,'row')||own(n.props,'column')))throw Error('cell: ожидаются row column без отдельных row/column');const v=c?.[axis==='rows'?0:1]??n.props[axis==='rows'?'row':'column'];if(v===undefined)return null;if(!Number.isInteger(v)||v<1||v>count)throw Error(`Ячейка за пределами ${axis}`);return v-1;}
function gap(p){const a=Array.isArray(p.gap)?p.gap:[p.gap??0,p.gap??0];if(a.length!==2)throw Error('gap: один или два размера');return a.map(v=>num(v,'gap'));}
function natural(n,axis,metrics){
  const key=axis===0?'width':'height',p=n.props;
  if(p[key]!==undefined&&!intrinsicLength(p[key])&&!/[*%]$/.test(p[key]?.expr??''))return constrained(num(p[key],key),p,axis,4096);
  if(containerTypes.has(n.type)&&n.type!=='Frame')return naturalContainer(n,axis,(child,dimension)=>natural(child,dimension,metrics));
  if(n.type==='Text'){const fs=num(p.fontSize??p['font.size']??16,'fontSize');if(!metrics?.measureText)throw Error('Размер текста по контенту требует метрик Rust runtime');const lines=String(p.text??'').split('\n');const size=lines.length===1?metrics.measureText(lines[0],fs)[axis]:axis===1?lines.length*fs*1.5:Math.max(...lines.map(line=>metrics.measureText(line,fs)[0]));if(!Number.isFinite(size)||size<0)throw Error('Некорректные метрики текста');return size;}
  if(n.type==='TextInput')return axis===0?80:Number(p.fontSize??15)*1.5;
  if(n.type==='Image')return 20;
  if(n.type==='Rectangle')return Math.max(0,...n.children.filter(c=>visualTypes.has(c.type)).map(c=>natural(c,axis,metrics)));
  if(n.type==='Frame'){
    const ts=tracks(p[axis===0?'columns':'rows']),g=gap(p)[axis===0?1:0],pad=insets(p.padding);
    return ts.reduce((s,t,i)=>s+(typeof t==='number'?num(t,key):Math.max(0,...n.children.filter(c=>cell(c,axis===0?'columns':'rows',ts.length)===i||ts.length===1).map(c=>natural(c,axis,metrics)))),0)+g*(ts.length-1)+pad[axis===0?1:0]+pad[axis===0?3:2];
  }throw Error(`Контент ${n.type} пока не поддерживается`);
}
function sizes(n,axis,available,metrics){
  const name=axis===0?'columns':'rows',ts=tracks(n.props[name]),g=gap(n.props)[axis===0?1:0];let weight=0;
  const result=ts.map((t,i)=>{const x=t?.expr??t;if(typeof x==='number')return num(x,name);if(x==='-'||x==='content'||x==='auto')return Math.max(0,...n.children.filter(c=>cell(c,name,ts.length)===i||ts.length===1).map(c=>natural(c,axis,metrics)));if(typeof x==='string'&&/^(?:\d+(?:\.\d+)?)?\*$/.test(x)){const w=x==='*'?1:Number(x.slice(0,-1));if(w<=0)throw Error('Вес * должен быть положительным');weight+=w;return {w};}if(typeof x==='string'&&/^\d+(?:\.\d+)?%$/.test(x))return available*Number(x.slice(0,-1))/100;throw Error(`Неподдерживаемый размер ${x}`);});
  const remaining=Math.max(0,available-g*(ts.length-1)-result.reduce((s,v)=>s+(typeof v==='number'?v:0),0));return result.map(v=>typeof v==='number'?v:remaining*v.w/weight);
}
function flatten(n,box,files,output,metrics){
  // Keys belong to the inspector snapshot. Lowering must not mutate that tree.
  const p=own(n.props,'key')?Object.fromEntries(Object.entries(n.props).filter(([k])=>k!=='key')):n.props,allowed=contentProps.get(n.type)??layoutProps;
  for(const k of Object.keys(p))if(!allowed.has(k))throw Error(`${n.type}.${k} пока не поддерживается в контенте`);
  if(Object.keys(n.events??{}).length||Object.keys(n.bindings??{}).length)throw Error('Контент кнопки пока не содержит отдельных интерактивных элементов');
  let [x,y,w,h]=box;const nw=constrained(p.width===undefined?w:length(p.width,w,intrinsicLength(p.width)?natural({...n,props:{...p,width:undefined}},0,metrics):0,'width'),p,0,w),nh=constrained(p.height===undefined?h:length(p.height,h,intrinsicLength(p.height)?natural({...n,props:{...p,height:undefined}},1,metrics):0,'height'),p,1,h);x+=(w-nw)/2;y+=(h-nh)/2;w=nw;h=nh;
  const visual=metrics.visuals?{id:metrics.visuals.length,parent:metrics.parentVisual??null,type:n.type,source:n.source,propertySources:n.propertySources,propertyOrigins:n.propertyOrigins,props:p,bounds:[x,y,w,h]}:null;
  if(visual)metrics.visuals.push(visual);
  const childMetrics=visual?{...metrics,parentVisual:visual.id}:metrics;
  if(n.type==='Rectangle'){
    const radius=num(p.radius??0,'radius');
    if(p.background!==undefined&&!(typeof color(p.background)==='string'&&/^#(?:[\da-f]{3}|[\da-f]{6}|[\da-f]{8})$/i.test(color(p.background))))throw Error('Rectangle.background в контенте принимает только hex; Brush и его состояния пока не поддерживаются');
    // A native clip rounds an ordinary polygon without a new Rust primitive.
    // Children share this bounded surface; use a Frame child for grid layout.
    output.push(`ContentClip { x: ${x}; y: ${y}; width: ${w}; height: ${h}; radius: ${radius}; }`);
    if(p.background!==undefined&&(metrics.preserveEmptyGeometry||(w>0&&h>0)))output.push(`ContentShape { points: '${x} ${y} ${x+w} ${y} ${x+w} ${y+h} ${x} ${y+h}'; color: ${color(p.background)}; }`);
    for(const child of n.children)flatten(child,[x,y,w,h],files,output,childMetrics);
    output.push('ContentClipEnd {}');return;
  }
  if(containerTypes.has(n.type)&&n.type!=='Frame'){
    if(p.clip!==undefined&&typeof p.clip!=='boolean')throw Error(`${n.type}.clip ожидает boolean`);
    if(p.clip)output.push(`ContentClip { x: ${x}; y: ${y}; width: ${w}; height: ${h}; radius: ${num(p.radius??0,'radius')}; }`);
    const layout=arrange(n,[x,y,w,h],(child,axis)=>natural(child,axis,metrics));
    if(visual&&layout.grid)visual.grid=layout.grid;
    n.children.forEach((child,index)=>flatten({...child,props:{...child.props,width:layout.boxes[index][2],height:layout.boxes[index][3]}},layout.boxes[index],files,output,childMetrics));
    if(p.clip)output.push('ContentClipEnd {}');return;
  }
  if(n.type==='Frame'){
    if(p.clip!==undefined&&typeof p.clip!=='boolean')throw Error('Frame.clip ожидает boolean');
    const radius=num(p.radius??0,'radius');
    if(p.clip)output.push(`ContentClip { x: ${x}; y: ${y}; width: ${w}; height: ${h}; radius: ${radius}; }`);
    const pad=insets(p.padding);x+=pad[3];y+=pad[0];w=Math.max(0,w-pad[1]-pad[3]);h=Math.max(0,h-pad[0]-pad[2]);
    const cols=sizes(n,0,w,metrics),rows=sizes(n,1,h,metrics),g=gap(p);
    if(visual)visual.grid={bounds:[x,y,w,h],columns:cols,rows,gap:g};
    for(const c of n.children){const ci=cell(c,'columns',cols.length),ri=cell(c,'rows',rows.length);flatten(c,[ci===null?x:x+cols.slice(0,ci).reduce((a,b)=>a+b,0)+ci*g[1],ri===null?y:y+rows.slice(0,ri).reduce((a,b)=>a+b,0)+ri*g[0],ci===null?w:cols[ci],ri===null?h:rows[ri]],files,output,childMetrics);}if(p.clip)output.push('ContentClipEnd {}');return;
  }
  if(n.children?.length)throw Error(`${n.type} не принимает дочерние элементы`);
  if(n.type==='TextInput'){
    output.push(`ContentInput { x: ${x}; y: ${y}; width: ${w}; height: ${h}; value: ${literal(String(p.value??''))}; placeholder: ${literal(String(p.placeholder??''))}; color: ${literal(p.color??{expr:'#ffffff'})}; placeholderColor: ${literal(p.placeholderColor??{expr:'#999999'})}; fontSize: ${num(p.fontSize??15,'fontSize')}; multiline: ${Boolean(p.multiline)}; }`);return;
  }
  if(n.type==='Text'){
    const text=String(p.text??''),fontSize=num(p.fontSize??p['font.size']??16,'fontSize'),lines=text.split('\n');
    if(lines.length===1)output.push(`ContentText { x: ${x}; y: ${y}; width: ${w}; height: ${h}; text: ${literal(text)}; color: ${literal(p.color??{expr:'#ffffff'})}; fontSize: ${fontSize}; }`);
    else {
      if(!metrics?.measureText)throw Error('Многострочный текст требует метрик Rust runtime');
      output.push(`ContentClip { x: ${x}; y: ${y}; width: ${w}; height: ${h}; radius: 0; }`);
      lines.forEach((line,i)=>output.push(`ContentText { x: ${x}; y: ${y+i*fontSize*1.5}; width: ${metrics.measureText(line,fontSize)[0]}; height: ${fontSize*1.5}; text: ${literal(line)}; color: ${literal(p.color??{expr:'#ffffff'})}; fontSize: ${fontSize}; }`));
      output.push('ContentClipEnd {}');
    }
    return;
  }
  if(n.type==='Image'){
    if(typeof p.source!=='string'||!own(files,p.source))throw Error(`SVG-файл не найден в проекте: ${p.source}`);
    const content=metrics.imageContent(files[p.source],[x,y,w,h],color(p.color??'#ffffff'));
    if(content)output.push(content);return;
  }
  throw Error(`Неподдерживаемый контент ${n.type}`);
}

const readDocument=(source,path,fragment=false)=>attachSources(parse(source,{fragment}),path);

// A session retains parsed documents and definition-only linked templates under
// one budget. State, callbacks, expansion and compiled output stay per-compile.
export function createComponentCompiler({cache:storage=createCacheBudget()}={}){
  let linkedHits=0,linkedMisses=0;
  function read(source,path,fragment=false){
    const key=(fragment?'compiler:fragment:':'compiler:document:')+path,previous=storage.get(key);
    const document=previous?.source===source?previous.document:readDocument(source,path,fragment);
    if(previous?.document!==document)storage.set(key,{source,document});
    return document;
  }
  const links={
    get(name,files){
      const saved=storage.get('compiler:linked:'+name);
      let valid=!!saved;
      if(saved)for(const [path,source]of saved.dependencies)if(files[path]!==source){valid=false;break;}
      if(valid){
        linkedHits++;return {...saved,linked:clone(saved.linked)};
      }
      if(saved)storage.delete('compiler:linked:'+name);
      linkedMisses++;return null;
    },
    set(name,linked,dependencies,depth){storage.set('compiler:linked:'+name,{linked:clone(linked),dependencies,depth});},
  };
  const run=(files,entry,state={},metrics={})=>compile(files,entry,state,metrics,read,links);
  run.cacheStats=()=>({...storage.snapshot(),linkedHits,linkedMisses});
  return run;
}

export function compileComponents(files,entry,state={},metrics={}){return compile(files,entry,state,metrics,readDocument);}
// Definition linking is also consumed by Rust code generation. It deliberately
// runs before evaluating props/state so generated forms retain dependencies.
export function linkComponentDefinitions(files){
  const entry='__forma_definitions.ui';
  return compile({...files,[entry]:'component Definitions { Frame {} }'},entry,{}, {definitionsOnly:true},readDocument);
}
function compile(files,entry,state,metrics,read,links){
  // Measurement and lowered SVG geometry are deterministic within one compile.
  // Keep these caches local so edits, state, fonts and metrics callbacks are
  // re-read on the next compilation, without retaining large output strings.
  const textSizes=new Map(),images=new Map(),measureText=metrics.measureText,metricsContext=metrics;
  let imageUnits=0;
  metrics={...metrics,
    measureText:measureText?function(text,size){
      let byText=textSizes.get(size);if(!byText){byText=new Map();textSizes.set(size,byText);}
      if(!byText.has(text)){const measured=measureText.call(metricsContext,text,size);byText.set(text,[measured[0],measured[1]]);}
      return byText.get(text);
    }:undefined,
    imageContent(source,box,currentColor){
      let byBox=images.get(source);const key=JSON.stringify([...box,currentColor]);
      if(byBox?.has(key))return byBox.get(key);
      const content=svgShapes(source,box,currentColor).map(s=>`ContentShape { points: ${literal(s.points.map(v=>v.join(' ')).join(' '))}; color: ${s.color}; }`).join(' ');
      if(imageUnits+content.length<=1_000_000){if(!byBox){byBox=new Map();images.set(source,byBox);}byBox.set(key,content);imageUnits+=content.length;}
      return content;
    },
  };
  const document=read(files[entry],entry);let scene=clone(document.nodes);metricsContext.transformScene?.(scene);
  visit(scene,n=>{const patch=metricsContext.instanceProps?.(n);if(patch)n.props={...n.props,...patch};});
  const cache=new Map(),loading=[],dependencies=new Map(),depths=new Map();
  function link(name){
    if(cache.has(name)){
      if(loading.length+depths.get(name)>32)throw Error('Глубина наследования компонентов превышает 32');
      return cache.get(name);
    }
    if(loading.includes(name))throw Error(`Цикл наследования: ${[...loading,name].join(' → ')}`);
    if(loading.length>=32)throw Error('Глубина наследования компонентов превышает 32');
    const saved=links?.get(name,files);
    if(saved){
      if(loading.length+saved.depth>32)throw Error('Глубина наследования компонентов превышает 32');
      cache.set(name,saved.linked);dependencies.set(name,saved.dependencies);depths.set(name,saved.depth);return saved.linked;
    }
    const path=`components/${name}.ui`,source=own(files,path)?files[path]:builtinSources[name];if(source===undefined)throw Error(`Компонент не найден: ${path}`);
    const inputs=links?new Map([[path,files[path]]]):null;
    const c=read(source,path);if(c.name!==name)throw Error(`${path}: ожидался component ${name}`);if(c.designs.length)throw Error('Дизайн-атрибут компонента пока не поддерживается векторным runtime');loading.push(name);
    const linked=c.base?clone(link(c.base)):{nodes:[],defaults:{}};
    if(links&&c.base)for(const [file,source]of dependencies.get(c.base))inputs.set(file,source);
    if(c.base&&c.nodes.length)throw Error('Наследник меняет визуальное дерево через override');
    if(!c.base)linked.nodes=clone(c.nodes);
    linked.defaults={...linked.defaults,...clone(c.defaults)};
    linked.defaultSources={...linked.defaultSources,...Object.fromEntries(Object.entries(c.defaultRanges??{}).map(([key,range])=>[key,{file:path,...range,label:c.name}]))};
    linked.propDefinitions={...linked.propDefinitions,...clone(c.propDefinitions??{})};
    linked.eventDefinitions={...linked.eventDefinitions,...clone(c.eventDefinitions??{})};
    linked.enums={...linked.enums,...clone(c.enums??{})};
    linked.matches=[...(linked.matches??[]),...clone(c.matches??[])];
    for(const definition of Object.values(linked.propDefinitions))validateTypeName(definition.type,linked.enums);
    for(const args of Object.values(linked.eventDefinitions))for(const arg of args)validateTypeName(arg.type,linked.enums);
    if(c.forward?.length)throw Error('forward применяется к дочернему элементу, не к объявлению component');
    const presenters=new Map();visit(linked.nodes,n=>{if(n.type==='ContentPresenter'){const key=n.props.key;if(typeof key!=='string'||!key)throw Error('ContentPresenter требует key');if(presenters.has(key))throw Error(`Повторная точка расширения ${key}`);presenters.set(key,n);}});
    const patchProperties=(target,patch,key)=>{
      if(patch.base||patch.children.length||patch.matches?.length||patch.forward?.length||Object.keys(patch.slots).length||Object.keys(patch.events).length||Object.keys(patch.bindings).length)throw Error('Блок override переопределяет только свойства');
      if(own(patch.props,'key'))throw Error('override не меняет key целевого элемента');
      const allowed=contentProps.get(target.type);
      if(allowed&&target.type!=='Rectangle')for(const name of Object.keys(patch.props))if(!allowed.has(name)){
        const suggestion=similarName(name,allowed);
        throw sourceError(`override ${key}: узел ${target.type} не поддерживает свойство ${name}.${suggestion?` Возможно, вы имели в виду ${suggestion}.`:''}`,patch.propertySources?.[name]??patch.source,target.source);
      }
      Object.assign(target.props,clone(patch.props));
      target.propertySources={...target.propertySources,...Object.fromEntries(Object.entries(patch.propertySources??{}).map(([name,origin])=>[name,{...origin,label:`${c.name} · override ${key}`}]))};
    };
    for(const [key,value] of Object.entries(c.slots)){
      if(value?.propertyOverride){
        if(!c.base)throw Error('override требует базового компонента');
        const targets=[],keys=[];
        const find=value=>{if(!value||typeof value!=='object')return;if(value.type&&typeof value.props?.key==='string'){keys.push(value.props.key);if(value.props.key===key)targets.push(value);}for(const child of Object.values(value))find(child);};
        find(linked.nodes);
        if(targets.length!==1){const suggestion=similarName(key,keys);throw sourceError(`override ${key}: ожидался один элемент, найдено ${targets.length}.${!targets.length&&suggestion?` Возможно, вы имели в виду ${suggestion}.`:''}`,value.patch.source);}
        const target=targets[0];
        if(target.type==='ContentPresenter'){
          const content=target.props.content??(target.children.length===1?target.children[0]:{type:'Frame',props:{},children:target.children});
          const patchContent=content=>{
            if(content?.type&&content.props){patchProperties(content,value.patch,key);return;}
            if(content?.match!==undefined){for(const branch of content.branches)patchContent(branch.value);return;}
            throw Error(`override ${key}: для изменения свойств содержимое должно быть элементом`);
          };
          patchContent(content);target.props.content=content;target.children=[];
        }else patchProperties(target,value.patch,key);
        continue;
      }
      if(value?.fileOverride){
        if(!c.base)throw Error('override требует базового компонента');
        const targets=[];visit(linked.nodes,n=>{if(n.props.key===key)targets.push(n);});
        if(targets.length!==1)throw Error(`override ${key}: ожидался один элемент, найдено ${targets.length}`);
        const target=targets[0],parts=path.split('/').slice(0,-1);
        if(value.file.startsWith('/')||value.file.includes('\\'))throw Error('override from: требуется относительный путь проекта');
        for(const part of value.file.split('/')){
          if(!part||part==='.')continue;
          if(part==='..'){if(!parts.length)throw Error('override from: выход за пределы проекта');parts.pop();}else parts.push(part);
        }
        const file=parts.join('/');if(!own(files,file))throw Error(`Файл override не найден: ${file}`);
        inputs?.set(file,files[file]);
        const patch=read(files[file],file,true).nodes[0];
        if(patch.type!==target.type)throw Error(`override ${key}: ожидался ${target.type}, получен ${patch.type} в ${file}`);
        for(const p of [patch,value.patch].filter(Boolean)){
          patchProperties(target,p,key);
        }
        continue;
      }
      const target=presenters.get(key);if(!target)throw Error(`Нет точки расширения ${key} в ${name}`);
      if(!c.base)throw Error('override требует базового компонента');
      const base=target.props.content??(target.children.length===1?target.children[0]:{type:'Frame',props:{},children:target.children});
      target.props.content=substitute(value,key,base);target.children=[];
    }
    const depth=1+(c.base?depths.get(c.base):0);
    loading.pop();cache.set(name,linked);if(links)dependencies.set(name,inputs);depths.set(name,depth);
    links?.set(name,linked,inputs,depth);return linked;
  }
  if(metricsContext.definitionsOnly){
    for(const name of Object.keys(builtinSources))link(name);
    for(const path of Object.keys(files)){const match=/^components\/([A-Za-z_][\w]*)\.ui$/.exec(path);if(match)link(match[1]);}
    return {definitions:Object.fromEntries(cache),fallback};
  }
  const enums={...document.enums};
  visit(scene,node=>{if(!primitiveTypes.has(node.type)&&!['Scroll','If','For'].includes(node.type)){
    const definition=link(node.type);
    for(const [name,variants]of Object.entries(definition.enums??{})){
      if(own(enums,name)&&JSON.stringify(enums[name])!==JSON.stringify(variants))throw Error(`Конфликт enum ${name}`);
      enums[name]=variants;
    }
  }});
  const environment={enums};
  const documentProps={...document.defaults,...metricsContext.properties};
  for(const [key,definition]of Object.entries(document.propDefinitions??{}))if(definition.required&&!own(documentProps,key))throw Error(`${document.name}: обязательное свойство ${key} не задано`);
  Object.assign(documentProps,selectedProperties(document.matches,documentProps,state,environment),metricsContext.properties);
  validateContract(document.propDefinitions,Object.fromEntries(Object.entries(documentProps).map(([key,value])=>[key,evaluate(value,documentProps,state,[key],false,environment)])),enums,document.name);
  if(document.forward?.length)throw Error('forward применяется к дочернему элементу, не к объявлению component');
  const documentSources={...Object.fromEntries(Object.entries(document.defaultRanges??{}).map(([key,range])=>[key,{file:entry,...range,label:document.name}])),...selectedPropertySources(document.matches,documentProps,state,environment)};
  scene=expandStructure(scene,documentProps,state,environment,{trackOrigins:true,propertySources:documentSources,recursive:node=>containerTypes.has(node.type)||node.type==='Scroll'});
  const instanceTree=createElementTree(scene);
  visit(scene,n=>{if(Object.keys(n.slots??{}).length)throw Error('override объявляется в наследнике component');});
  if(scene.length!==1||!containerTypes.has(scene[0].type))throw Error('Ожидается один корневой Frame, Row, Column, Grid или Stack');
  let instances=[];
  let expandedCount=0;
  function rejectInteraction(n){
    if(n.type==='PointerArea'||Object.keys(n.events??{}).length||Object.keys(n.bindings??{}).length)throw Error(`Вложенный визуальный компонент не содержит интерактивных элементов: ${n.type}`);
  }
  function instantiate(instance,top,parents=[]){
    if(instance.normalizedRange&&typeof instance.props.value==='number')instance={...instance,props:{...instance.props,value:{expr:`${instance.props.value*100}%`}}};
    if(parents.includes(instance.type))throw Error(`Цикл композиции: ${[...parents,instance.type].join(' → ')}`);
    if(parents.length>=32)throw Error('Глубина композиции компонентов превышает 32');
    if(!top)rejectInteraction(instance);
    const linked=link(instance.type);
    let hasContent=false;visit(linked.nodes,n=>{if(n.type==='ContentPresenter'&&n.props.key==='content')hasContent=true;});
    if(instance.children.length&&!hasContent)throw Error('Для дочернего контента нужен ContentPresenter с key: content');
    const groupedNames=new Set((linked.matches??[]).flatMap(group=>group.branches.flatMap(branch=>Object.keys(branch.props))));
    for(const k of Object.keys(instance.props))if(!(top?standard:layoutProps).has(k)&&!layoutProps.has(k)&&!own(linked.defaults,k)&&!own(linked.propDefinitions??{},k)&&!groupedNames.has(k)&&!(k==='font.size'&&own(linked.defaults,'fontSize')))throw Error(`Неизвестное свойство ${instance.type}.${k}`);
    if(instance.props['font.size']!==undefined&&instance.props.fontSize!==undefined)throw Error('fontSize и font.size — одно свойство');
    // A visual instance never inherits its caller's props or button fallbacks.
    // Arguments and supplied children have already been resolved in the caller.
    const props={...(top?fallback:{}),...linked.defaults,...instance.props};
    const env={...instance.environment,enums:{...enums,...linked.enums},locals:instance.environment?.locals};
    Object.assign(props,selectedProperties(linked.matches,props,state,env),instance.props);
    for(const [key,definition]of Object.entries(linked.propDefinitions??{}))if(definition.required&&!own(linked.defaults,key)&&!own(instance.props,key))throw Error(`${instance.type}: обязательное свойство ${key} не задано`);
    if(instance.props['font.size']!==undefined)props.fontSize=instance.props['font.size'];
    const sources={...linked.defaultSources,...selectedPropertySources(linked.matches,props,state,env),...instance.propertySources};
    const scope={props,sources,traces:instance.propertyOrigins??{},instance,top,environment:env,parents:[...parents,instance.type]};
    const resolved=Object.fromEntries(Object.entries(props).map(([k,v])=>[k,expandValue(v,scope,0,[k])]));
    validateContract(linked.propDefinitions,resolved,env.enums,instance.type);
    const roots=expandChildren(linked.nodes,scope,0);
    if(roots.length!==1||(top?roots[0].type!=='Rectangle':!visualTypes.has(roots[0].type)))throw Error(top?'Базовая кнопка требует один Rectangle':'Визуальный компонент требует один Frame, Text, Image или Rectangle');
    if(!top){
      visit(roots,n=>{rejectInteraction(n);if(!visualTypes.has(n.type))throw Error(`Неподдерживаемый визуальный примитив ${n.type}`);});
      // Keep defining-file origins, and use the caller's origin for layout edits.
      const root=roots[0];
      if(own(instance.props,'cell')&&(own(instance.props,'row')||own(instance.props,'column')))throw Error('cell: ожидаются row column без отдельных row/column');
      if(['cell','row','column'].some(key=>own(instance.props,key))){root.propertySources={...root.propertySources};for(const key of ['cell','row','column']){delete root.props[key];delete root.propertySources[key];}}
      for(const key of layoutProps)if(own(instance.props,key)){
        root.props[key]=resolved[key];
        if(instance.propertySources?.[key])root.propertySources={...root.propertySources,[key]:clone(instance.propertySources[key])};
      }
    }
    const origins=Object.fromEntries(Object.entries(props).map(([key,value])=>[key,instance.propertyOrigins?.[key]??propertyOrigins(value,sources[key],props,sources,{},new Set([key]))]));
    return {roots,resolved,origins};
  }
  function expandValue(value,scope,depth,stack=[]){
    const result=evaluate(value,scope.props,state,stack,false,scope.environment);
    return result?.type?expand(result,scope,depth+1):result;
  }
  function expandChildren(nodes,scope,depth){
    return expandStructure(nodes,scope.props,state,scope.environment,{recursive:false,evaluateProps:false}).map(node=>expand(node,{...scope,environment:node.environment},depth+1));
  }
  function expand(n,scope,depth){
    if(Object.keys(n.bindings??{}).length)throw Error('Вложенные двусторонние привязки пока не поддерживаются; задайте привязку на экземпляре контрола');
    if(depth>64||++expandedCount>16384)throw Error('Превышен предел глубины или размера визуальной композиции');
    if(n.expandedVisual)return clone(n);
    if(Object.keys(n.slots??{}).length)throw Error('override объявляется в наследнике component');
    if(!scope.top)rejectInteraction(n);
    if(n.type==='ContentPresenter'){
      if(Object.keys(n.props).some(k=>!['key','content'].includes(k)))throw Error('ContentPresenter принимает key и content');
      let content=n.props.content??(n.children.length===1?n.children[0]:{type:'Frame',props:{},children:n.children});
      if(scope.instance.children.length&&n.props.key==='content'){
        if(own(scope.instance.props,'content'))throw Error('Задайте content или дочерние элементы');
        content={type:'Frame',props:{},children:scope.instance.children};
      }
      const result=expandValue(content,scope,depth);
      if(!result?.type)throw Error('override должен вернуть элемент или base.content');
      return result;
    }
    const sources={...selectedPropertySources(n.matches,scope.props,state,scope.environment),...n.propertySources};
    const raw={...Object.fromEntries((n.forward??[]).map(key=>[key,{expr:`props.${key}`}])) ,...selectedProperties(n.matches,scope.props,state,scope.environment),...n.props};
    const origins=Object.fromEntries(Object.entries(raw).map(([key,value])=>[key,propertyOrigins(value,sources[key],scope.props,scope.sources,scope.traces)]));
    const node={...n,props:Object.fromEntries(Object.entries(evaluateProperties(n,scope.props,state,scope.environment)).map(([k,v])=>[k,v?.type?expand(v,scope,depth+1):v])),propertySources:sources,propertyOrigins:origins,children:expandChildren(n.children??[],scope,depth),forward:[],matches:[]};
    if(!primitiveTypes.has(n.type))return instantiate(node,false,scope.parents).roots[0];
    return {...node,expandedVisual:true};
  }
  const visualNodes=[];
  const root=scene[0],rootProps=root.props,children=root.children;
  const scroll=children.length===1&&children[0].type==='Scroll'?children[0]:null;
  instances=scroll?scroll.children:children;
  const rootWidth=constrained(length(rootProps.width??360,360,360,'width'),rootProps,0,360),rootHeight=constrained(length(rootProps.height??220,220,220,'height'),rootProps,1,220);
  const rootVisual={id:-1,parent:null,control:-1,type:root.type,source:root.source,propertySources:root.propertySources,props:clone(rootProps),bounds:[0,0,rootWidth,rootHeight]};
  let modern=root.type!=='Frame';
  for(const n of instances)if(containerTypes.has(n.type)||['minWidth','maxWidth','minHeight','maxHeight'].some(key=>own(n.props,key))||['width','height'].some(key=>own(n.props,key)&&intrinsicLength(n.props[key])))modern=true;
  const previewNodes=scene;
  function intrinsicScene(node,axis){
    const key=axis===0?'width':'height',value=node.props[key];
    if(value!==undefined&&!intrinsicLength(value)&&!/[\*%]$/.test(value?.expr??''))return constrained(length(value,axis===0?rootWidth:rootHeight,0),node.props,axis,axis===0?rootWidth:rootHeight);
    if(containerTypes.has(node.type))return naturalContainer({...node,layoutKind:node.type==='Frame'&&node.props.columns===undefined&&node.props.rows===undefined?'Column':node.type},axis,intrinsicScene);
    const {roots,resolved}=instantiate(node,true);
    if(intrinsicLength(resolved[key]))return constrained(natural(roots[0],axis,metrics),node.props,axis,axis===0?rootWidth:rootHeight);
    return constrained(length(resolved[key],axis===0?rootWidth:rootHeight,0),node.props,axis,axis===0?rootWidth:rootHeight);
  }
  function layoutScene(node,box,output){
    if(!containerTypes.has(node.type)){
      output.push({...node,props:{...node.props,x:box[0],y:box[1],width:box[2],height:box[3]}});return;
    }
    if(node!==root&&node.props.clip)throw Error('Обрезка вложенного контейнера сцены пока не поддерживается; используйте clip корневого Frame');
    let layout;
    if(node.type==='Frame'&&(node.props.columns!==undefined||node.props.rows!==undefined)){
      const pad=insets(node.props.padding),g=gap(node.props),inside=[box[0]+pad[3],box[1]+pad[0],Math.max(0,box[2]-pad[1]-pad[3]),Math.max(0,box[3]-pad[0]-pad[2])];
      const measured={...node,children:node.children.map(child=>({...child,props:{...child.props,width:intrinsicScene(child,0),height:intrinsicScene(child,1)}}))};
      const cols=sizes(measured,0,inside[2],metrics),rows=sizes(measured,1,inside[3],metrics);
      layout={grid:{bounds:inside,columns:cols,rows,gap:g},boxes:node.children.map(child=>{const ci=cell(child,'columns',cols.length),ri=cell(child,'rows',rows.length);return [inside[0]+(ci===null?0:cols.slice(0,ci).reduce((a,b)=>a+b,0)+ci*g[1]),inside[1]+(ri===null?0:rows.slice(0,ri).reduce((a,b)=>a+b,0)+ri*g[0]),ci===null?inside[2]:cols[ci],ri===null?inside[3]:rows[ri]];})};
    }else layout=arrange(node,box,intrinsicScene,{scene:true});
    if(node===root&&layout.grid)rootVisual.grid=layout.grid;
    node.children.forEach((child,index)=>layoutScene(child,layout.boxes[index],output));
  }
  if(modern){
    const output=[];
    layoutScene(scroll?{...scroll,type:'Column'}:root,[0,0,rootWidth,rootHeight],output);
    instances=output;
    rootProps.width=rootWidth;rootProps.height=rootHeight;rootProps.padding=0;rootProps.gap=0;
    for(const key of ['columns','rows','minWidth','maxWidth','minHeight','maxHeight'])delete rootProps[key];
  }else if(rootProps.columns!==undefined||rootProps.rows!==undefined){
    const pad=insets(rootProps.padding),g=gap(rootProps);
    const resolvedChildren=instances.map(n=>{const control=clone(n);for(const key of ['cell','row','column'])delete control.props[key];const {resolved}=instantiate(control,true);return {...n,props:{...n.props,width:resolved.width,height:resolved.height}};});
    const layout={...root,children:resolvedChildren};
    const cols=sizes(layout,0,Math.max(0,rootWidth-pad[1]-pad[3]),metrics),rows=sizes(layout,1,Math.max(0,rootHeight-pad[0]-pad[2]),metrics);
    rootVisual.grid={bounds:[pad[3],pad[0],rootWidth-pad[1]-pad[3],rootHeight-pad[0]-pad[2]],columns:cols,rows,gap:g};
    instances.forEach(n=>{
      const ci=cell(n,'columns',cols.length),ri=cell(n,'rows',rows.length);
      n.props.x=pad[3]+(ci===null?0:cols.slice(0,ci).reduce((a,b)=>a+b,0)+ci*g[1]);
      n.props.y=pad[0]+(ri===null?0:rows.slice(0,ri).reduce((a,b)=>a+b,0)+ri*g[0]);
      if(n.props.width===undefined)n.props.width=ci===null?rootVisual.grid.bounds[2]:cols[ci];
      if(n.props.height===undefined)n.props.height=ri===null?rootVisual.grid.bounds[3]:rows[ri];
      for(const key of ['cell','row','column'])delete n.props[key];
    });
    delete rootProps.columns;delete rootProps.rows;rootProps.gap=0;
  }
  if(instances.length>256||instances.some(n=>['Frame','Scroll'].includes(n.type)))throw Error('Frame принимает 0…256 контролов либо один Scroll с контролами');
  visualNodes.push(rootVisual);
  function compileInstance(instance,index){
  for(const [event,action]of Object.entries(instance.events??{})){
    if(!action.startsWith('events.'))continue;
    const name=action.slice(7),signature=document.eventDefinitions?.[name],args=instance.eventArgs?.[event]??[];
    if(!signature){
      if(Object.keys(document.eventDefinitions??{}).length||Array.isArray(document.defaults.events)){
        if(!document.defaults.events?.includes(name))throw Error(`Событие ${name} не объявлено`);
      }
      if(args.length)throw Error(`Для аргументов events.${name} требуется объявление event`);
    }else{
      if(signature.length!==args.length)throw Error(`events.${name}: ожидалось ${signature.length} аргументов, получено ${args.length}`);
      signature.forEach((arg,i)=>{validateTypeName(arg.type,enums);if(!matchesType(args[i],arg.type,enums))throw Error(`events.${name}.${arg.name}: ожидался ${arg.type}`);});
    }
  }
  const {roots,resolved,origins}=instantiate(instance,true);
  let rangeMetadata='';
  if(instance.type==='Slider'){
    const endpoint=value=>{
      const result=instantiate({...instance,props:{...instance.props,value:{expr:value+'%'}}},true);
      const content=[];
      for(const child of result.roots[0].children)if(visualTypes.has(child.type))flatten(child,[0,0,num(resolved.width,'width'),num(resolved.height,'height')],files,content,{...metrics,preserveEmptyGeometry:true});
      return content.join(' ');
    };
    rangeMetadata=`RangeInput { value: ${parseFloat(resolved.value?.expr??resolved.value??50)/100}; Minimum { ${endpoint(0)} } Maximum { ${endpoint(100)} } }`;
  }
  const root=roots[0],out=[],visuals=[];let body='';
  for(const n of root.children){if(visualTypes.has(n.type)){flatten(n,[0,0,num(resolved.width,'width'),num(resolved.height,'height')],files,out,{...metrics,visuals});}else body+=serializeNode(n,true);}
  const sourceNode={...instance,type:'Button',bindings:{},events:instance.events.clicked?{clicked:instance.events.clicked}:{},props:Object.fromEntries(Object.entries(resolved).filter(([k])=>standard.has(k)&&k!=='font.size')),children:[]};
  const template=`component Button { Rectangle { ${Object.entries(root.props).filter(([k])=>k!=='key').map(([k,v])=>`${k}: ${literal(v)};`).join(' ')} ${body} ${out.join(' ')} ${rangeMetadata} } }`;
  visualNodes.push({id:-2,parent:null,control:index,type:instance.type,source:instance.source,props:resolved,propertyOrigins:origins,bounds:[0,0,num(resolved.width,'width'),num(resolved.height,'height')]},...visuals.map(v=>({...v,parent:v.parent??-2,control:index})));
  return {template,treeRoots:roots,sourceNode};
  }
  const controls=instances.map(compileInstance);
  const templateTree=createElementTree(controls.flatMap(c=>c.treeRoots));
  const sourceControls=controls.map(c=>c.sourceNode);
  const sourceScene=[{...root,type:'Frame',children:scroll?[{...scroll,children:sourceControls}]:sourceControls}];
  // Byte-length framing keeps arbitrary Unicode and quoted template text intact.
  const template=controls.length===1?controls[0].template:'FORMA-TEMPLATES-1\n'+controls.map(c=>`${templateEncoder.encode(c.template).length}\n${c.template}`).join('');
  return {source:`component ${document.name} { ${sourceScene.map(serialize).join(' ')} }`,template,instanceTree,templateTree,visualNodes,previewNodes,previewControls:instances};
}
