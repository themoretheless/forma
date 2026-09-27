import {designReference,readDesignData} from './design-data.js';
import {tokenize,createValueParser,evaluateExpression} from './expressions.js';
export function parse(source, {fragment=false}={}) {
  const tokens=tokenize(source);let i=0;
  const peek=()=>tokens[i]?.v;
  const take=expected=>{const t=tokens[i++];if(!t||expected&&t.v!==expected)throw Error(`Строка ${source.slice(0,t?.pos??source.length).split('\n').length}: ожидалось ${expected||'значение'}, получено ${t?.v??'конец файла'}`);return t.v;};
  const identifier=label=>{const name=take();if(!/^[A-Za-z_]\w*$/.test(name))throw Error(`${label}: ожидалось имя, получено ${name}`);return name;};
  const semi=()=>{if(peek()===';')take(';');};
  const value=createValueParser({peek,take},{special(){
    if(peek()==='match')return match(false);
    if(/^[A-Z]/.test(peek()??'')&&tokens[i+1]?.v==='{'){
      const typed=node();if(typed.type==='Brush'&&(typed.children.length||Object.keys(typed.bindings).length||Object.keys(typed.events).length))throw Error('Brush содержит только свойства');return typed;
    }
  }});
  function sequence(end=';'){const out=[value()];while(peek()!==end){if(peek()===undefined)take(end);out.push(value());}return out.length===1?out[0]:out;}
  function tuple(read=value){if(peek()!=='(')return read();take('(');const values=[read()];while(peek()===','){take(',');values.push(read());}take(')');return values.length===1?values[0]:values;}
  function pattern(){
    if(peek()==='(')return tuple(pattern);
    if(['<','<=','>','>='].includes(peek())){const comparison=take();return {comparison,value:value()};}
    const p=value();if(p?.expression||p?.interpolation||p&&typeof p==='object'&&!Array.isArray(p)&&!p.expr)throw Error('Паттерн: литерал, значение enum, сравнение, кортеж или _');return p;
  }
  function match(group){
    take('match');const subject=tuple();take('{');const branches=[];const seen=new Set();
    while(peek()!=='}'){
      const p=pattern(),key=JSON.stringify(p);if(seen.has(key))throw Error('Повторный паттерн match');seen.add(key);take('=>');
      if(group){const patch=node('Patch');if(patch.children.length||patch.matches.length||patch.forward.length||Object.keys(patch.bindings).length||Object.keys(patch.events).length||Object.keys(patch.slots).length)throw Error('Групповой match содержит только свойства');branches.push({pattern:p,props:patch.props,propertyRanges:patch.propertyRanges,statementRanges:patch.statementRanges});semi();}
      else{const result=sequence();take(';');branches.push({pattern:p,value:result});}
    }take('}');if(!branches.length)throw Error('match требует хотя бы одну ветку');return {match:subject,branches};
  }
  function emptyNode(type,start){return {type,start,props:{},propertyRanges:{},statementRanges:{},bindings:{},events:{},children:[],slots:{},matches:[],forward:[]};}
  function children(){take('{');const out=[];while(peek()!=='}')out.push(child());const end=tokens[i]?.pos+1;take('}');return {nodes:out,end};}
  function child(){
    if(peek()==='if'){
      const n=emptyNode('If',tokens[i].pos);take('if');n.condition=value();const block=children();n.children=block.nodes;n.end=block.end;n.elseChildren=[];
      if(peek()==='else'){take('else');if(peek()==='if'){const other=child();n.elseChildren=[other];n.end=other.end;}else{const other=children();n.elseChildren=other.nodes;n.end=other.end;}}return n;
    }
    if(peek()==='for'){
      const n=emptyNode('For',tokens[i].pos);take('for');n.item=identifier('for');if(['state','props','events','actions','base','design','viewport'].includes(n.item))throw Error(`for: зарезервированное имя ${n.item}`);take('in');n.collection=value();take('key');n.key=value();const block=children();n.children=block.nodes;n.end=block.end;n.emptyChildren=[];
      if(peek()==='empty'){take('empty');const other=children();n.emptyChildren=other.nodes;n.end=other.end;}return n;
    }
    return node();
  }
  function typeName(){let type=take();if(!/^[A-Za-z_]\w*(?:\.[A-Za-z_]\w*)*$/.test(type))throw Error('Ожидалось имя типа');if(peek()==='?'){take('?');type+='?';}return type;}
  function declaration(n){
    const start=tokens[i].pos;let required=false;if(peek()==='required'){take();required=true;}
    const kind=take();if(required&&kind!=='prop')throw Error('required применяется только к prop');
    const name=identifier(kind);
    if(kind==='enum'){
      if(Object.hasOwn(n.enums,name))throw Error(`Повторный enum ${name}`);take('{');const variants=[];
      while(peek()!=='}'){const variant=identifier('enum');if(variants.includes(variant))throw Error(`Повторный вариант ${name}.${variant}`);variants.push(variant);if(peek()!=='}')take(',');}
      take('}');semi();if(!variants.length)throw Error(`enum ${name} требует варианты`);n.enums[name]=variants;return;
    }
    if(Object.hasOwn(n.statementRanges,name)||Object.hasOwn(n.propDefinitions,name)||Object.hasOwn(n.eventDefinitions,name))throw Error(`Повторное свойство или событие ${name}`);
    if(kind==='prop'){
      take(':');const type=typeName();n.propDefinitions[name]={type,required};
      if(peek()==='='){if(required)throw Error(`required prop ${name} не принимает значение по умолчанию`);take('=');const from=tokens[i]?.pos;n.props[name]=sequence();const last=tokens[i-1];n.propertyRanges[name]={from,to:last.pos+last.v.length};}
      else if(!required&&!type.endsWith('?'))throw Error(`prop ${name}: требуется значение по умолчанию или required`);
      else if(type.endsWith('?')&&!required)n.props[name]=null;
    }else if(kind==='event'){
      take('(');const args=[],names=new Set();while(peek()!==')'){const arg=identifier('event');if(names.has(arg))throw Error(`Повторный аргумент ${arg}`);names.add(arg);take(':');args.push({name:arg,type:typeName()});if(peek()!==')')take(',');}take(')');n.eventDefinitions[name]=args;
    }else throw Error(`Неизвестное объявление ${kind}`);
    const end=tokens[i]?.pos+1;take(';');n.statementRanges[name]={from:start,to:end};
  }
  function node(implicitType,component=false){
    const start=tokens[i]?.pos,type=implicitType??take();if(!implicitType&&['If','For'].includes(type))throw Error(`${type}: зарезервированный тип`);
    const n=emptyNode(type,start);if(component){n.propDefinitions={};n.eventDefinitions={};n.enums={};}
    if(peek()===':'){take(':');n.base=take();}take('{');
    while(peek()!=='}'){
      if(['required','prop','event','enum'].includes(peek())){if(!component)throw Error('Объявления prop, event и enum разрешены только в component');declaration(n);continue;}
      if(peek()==='match'){n.matches.push(match(true));semi();continue;}
      if(peek()==='forward'){
        take('forward');take('props');take('{');while(peek()!=='}'){const key=identifier('forward');if(n.forward.includes(key))throw Error(`Повторный forward ${key}`);n.forward.push(key);if(peek()!=='}')take(',');}take('}');take(';');continue;
      }
      if(peek()==='if'||peek()==='for'){n.children.push(child());continue;}
      if(peek()==='override'){
        take('override');let key=take();if(/^['"]/.test(key))key=key.slice(1,-1);
        if(Object.hasOwn(n.slots,key))throw Error(`Повторный override ${key}`);
        if(peek()==='{'){n.slots[key]={propertyOverride:true,patch:node('Patch')};semi();}
        else if(peek()==='from'){take('from');const file=value();if(typeof file!=='string')throw Error('override from требует путь в кавычках');const patch=peek()==='{'?node('Patch'):null;if(!patch||peek()===';')take(';');n.slots[key]={fileOverride:true,file,patch};}
        else{take(':');n.slots[key]=value();take(';');}continue;
      }
      if(tokens[i+1]?.v==='{'){n.children.push(node());continue;}
      const statementStart=tokens[i]?.pos,key=take(),op=take();
      if(Object.hasOwn(n.statementRanges,key))throw Error(`Повторное свойство или событие ${key}`);
      if(op===':'){const from=tokens[i]?.pos;n.props[key]=sequence();const last=tokens[i-1];n.propertyRanges[key]={from,to:last.pos+last.v.length};}
      else if(op==='<->'){const path=take();if(!/^[A-Za-z_]\w*(?:\.[A-Za-z_]\w*)+$/.test(path))throw Error('Двусторонняя привязка требует путь к полю');n.bindings[key]=path;}
      else if(op==='->'){const action=take();if(!/^(?:actions|events|state)\.[A-Za-z_]\w*(?:\.[A-Za-z_]\w*)*$/.test(action))throw Error('Обработчик требует путь actions.…, events.… или state.…');take('(');const args=[];while(peek()!==')'){args.push(value());if(peek()!==')')take(',');}take(')');n.events[key]=action;if(args.length)(n.eventArgs??={})[key]=args;}
      else throw Error(`Неизвестный оператор ${op}`);
      const statementEnd=tokens[i]?.pos+1;take(';');n.statementRanges[key]={from:statementStart,to:statementEnd};
    }n.end=tokens[i]?.pos+1;take('}');return n;
  }
  // One `Type { key: …; prop: … }` entry of a design document, written into the maps the caller
  // owns: the document body fills the base overrides, a state block fills its own.
  // The node itself is kept as well, key included, so the designer can edit an entry in place
  // while the preview patch stays the keyless property map.
  function designEntry(overrides,overrideTypes,nodes){
    if(!/^[A-Z][\w]*$/.test(peek()??''))throw Error('В design ожидается тип элемента с явным key');
    const item=node(),key=item.props.key;
    if(typeof key!=='string'||!key)throw Error('Дизайн-key должен быть непустой строкой');
    if(Object.hasOwn(overrides,key))throw Error(`Повторный дизайн-key ${key}`);
    if(item.children.length||item.matches.length||item.forward.length||Object.keys(item.events).length||Object.keys(item.bindings).length)throw Error('Дизайн переопределяет только свойства');
    const props={...item.props};delete props.key;nodes.push(item);
    Object.defineProperty(overrideTypes,key,{value:item.type,enumerable:true});Object.defineProperty(overrides,key,{value:props,enumerable:true});
  }
  // A state is a named second source file's worth of overrides for the same keys; the offsets are
  // what the designer's states panel edits in place.
  function designState(){
    const start=tokens[i].pos;take('state');
    const literal=tokens[i],label=value();
    if(!/^['"]/.test(literal?.v??''))throw Error('Имя состояния ожидает строку в кавычках');
    if(typeof label!=='string'||!label)throw Error('Имя состояния должно быть непустым');
    const nameEnd=literal.pos+literal.v.length;take('{');
    const overrides={},overrideTypes={},nodes=[];while(peek()!=='}')designEntry(overrides,overrideTypes,nodes);
    const end=(tokens[i]?.pos??0)+1;take('}');
    return {name:label,start,end,nameStart:literal.pos,nameEnd,overrides,overrideTypes,nodes};
  }
  const designs=[];while(peek()==='#'){take('#');take('[');take('design');if(peek()==='('){take('(');designs.push(value());take(')');}take(']');}
  const nodes=[],states=[],entries=[],overrides={},overrideTypes={},defaults={};let name='Preview',base=null,slots={},defaultRanges={},propDefinitions={},eventDefinitions={},enums={},matches=[],forward=[],designBody=null;
  if(fragment)nodes.push(node());
  else if(peek()==='component'){
    take('component');const c=node(undefined,true);name=c.type;base=c.base??null;slots=c.slots;defaultRanges=c.statementRanges;nodes.push(...c.children);Object.assign(defaults,c.props);({propDefinitions,eventDefinitions,enums,matches,forward}=c);
    if(Object.keys(c.bindings).length||Object.keys(c.events).length)throw Error('На уровне component разрешены только значения свойств и объявления event');
  }else if(peek()==='design'){
    // `entries` keeps the base overrides as nodes and `designBody` marks the braces the states
    // panel appends a block inside, so the same primitives that edit a component edit a design file.
    take('design');name=take();const open=tokens[i];take('{');while(peek()!=='}'){
      if(peek()!=='state'){designEntry(overrides,overrideTypes,entries);continue;}
      const state=designState();
      if(states.some(other=>other.name===state.name))throw Error(`Повторное состояние ${state.name}`);
      states.push(state);
    }
    designBody={from:open.pos,to:tokens[i]?.pos??0};take('}');
  }else throw Error('Ожидается component или design. Старый preview/state больше не поддерживается; используйте design с переопределениями по key.');
  if(peek())throw Error('Лишний текст после компонента');
  function check(list,keys=new Set()){for(const n of list){if('key'in n.props){const key=n.props.key;if(typeof key!=='string'||!key)throw Error('key должен быть непустой строкой');if(keys.has(key))throw Error(`Повторный key ${key}`);keys.add(key);}check(n.children,n.type==='For'?new Set():keys);check(n.elseChildren??[],keys);check(n.emptyChildren??[],keys);}}check(nodes);
  return {name,nodes,designs,states,entries,designBody,overrides,overrideTypes,defaults,defaultRanges,base,slots,propDefinitions,eventDefinitions,enums,matches,forward};
}
export function validateDesign(component,design){
  if(design.name!==component.name)throw Error(`Дизайн ${design.name} не соответствует ${component.name}`);
  const keys=new Map();function walk(nodes){for(const n of nodes){keys.set(n.props.key,n.type);walk(n.children);walk(n.elseChildren??[]);walk(n.emptyChildren??[]);}}walk(component.nodes);
  function check(overrides,overrideTypes,where){for(const key of Object.keys(overrides)){if(!keys.has(key))throw Error(`Неизвестный дизайн-key ${key}`);if(keys.get(key)!==overrideTypes[key])throw Error(`Тип дизайн-key ${key}: ожидался ${keys.get(key)}, получен ${overrideTypes[key]}${where}`);}}
  check(design.overrides,design.overrideTypes,'');
  for(const state of design.states??[])check(state.overrides,state.overrideTypes,` в состоянии ${state.name}`);
}
// The patch a preview applies for one state: the base entries with the state's props written over
// them, property by property, so an author names only what changes.
export function designStatePatch(design,stateName=null){
  const merged={};for(const key of Object.keys(design.overrides??{}))merged[key]={...design.overrides[key]};
  if(stateName==null)return merged;
  const state=(design.states??[]).find(s=>s.name===stateName);
  if(!state)throw Error(`Состояние ${stateName} не объявлено`);
  for(const key of Object.keys(state.overrides)){merged[key]??={};Object.assign(merged[key],state.overrides[key]);}
  return merged;
}
export function resolve(v,state){
  const ref=designReference(v);if(ref)return readDesignData(ref);
  if(Array.isArray(v))return v.map(x=>resolve(x,state)).join(' ');
  return evaluateExpression(v,(path)=>{const ref=designReference(path);if(ref)return readDesignData(ref);return path.startsWith('state.')?path.slice(6).split('.').reduce((o,k)=>o?.[k],state):path;});
}
