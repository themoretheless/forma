// Component linking is shared by Studio preview and native launch. It emits the
// same concrete primitive template; there is no DOM rendering of button content.
import {parse} from './language.js';
import {svgShapes} from './svg-shapes.js';

const own=(o,k)=>Object.hasOwn(o,k);
const clone=v=>structuredClone(v);
const standard=new Set('key width height text fontSize font.size color radius disabled background hoverBackground pressedBackground disabledBackground borderWidth borderColor focusBorderColor transitionDuration'.split(' '));
const fallback={width:260,height:70,text:'',fontSize:20,color:{expr:'#14213b'},radius:16,disabled:false,background:{expr:'#8ca5ff'},hoverBackground:{expr:'#a8baff'},pressedBackground:{expr:'#6a83da'},disabledBackground:{expr:'#596273'},borderWidth:1,borderColor:{expr:'#bed0ff'},focusBorderColor:{expr:'#ffffff'},transitionDuration:{expr:'140ms'}};
function visit(nodes,fn){for(const n of nodes){fn(n);visit(n.children??[],fn);}}
function substitute(v,key,base){
  if(v?.expr?.startsWith('base.')){if(v.expr!==`base.${key}`)throw Error(`В override ${key} допустим только base.${key}`);return clone(base);}
  if(Array.isArray(v))return v.map(x=>substitute(x,key,base));
  if(v&&typeof v==='object')return Object.fromEntries(Object.entries(v).map(([k,x])=>[k,substitute(x,key,base)]));
  return v;
}
function matches(pattern,value){
  if(pattern?.expr==='_')return true;
  if(Array.isArray(pattern))return Array.isArray(value)&&pattern.length===value.length&&pattern.every((p,i)=>matches(p,value[i]));
  if(pattern&&typeof pattern==='object')throw Error('Паттерн: литерал, кортеж или _');
  return pattern===value;
}
export function evaluate(v,props,state,stack=[]){
  if(Array.isArray(v))return v.map(x=>evaluate(x,props,state,stack));
  if(v&&typeof v==='object'&&own(v,'match')){
    const subject=evaluate(v.match,props,state,stack);
    const branch=v.branches.find(b=>matches(b.pattern,subject));
    if(!branch)throw Error('match: нет подходящей ветки; добавьте _ => …');
    return evaluate(branch.value,props,state,stack);
  }
  if(v?.expr){
    const neg=v.expr.startsWith('!'),path=neg?v.expr.slice(1):v.expr;
    let result;
    if(path.startsWith('props.')){
      const key=path.slice(6)==='font.size'?'fontSize':path.slice(6);
      if(!own(props,key))throw Error(`Неизвестное свойство ${path}`);
      if(stack.includes(key))throw Error(`Циклическая ссылка props: ${[...stack,key].join(' → ')}`);
      result=evaluate(props[key],props,state,[...stack,key]);
    }else if(path.startsWith('state.')){
      result=state;for(const key of path.slice(6).split('.')){if(result==null||!own(Object(result),key))throw Error(`Нет значения ${path}`);result=result[key];}
    }else if(path.startsWith('base.'))throw Error(`${path} допустим только внутри override`);
    else result=v;
    if(neg){if(typeof result!=='boolean')throw Error('! ожидает boolean');return !result;}return result;
  }
  if(v?.type){return {...v,props:Object.fromEntries(Object.entries(v.props).map(([k,x])=>[k,evaluate(x,props,state,stack)])),children:(v.children??[]).map(x=>evaluate(x,props,state,stack))};}
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
function serialize(n){if(Object.keys(n.bindings??{}).length||Object.keys(n.slots??{}).length)throw Error(`Необработанная привязка или override в ${n.type}`);return `${n.type} { ${Object.entries(n.props).map(([k,v])=>`${k}: ${literal(v)};`).join(' ')} ${Object.entries(n.events??{}).map(([k,v])=>`${k} -> ${v}();`).join(' ')} ${(n.children??[]).map(serialize).join(' ')} }`;}
const num=(v,label)=>{if(typeof v==='object'&&v?.expr?.endsWith('px'))v=Number(v.expr.slice(0,-2));if(typeof v!=='number'||!Number.isFinite(v)||v<0||v>4096)throw Error(`${label}: ожидается размер 0…4096`);return v;};
const color=v=>v?.expr??v;
function insets(v=0){const a=Array.isArray(v)?v:[v];if(a.length<1||a.length>4)throw Error('padding: от 1 до 4 размеров');const x=a.map(v=>num(v,'padding'));return x.length===1?[x[0],x[0],x[0],x[0]]:x.length===2?[x[0],x[1],x[0],x[1]]:x.length===3?[x[0],x[1],x[2],x[1]]:x;}
function tracks(v){return v===undefined?[{expr:'*'}]:Array.isArray(v)?v:[v];}
function cell(n,axis,count){const c=n.props.cell;if(c&&(!Array.isArray(c)||c.length!==2||own(n.props,'row')||own(n.props,'column')))throw Error('cell: ожидаются row column без отдельных row/column');const v=c?.[axis==='rows'?0:1]??n.props[axis==='rows'?'row':'column'];if(v===undefined)return null;if(!Number.isInteger(v)||v<1||v>count)throw Error(`Ячейка за пределами ${axis}`);return v-1;}
function gap(p){const a=Array.isArray(p.gap)?p.gap:[p.gap??0,p.gap??0];if(a.length!==2)throw Error('gap: один или два размера');return a.map(v=>num(v,'gap'));}
function natural(n,axis){
  const key=axis===0?'width':'height',p=n.props;
  if(p[key]!==undefined)return num(p[key],key);
  if(n.type==='Text'){const fs=num(p.fontSize??p['font.size']??16,'fontSize');return axis===0?[...String(p.text??'')].length*fs*.56:fs*1.3;}
  if(n.type==='Image')return 20;
  if(n.type==='Frame'){
    const ts=tracks(p[axis===0?'columns':'rows']),g=gap(p)[axis===0?1:0],pad=insets(p.padding);
    return ts.reduce((s,t,i)=>s+(typeof t==='number'?num(t,key):Math.max(0,...n.children.filter(c=>cell(c,axis===0?'columns':'rows',ts.length)===i||ts.length===1).map(c=>natural(c,axis)))),0)+g*(ts.length-1)+pad[axis===0?1:0]+pad[axis===0?3:2];
  }throw Error(`Контент ${n.type} пока не поддерживается`);
}
function sizes(n,axis,available){
  const name=axis===0?'columns':'rows',ts=tracks(n.props[name]),g=gap(n.props)[axis===0?1:0];let weight=0;
  const result=ts.map((t,i)=>{const x=t?.expr??t;if(typeof x==='number')return num(x,name);if(x==='-'||x==='content'||x==='auto')return Math.max(0,...n.children.filter(c=>cell(c,name,ts.length)===i||ts.length===1).map(c=>natural(c,axis)));if(typeof x==='string'&&/^(?:\d+(?:\.\d+)?)?\*$/.test(x)){const w=x==='*'?1:Number(x.slice(0,-1));if(w<=0)throw Error('Вес * должен быть положительным');weight+=w;return {w};}if(typeof x==='string'&&/^\d+(?:\.\d+)?%$/.test(x))return available*Number(x.slice(0,-1))/100;throw Error(`Неподдерживаемый размер ${x}`);});
  const remaining=Math.max(0,available-g*(ts.length-1)-result.reduce((s,v)=>s+(typeof v==='number'?v:0),0));return result.map(v=>typeof v==='number'?v:remaining*v.w/weight);
}
function flatten(n,box,files,output){
  const p=n.props;const allowed=new Set(['key','cell','row','column','width','height',...(n.type==='Frame'?['columns','rows','gap','padding','clip','radius']:n.type==='Text'?['text','color','fontSize','font.size']:n.type==='Image'?['source','color']:[])]);
  for(const k of Object.keys(p))if(!allowed.has(k))throw Error(`${n.type}.${k} пока не поддерживается в контенте`);
  if(Object.keys(n.events??{}).length||Object.keys(n.bindings??{}).length)throw Error('Контент кнопки пока не содержит отдельных интерактивных элементов');
  let [x,y,w,h]=box;if(p.width!==undefined){const v=num(p.width,'width');x+=(w-v)/2;w=v;}if(p.height!==undefined){const v=num(p.height,'height');y+=(h-v)/2;h=v;}
  if(n.type==='Frame'){
    if(p.clip!==undefined&&typeof p.clip!=='boolean')throw Error('Frame.clip ожидает boolean');
    const radius=num(p.radius??0,'radius');
    if(p.clip)output.push(`ContentClip { x: ${x}; y: ${y}; width: ${w}; height: ${h}; radius: ${radius}; }`);
    const pad=insets(p.padding);x+=pad[3];y+=pad[0];w=Math.max(0,w-pad[1]-pad[3]);h=Math.max(0,h-pad[0]-pad[2]);
    const cols=sizes(n,0,w),rows=sizes(n,1,h),g=gap(p);
    for(const c of n.children){const ci=cell(c,'columns',cols.length),ri=cell(c,'rows',rows.length);flatten(c,[ci===null?x:x+cols.slice(0,ci).reduce((a,b)=>a+b,0)+ci*g[1],ri===null?y:y+rows.slice(0,ri).reduce((a,b)=>a+b,0)+ri*g[0],ci===null?w:cols[ci],ri===null?h:rows[ri]],files,output);}if(p.clip)output.push('ContentClipEnd {}');return;
  }
  if(n.children?.length)throw Error(`${n.type} не принимает дочерние элементы`);
  if(n.type==='Text'){output.push(`ContentText { x: ${x}; y: ${y}; width: ${w}; height: ${h}; text: ${literal(p.text??'')}; color: ${literal(p.color??{expr:'#ffffff'})}; fontSize: ${num(p.fontSize??p['font.size']??16,'fontSize')}; }`);return;}
  if(n.type==='Image'){
    if(typeof p.source!=='string'||!own(files,p.source))throw Error(`SVG-файл не найден в проекте: ${p.source}`);
    for(const s of svgShapes(files[p.source],[x,y,w,h],color(p.color??'#ffffff')))output.push(`ContentShape { points: ${literal(s.points.map(v=>v.join(' ')).join(' '))}; color: ${s.color}; }`);return;
  }
  throw Error(`Неподдерживаемый контент ${n.type}`);
}

export function compileComponents(files,entry,state={}){
  const document=parse(files[entry]);const scene=clone(document.nodes);
  visit(scene,n=>{if(Object.keys(n.bindings??{}).length)throw Error('Векторный runtime пока не поддерживает двусторонние привязки');if(Object.keys(n.slots??{}).length)throw Error('override объявляется в наследнике component');});
  let instance=scene[0]?.children?.[0];if(instance?.type==='Scroll')instance=instance.children[0];if(!instance)throw Error('Ожидается Frame с экземпляром кнопки');
  const cache=new Map(),loading=[];
  function link(name){
    if(cache.has(name))return clone(cache.get(name));
    if(loading.includes(name))throw Error(`Цикл наследования: ${[...loading,name].join(' → ')}`);
    const path=`components/${name}.ui`;if(!files[path])throw Error(`Компонент не найден: ${path}`);
    const c=parse(files[path]);if(c.name!==name)throw Error(`${path}: ожидался component ${name}`);if(c.designs.length)throw Error('Дизайн-атрибут компонента пока не поддерживается векторным runtime');loading.push(name);
    const linked=c.base?link(c.base):{nodes:[],defaults:{}};
    if(c.base&&c.nodes.length)throw Error('Наследник меняет визуальное дерево через override');
    if(!c.base)linked.nodes=c.nodes;
    linked.defaults={...linked.defaults,...c.defaults};
    const presenters=new Map();visit(linked.nodes,n=>{if(n.type==='ContentPresenter'){const key=n.props.key;if(typeof key!=='string'||!key)throw Error('ContentPresenter требует key');if(presenters.has(key))throw Error(`Повторная точка расширения ${key}`);presenters.set(key,n);}});
    for(const [key,value] of Object.entries(c.slots)){
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
        const patch=parse(files[file],{fragment:true}).nodes[0];
        if(patch.type!==target.type)throw Error(`override ${key}: ожидался ${target.type}, получен ${patch.type} в ${file}`);
        for(const p of [patch,value.patch].filter(Boolean)){
          if(p.base||p.children.length||Object.keys(p.slots).length||Object.keys(p.events).length||Object.keys(p.bindings).length)throw Error('Файл override и локальный блок переопределяют только свойства');
          if(own(p.props,'key'))throw Error('override не меняет key целевого элемента');
          Object.assign(target.props,clone(p.props));
        }
        continue;
      }
      const target=presenters.get(key);if(!target)throw Error(`Нет точки расширения ${key} в ${name}`);
      if(!c.base)throw Error('override требует базового компонента');
      const base=target.props.content??(target.children.length===1?target.children[0]:{type:'Frame',props:{},children:target.children});
      target.props.content=substitute(value,key,base);target.children=[];
    }
    loading.pop();cache.set(name,clone(linked));return linked;
  }
  const linked=link(instance.type);
  let hasContent=false;visit(linked.nodes,n=>{if(n.type==='ContentPresenter'&&n.props.key==='content')hasContent=true;});
  if(instance.children.length&&!hasContent)throw Error('Для дочернего контента нужен ContentPresenter с key: content');
  for(const k of Object.keys(instance.props))if(!standard.has(k)&&!own(linked.defaults,k))throw Error(`Неизвестное свойство ${instance.type}.${k}`);
  if(instance.props['font.size']!==undefined&&instance.props.fontSize!==undefined)throw Error('fontSize и font.size — одно свойство');
  const props={...fallback,...linked.defaults,...instance.props};if(instance.props['font.size']!==undefined)props.fontSize=instance.props['font.size'];
  const resolved=Object.fromEntries(Object.entries(props).map(([k,v])=>[k,evaluate(v,props,state,[k])]));
  let roots=linked.nodes;
  function expand(n){
    if(n.type==='ContentPresenter'){
      if(Object.keys(n.props).some(k=>!['key','content'].includes(k)))throw Error('ContentPresenter принимает key и content');
      let content=n.props.content??(n.children.length===1?n.children[0]:{type:'Frame',props:{},children:n.children});
      if(instance.children.length){if(n.props.key!=='content')return expandContent(content);if(own(instance.props,'content'))throw Error('Задайте content или дочерние элементы');content={type:'Frame',props:{},children:instance.children};}
      return expandContent(content);
    }
    return {...n,props:Object.fromEntries(Object.entries(n.props).map(([k,v])=>[k,evaluate(v,props,state)])),children:(n.children??[]).map(expand)};
  }
  function expandContent(v){const result=evaluate(v,props,state);if(!result?.type)throw Error('override должен вернуть элемент или base.content');return expand(result);}
  roots=roots.map(expand);if(roots.length!==1||roots[0].type!=='Rectangle')throw Error('Базовая кнопка требует один Rectangle');
  // Keys are linker identities, not properties of the lowered Rust primitives.
  visit(roots,n=>{delete n.props.key;});
  const root=roots[0],out=[];let body='';
  for(const n of root.children){if(['Frame','Image','Text'].includes(n.type)){flatten(n,[0,0,num(resolved.width,'width'),num(resolved.height,'height')],files,out);}else body+=serialize(n);}
  instance.type='Button';instance.props=Object.fromEntries(Object.entries(resolved).filter(([k])=>standard.has(k)&&k!=='font.size'));instance.children=[];
  const template=`component Button { Rectangle { ${Object.entries(root.props).map(([k,v])=>`${k}: ${literal(v)};`).join(' ')} ${body} ${out.join(' ')} } }`;
  return {source:`component ${document.name} { ${scene.map(serialize).join(' ')} }`,template};
}
