import {designReference,readDesignData} from './design-data.js';
export function parse(source, {fragment=false}={}) {
  const tokens = []; let pos = 0;
  const re = /\s+|\/\/[^\n]*|\/\*[\s\S]*?\*\/|'(?:\\.|[^'\\])*'|"(?:\\.|[^"\\])*"|<->|->|=>|#[\da-fA-F]{3,8}\b|-?\d+(?:\.\d+)?(?:%|ms|px|deg|fr|[*])?|[A-Za-z_][\w.-]*|[{}\[\]():;,!@#*-]/gy;
  while (pos < source.length) {
    re.lastIndex = pos; const m = re.exec(source);
    if (!m) throw new Error(`Строка ${source.slice(0,pos).split('\n').length}: неожиданный символ «${source[pos]}»`);
    if (!/^\s|^\/\//.test(m[0]) && !m[0].startsWith('/*')) tokens.push({v:m[0],pos});
    pos = re.lastIndex;
  }
  let i = 0; const peek = () => tokens[i]?.v;
  const take = expected => { const t=tokens[i++]; if (!t || expected && t.v!==expected) throw new Error(`Строка ${source.slice(0,t?.pos ?? source.length).split('\n').length}: ожидалось ${expected || 'значение'}, получено ${t?.v ?? 'конец файла'}`); return t.v; };
  function value() {
    if(peek()==='match'){
      take('match');let subject;
      if(peek()==='('){take('(');subject=[value()];while(peek()===','){take(',');subject.push(value());}take(')');}else subject=value();
      take('{');const branches=[];
      while(peek()!=='}'){
        let pattern;if(peek()==='('){take('(');pattern=[value()];while(peek()===','){take(',');pattern.push(value());}take(')');}else pattern=value();
        take('=>');const result=value();take(';');branches.push({pattern,value:result});
      }take('}');return {match:subject,branches};
    }
    if(peek()==='Brush'&&tokens[i+1]?.v==='{'){
      const brush=node();
      if(brush.children.length||Object.keys(brush.bindings).length||Object.keys(brush.events).length)throw Error('Brush содержит только свойства');
      return {type:'Brush',props:brush.props};
    }
    if(/^[A-Z]/.test(peek()??'')&&tokens[i+1]?.v==='{')return node();
    if (peek()==='{') { take('{'); const out={}; while(peek()!=='}') {const k=take();take(':');out[k]=value();take(';');}take('}');return out; }
    if (peek()==='[') {take('[');const out=[];while(peek()!==']'){out.push(value());if(peek()!==']')take(',');}take(']');return out;}
    const v=take(); if (v[0]==="'" || v[0]==='"') return v.slice(1,-1).replace(/\\(['"\\])/g,'$1').replace(/\\n/g,'\n');
    if(v==='true'||v==='false')return v==='true'; if (/^-?\d+(\.\d+)?$/.test(v)) return Number(v);
    if(v==='!')return {expr:'!'+take()};
    if(peek()==='('){take('(');let args=[];while(peek()!==')')args.push(take());take(')');return v+'('+args.join(' ')+')';}
    return {expr:v};
  }
  function node(implicitType) {
    const start=tokens[i]?.pos; const type=implicitType??take(); const n={type,start,props:{},bindings:{},events:{},children:[],slots:{}};
    if(peek()===':'){take(':');n.base=take();}take('{');
    while(peek()!=='}') {
      if(peek()==='override'){
        take('override');let key=take();if (/^['"]/.test(key)) key=key.slice(1,-1);
        if(Object.hasOwn(n.slots,key))throw Error(`Повторный override ${key}`);
        if(peek()==='from'){
          take('from');const file=value();if(typeof file!=='string')throw Error('override from требует путь в кавычках');
          const patch=peek()==='{'?node('Patch'):null;
          if(!patch||peek()===';')take(';');
          n.slots[key]={fileOverride:true,file,patch};
        }else{take(':');n.slots[key]=value();take(';');}continue;
      }
      if(tokens[i+1]?.v==='{'){n.children.push(node());continue;}
      const key=take(), op=take();
      if(op===':'){if(key in n.props)throw new Error(`Повторное свойство ${key}`);const v=value();const values=[v];while(peek()!==';')values.push(value());n.props[key]=values.length===1?v:values;}
      else if(op==='<->')n.bindings[key]=take();
      else if(op==='->'){const action=take();take('(');take(')');n.events[key]=action;}
      else throw new Error(`Неизвестный оператор ${op}`);
      take(';');
    }
    n.end=tokens[i]?.pos+1;take('}');return n;
  }
  const designs=[];
  while(peek()==='#'){take('#');take('[');take('design');if(peek()==='('){take('(');designs.push(value());take(')');}take(']');}
  const nodes=[], scenarios=[], overrides={}, overrideTypes={},defaults={};let name='Preview',base=null,slots={};
  if(fragment){nodes.push(node());}
  else if(peek()==='component'){take('component');const component=node();name=component.type;base=component.base??null;slots=component.slots;nodes.push(...component.children);Object.assign(defaults,component.props);if(Object.keys(component.bindings).length||Object.keys(component.events).length)throw Error('На уровне component пока разрешены только значения свойств');}
  else if(peek()==='design'){
    take('design');name=take();take('{');
    while(peek()!=='}'){
      if(!/^[A-Z][\w]*$/.test(peek()??''))throw Error('В design ожидается тип элемента с явным key');
      const item=node();const key=item.props.key;
      if(typeof key!=='string'||!key)throw Error('Дизайн-key должен быть непустой строкой');
      if(Object.hasOwn(overrides,key))throw Error(`Повторный дизайн-key ${key}`);
      if(item.children.length||Object.keys(item.events).length||Object.keys(item.bindings).length)throw Error('Дизайн переопределяет только свойства');
      delete item.props.key;
      Object.defineProperty(overrideTypes,key,{value:item.type,enumerable:true});
      Object.defineProperty(overrides,key,{value:item.props,enumerable:true});
    }take('}');
  }
  else throw Error('Ожидается component или design. Старый preview/state больше не поддерживается; используйте design с переопределениями по key.');
  if(peek())throw new Error('Лишний текст после компонента');
  const keys=new Set();
  function check(list){for(const n of list){if('key' in n.props){const key=n.props.key;if(typeof key!=='string'||!key)throw Error('key должен быть непустой строкой');if(keys.has(key))throw Error(`Повторный key ${key}`);keys.add(key);}check(n.children);}}check(nodes);
  return {name,nodes,designs,scenarios,overrides,overrideTypes,defaults,base,slots};
}
export function validateDesign(component,design){
  if(design.name!==component.name)throw Error(`Дизайн ${design.name} не соответствует ${component.name}`);
  const keys=new Map();function walk(nodes){for(const n of nodes){keys.set(n.props.key,n.type);walk(n.children);}}walk(component.nodes);
  for(const key of Object.keys(design.overrides)){
    if(!keys.has(key))throw Error(`Неизвестный дизайн-key ${key}`);
    if(keys.get(key)!==design.overrideTypes[key])throw Error(`Тип дизайн-key ${key}: ожидался ${keys.get(key)}, получен ${design.overrideTypes[key]}`);
  }
}
export function resolve(v,state) {
  const ref=designReference(v);if(ref)return readDesignData(ref);
  if(Array.isArray(v))return v.map(x=>resolve(x,state)).join(' ');
  if(v&&typeof v==='object'&&'expr'in v){const neg=v.expr.startsWith('!'); const path=(neg?v.expr.slice(1):v.expr);const out=path.startsWith('state.')?path.slice(6).split('.').reduce((o,k)=>o?.[k],state):path;return neg?!out:out;}
  return v;
}
