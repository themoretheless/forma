// Serializable, deliberately small expression language. No host-language eval.
const own=(o,k)=>Object.hasOwn(o,k);
export const pureFunctions=new Set(['min','max','clamp','abs','round','floor','ceil','len','String','Number','Bool']);
const precedence={'??':1,'||':2,'&&':3,'==':4,'!=':4,'===':4,'!==':4,'<':5,'<=':5,'>':5,'>=':5,'+':6,'-':6,'*':7,'/':7,'%':7};
function stringEnd(source,start){
  const quote=source[start];let i=start+1;
  while(i<source.length){
    if(source[i]==='\\'){i+=2;continue;}
    if(source[i]===quote)return i+1;
    if(source[i]==='$'&&source[i+1]==='{'){
      i+=2;let depth=1;
      while(i<source.length&&depth){
        if(source[i]==="'"||source[i]==='"'){i=stringEnd(source,i);continue;}
        if(source[i]==='{')depth++;if(source[i]==='}')depth--;i++;
      }
      if(depth)throw Error('Незавершённая интерполяция ${…}');continue;
    }
    i++;
  }
  throw Error('Незавершённая строка');
}
export function tokenize(source,offset=0){
  const tokens=[];let pos=0;
  const re=/\s+|\/\/[^\n]*|\/\*[\s\S]*?\*\/|<->|->|=>|===|!==|==|!=|<=|>=|&&|\|\||\?\?|\?\.|#[\da-fA-F]{3,8}\b|\d+(?:\.\d+)?(?:ms|px|deg|fr|%(?![\w.(])|\*(?![\w.(]))?|[A-Za-z_][\w]*(?:\.[A-Za-z_][\w]*)*|[{}\[\]():;,!@#*+\-/%%<>=?.]/gy;
  while(pos<source.length){
    if(source[pos]==="'"||source[pos]==='"'){const end=stringEnd(source,pos);tokens.push({v:source.slice(pos,end),pos:pos+offset});pos=end;continue;}
    re.lastIndex=pos;const m=re.exec(source);
    if(!m)throw Error(`Строка ${source.slice(0,pos).split('\n').length}: неожиданный символ «${source[pos]}»`);
    if(!/^\s|^\/\//.test(m[0])&&!m[0].startsWith('/*'))tokens.push({v:m[0],pos:pos+offset});pos=re.lastIndex;
  }
  return tokens;
}
function decode(s){return s.replace(/\\(['"\\nrt$])/g,(_,c)=>({n:'\n',r:'\r',t:'\t'}[c]??c));}
export function parseString(token){
  const body=token.slice(1,-1),parts=[];let start=0,i=0;
  while(i<body.length){
    if(body[i]==='\\'){i+=2;continue;}
    if(body[i]==='$'&&body[i+1]==='{'){
      if(i>start)parts.push(decode(body.slice(start,i)));
      const from=i+2;let end=from,depth=1;
      while(end<body.length&&depth){if(body[end]==="'"||body[end]==='"'){end=stringEnd(body,end);continue;}if(body[end]==='{')depth++;if(body[end]==='}')depth--;if(depth)end++;}
      if(depth)throw Error('Незавершённая интерполяция ${…}');
      parts.push(parseExpression(body.slice(from,end)));i=end+1;start=i;continue;
    }i++;
  }
  if(!parts.length)return decode(body);
  if(start<body.length)parts.push(decode(body.slice(start)));
  return {interpolation:parts};
}
export function createValueParser(cursor,{special}={}){
  const {peek,take}=cursor;
  function primary(){
    const custom=special?.();if(custom!==undefined)return custom;
    const token=take();
    if(token==='('){const v=expression();take(')');return v;}
    if(token==='['){const out=[];while(peek()!==']'){out.push(expression());if(peek()!==']')take(',');}take(']');return out;}
    if(token==='{'){const out={};while(peek()!=='}'){const k=take();if(own(out,k))throw Error(`Повторное свойство ${k}`);take(':');out[k]=expression();take(';');}take('}');return out;}
    if(token==='!'||token==='-'||token==='+'){
      if(token==='-'&&[undefined,';',',',']',')','}'].includes(peek()))return {expr:'-'};
      const argument=expression(8);
      if(token==='!'&&argument?.expr&&/^[\w.]+$/.test(argument.expr))return {expr:'!'+argument.expr};
      if(typeof argument==='number'&&token!=='!')return token==='-'?-argument:argument;
      if(token==='-'&&/^\d+(?:\.\d+)?(?:px|ms|deg|fr|%)$/.test(argument?.expr??''))return {expr:'-'+argument.expr};
      return {expression:{kind:'unary',operator:token,argument}};
    }
    if(token[0]==="'"||token[0]==='"')return parseString(token);
    if(token==='true'||token==='false')return token==='true';if(token==='null')return null;
    if(/^\d+(\.\d+)?$/.test(token))return Number(token);
    if(peek()==='('){
      take('(');const args=[];while(peek()!==')'){args.push(expression());if(peek()!==')')take(',');}take(')');
      if(token==='c'||token.startsWith('design.'))return `${token}(${args.map(serializeValue).join(', ')})`;
      if(!pureFunctions.has(token))throw Error(`Неизвестная чистая функция ${token}`);
      return {expression:{kind:'call',name:token,args}};
    }
    if(!/^[\w#*]/.test(token))throw Error(`Ожидалось значение, получено ${token}`);
    return {expr:token};
  }
  function expression(min=0){
    let left=primary();
    while(true){
      const op=peek();
      if(op==='?.'||op==='.'||op==='['){
        take();const optional=op==='?.',computed=op==='['||optional&&peek()==='[';let property;
        if(op==='['||optional&&peek()==='['){if(op!=='[')take('[');property=expression();take(']');}else{property=take();if(!/^[A-Za-z_]\w*(?:\.[A-Za-z_]\w*)*$/.test(property))throw Error('Ожидалось имя свойства');}
        if(computed)left={expression:{kind:'member',object:left,property,optional,computed}};
        else for(const [index,part]of property.split('.').entries())left={expression:{kind:'member',object:left,property:part,optional:index===0&&optional,computed:false}};
        continue;
      }
      if(op==='?'&&min===0){take('?');const then=expression();take(':');const otherwise=expression();left={expression:{kind:'conditional',condition:left,then,else:otherwise}};continue;}
      const rank=precedence[op];if(rank===undefined||rank<min)break;
      take();left={expression:{kind:'binary',operator:op,left,right:expression(rank+1)}};
    }
    return left;
  }
  return expression;
}
export function parseExpression(source){
  const tokens=tokenize(source);let i=0;const cursor={peek:()=>tokens[i]?.v,take:expected=>{const t=tokens[i++];if(!t||expected&&t.v!==expected)throw Error(`Ожидалось ${expected??'значение'}, получено ${t?.v??'конец выражения'}`);return t.v;}};
  const value=createValueParser(cursor)();if(i!==tokens.length)throw Error(`Лишний текст в выражении: ${tokens[i].v}`);return value;
}
function truth(value){if(typeof value!=='boolean')throw Error('Логическое выражение ожидает Bool');return value;}
function textValue(value){if(typeof value?.expr==='string')return value.expr;if(value!==null&&typeof value==='object'||!['undefined','boolean','number','string'].includes(typeof value)&&value!==null)throw Error('Строковое преобразование ожидает скалярное значение');return String(value??'');}
function numeric(value){if(typeof value!=='number'||!Number.isFinite(value))throw Error('Арифметика ожидает конечное число');return value;}
// Values in the markup are data: collection equality is structural and does
// not depend on JavaScript object identity or object-key insertion order.
export function equalValues(a,b){
  if(a===b)return true;
  if(a===null||b===null||typeof a!=='object'||typeof b!=='object')return false;
  if(Array.isArray(a)||Array.isArray(b))return Array.isArray(a)&&Array.isArray(b)&&a.length===b.length&&a.every((value,index)=>equalValues(value,b[index]));
  const keys=Object.keys(a);return keys.length===Object.keys(b).length&&keys.every(key=>own(b,key)&&equalValues(a[key],b[key]));
}
function compareValues(a,b){
  if(typeof a!==typeof b||!['number','string'].includes(typeof a))return null;
  if(typeof a==='number')return a===b?0:a<b?-1:1;
  // Rust strings compare Unicode scalar values, whereas JavaScript's built-in
  // ordering compares UTF-16 code units. Make the language portable explicitly.
  const left=Array.from(a),right=Array.from(b);
  for(let i=0;i<Math.min(left.length,right.length);i++){const delta=left[i].codePointAt(0)-right[i].codePointAt(0);if(delta)return Math.sign(delta);}
  return Math.sign(left.length-right.length);
}
export function evaluateExpression(value,resolveReference){
  function run(v,optional=false){
    if(Array.isArray(v))return v.map(x=>run(x));
    if(v===null||typeof v!=='object')return v;
    if(own(v,'expr')){const neg=v.expr.startsWith('!');const result=resolveReference(neg?v.expr.slice(1):v.expr,{optional});return neg?!truth(result):result;}
    if(v.interpolation)return v.interpolation.map(x=>typeof x==='string'?x:textValue(run(x))).join('');
    if(own(v,'match')){const actual=run(v.match),branch=v.branches.find(b=>matchesPattern(b.pattern,actual,x=>run(x)));if(!branch)throw Error('match: нет подходящей ветки; добавьте _ => …');return run(branch.value);}
    if(!v.expression)return Object.fromEntries(Object.entries(v).map(([k,x])=>[k,run(x)]));
    const e=v.expression;
    if(e.kind==='conditional')return run(truth(run(e.condition))?e.then:e.else);
    if(e.kind==='unary'){const a=run(e.argument);if(e.operator==='!')return !truth(a);return e.operator==='-'?-numeric(a):numeric(a);}
    if(e.kind==='member'){
      const object=run(e.object,optional||e.optional);
      if(object==null){if(e.optional||optional)return null;throw Error('Нельзя прочитать свойство пустого значения; используйте ?.');}
      const property=typeof e.property==='string'?e.property:run(e.property);
      if(['__proto__','prototype','constructor'].includes(String(property)))throw Error(`Недоступное свойство ${property}`);
      if(typeof object==='string'&&property==='length')return object.length;
      if(!own(Object(object),property)){if(e.optional||optional)return null;throw Error(`Неизвестное свойство ${property}`);}return object[property];
    }
    if(e.kind==='call'){
      const a=e.args.map(x=>run(x)),arity=(min,max=min)=>{if(a.length<min||a.length>max)throw Error(`${e.name}: неверное число аргументов`);};
      if(e.name==='String'){arity(1);return textValue(a[0]);}
      if(e.name==='Number'){arity(1);if(typeof a[0]!=='number'&&(typeof a[0]!=='string'||!/^[-+]?(?:\d+(?:\.\d*)?|\.\d+)(?:[eE][-+]?\d+)?$/.test(a[0].trim())))throw Error('Number ожидает число или непустую числовую строку');return numeric(Number(a[0]));}
      if(e.name==='Bool'){arity(1);return truth(a[0]);}
      if(e.name==='len'){arity(1);if(typeof a[0]!=='string'&&!Array.isArray(a[0]))throw Error('len ожидает строку или массив');return a[0].length;}
      if(e.name==='clamp'){arity(3);a.forEach(numeric);if(a[1]>a[2])throw Error('clamp: минимум превышает максимум');return Math.max(a[1],Math.min(a[2],a[0]));}
      if(['min','max'].includes(e.name)){arity(1,Infinity);return Math[e.name](...a.map(numeric));}
      if(['abs','round','floor','ceil'].includes(e.name)){arity(1);return Math[e.name](numeric(a[0]));}
      throw Error(`Неизвестная чистая функция ${e.name}`);
    }
    if(e.kind==='binary'){
      const op=e.operator,a=run(e.left,op==='??'||optional);
      if(op==='??')return a==null?run(e.right):a;if(op==='&&')return truth(a)?truth(run(e.right)):false;if(op==='||')return truth(a)?true:truth(run(e.right));
      const b=run(e.right);
      if(op==='=='||op==='===')return equalValues(a,b);if(op==='!='||op==='!==')return !equalValues(a,b);
      if(['<','<=','>','>='].includes(op)){const order=compareValues(a,b);if(order===null)throw Error('Сравнение ожидает два числа или две строки');return op==='<'?order<0:op==='<='?order<=0:op==='>'?order>0:order>=0;}
      if(op==='+'&&typeof a==='string'&&typeof b==='string')return a+b;
      numeric(a);numeric(b);const result=op==='+'?a+b:op==='-'?a-b:op==='*'?a*b:op==='/'?a/b:a%b;return numeric(result);
    }
    throw Error(`Неизвестное выражение ${e.kind}`);
  }
  return run(value);
}
export function matchesPattern(pattern,value,evaluate=v=>v){
  if(pattern?.expr==='_')return true;
  if(Array.isArray(pattern))return Array.isArray(value)&&pattern.length===value.length&&pattern.every((p,i)=>matchesPattern(p,value[i],evaluate));
  if(pattern?.comparison){const expected=evaluate(pattern.value),order=compareValues(value,expected);return order!==null&&(pattern.comparison==='<'?order<0:pattern.comparison==='<='?order<=0:pattern.comparison==='>'?order>0:order>=0);}
  return equalValues(evaluate(pattern),value);
}
export function expressionReferences(value){
  const paths=new Set();function walk(v){if(v==null||typeof v!=='object')return;if(v.expr){const path=v.expr.replace(/^!/, '');if(/^[A-Za-z_]\w*(?:\.[A-Za-z_]\w*)+$/.test(path))paths.add(path);}for(const [key,x]of Object.entries(v))if(key!=='expr')walk(x);}
  walk(value);return [...paths];
}
function quote(value){return `'${value.replace(/\\/g,'\\\\').replace(/'/g,"\\'").replace(/\n/g,'\\n').replace(/\r/g,'\\r').replace(/\t/g,'\\t').replace(/\$\{/g,'\\${')}'`;}
export function serializeValue(v){
  if(v?.expr)return v.expr;
  if(v?.interpolation)return `'${v.interpolation.map(x=>typeof x==='string'?quote(x).slice(1,-1):'${'+serializeValue(x)+'}').join('')}'`;
  if(v?.expression){const e=v.expression;if(e.kind==='binary')return `(${serializeValue(e.left)} ${e.operator} ${serializeValue(e.right)})`;if(e.kind==='unary')return `${e.operator}(${serializeValue(e.argument)})`;if(e.kind==='conditional')return `(${serializeValue(e.condition)} ? ${serializeValue(e.then)} : ${serializeValue(e.else)})`;if(e.kind==='call')return `${e.name}(${e.args.map(serializeValue).join(', ')})`;if(e.kind==='member')return `${serializeValue(e.object)}${!e.computed&&typeof e.property==='string'?(e.optional?'?.':'.')+e.property:(e.optional?'?.':'')+'['+serializeValue(e.property)+']'}`;}
  if(v&&own(v,'match'))return `match ${Array.isArray(v.match)?'('+v.match.map(serializeValue).join(', ')+')':serializeValue(v.match)} { ${v.branches.map(b=>`${Array.isArray(b.pattern)?'('+b.pattern.map(serializeValue).join(', ')+')':serializeValue(b.pattern)} => ${serializeValue(b.value)};`).join(' ')} }`;
  if(Array.isArray(v))return `[${v.map(serializeValue).join(', ')}]`;
  if(typeof v==='string')return quote(v);if(v==null||typeof v==='number'||typeof v==='boolean')return String(v);
  return `{ ${Object.entries(v).map(([k,x])=>`${k}: ${serializeValue(x)};`).join(' ')} }`;
}
