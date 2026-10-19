import {parser} from './forma-parser.js';
import {parse} from './language.js';
import {linkComponentDefinitions} from './components.js';

const words=text=>text.split(' ');
const common=words('key width height text fontSize color radius disabled background hoverBackground pressedBackground disabledBackground borderWidth borderColor focusBorderColor transitionDuration transition x y cell row column');
const properties={Frame:words('key width height padding gap columns rows clip radius background overflow transition'),Scroll:words('key width height padding gap x y'),Modal:words('key x y width height backdrop dismiss open dismissed'),Rectangle:words('key width height radius background cell row column transition'),Text:words('key text color fontSize width height cell row column transition'),TextInput:words('key value placeholder placeholderColor color fontSize multiline width height transition'),Image:words('key source color width height cell row column'),Badge:words('key text tone color background fontSize radius width height'),Tag:words('key text tone color background fontSize radius width height'),Chip:words('key text selected tone color background fontSize radius width height'),Avatar:words('key text color background fontSize radius width height'),ProgressBar:words('key value fill background radius width height'),Divider:words('key color background width height'),Separator:words('key color background width height'),ContentPresenter:words('key content'),Brush:words('color hover pressed disabled focus transition'),Border:words('width background'),Reveal:words('width color radius stop activeRadius activeStop transition targetX targetY targetWidth targetHeight targetRadius'),PointerArea:['clicked']};
const option=(label,type='property',detail='',apply)=>({label,type,detail,...(apply?{apply}:{})});
for(const type of ['Row','Column','Grid','Stack'])properties[type]=words('key width height minWidth maxWidth minHeight maxHeight padding gap clip radius cell row column row.span column.span transition'+(type==='Grid'?' columns rows':''));
function overrideTargets(files,base){
 const needed=new Set();let name=base;
 while(name&&!needed.has(name)){needed.add(name);try{name=parse(files[`components/${name}.ui`]).base;}catch{return new Map();}}
 const subset=Object.fromEntries(Object.entries(files).filter(([path])=>!/^components\/\w+\.ui$/.test(path)||needed.has(path.slice(11,-3))));
 let definition;try{definition=linkComponentDefinitions(subset).definitions[base];}catch{return new Map();}
 const targets=new Map();
 const roots=value=>value?.type?[value]:value?.branches?value.branches.flatMap(b=>roots(b.value)):[];
 const visit=value=>{if(!value||typeof value!=='object')return;
  if(value.type&&typeof value.props?.key==='string'){
   const key=value.props.key,nodes=value.type==='ContentPresenter'?roots(value.props.content??(value.children.length===1?value.children[0]:{type:'Frame',props:{},children:value.children})): [value];
   const sets=nodes.map(node=>new Set((properties[node.type]??[...common,...Object.keys(node.props)]).filter(p=>p!=='key'&&p!=='clicked')));
   targets.set(key,targets.has(key)?null:{types:[...new Set(nodes.map(n=>n.type))],properties:sets.length?[...sets[0]].filter(p=>sets.every(s=>s.has(p))):[],content:value.type==='ContentPresenter'});
  }
  for(const child of Object.values(value))visit(child);
 };visit(definition?.nodes);return new Map([...targets].filter(([,value])=>value));
}
const modulePath=path=>'crate'+(path.replace(/^src\//,'').replace(/(?:\/mod)?\.rs$/,'').replace(/^(?:lib|main)$/,'').split('/').filter(Boolean).map(p=>'::'+p).join(''));
const basename=type=>type?.replace(/\s/g,'').split('::').at(-1);
function children(node,name){const out=[];for(let n=node?.firstChild;n;n=n.nextSibling)if(!name||n.name===name)out.push(n);return out;}
function text(source,node){return node?source.slice(node.from,node.to):'';}
function ownProps(source,block){return Object.fromEntries(children(block,'Property').map(n=>[text(source,n.getChild('PropertyName')),text(source,n).replace(/^[\w.]+\s*:\s*/, '').replace(/;\s*$/,'').trim()]));}
function quoted(value){const m=value?.match(/^(['"])((?:\\.|(?!\1)[\s\S])*)\1$/);return m?m[2].replace(/\\(.)/gs,(_,c)=>c==='n'?'\n':c==='r'?'\r':c==='t'?'\t':c):undefined;}
function maskRust(source){return source.replace(/\/\/[^\n]*|\/\*[\s\S]*?\*\/|r(#+)?"[\s\S]*?"\1|"(?:\\.|[^"\\])*"|'(?:\\.|[^'\\])'/g,m=>m.replace(/[^\n]/g,' '));}
function closing(source,start,open='{',close='}') {let depth=0;for(let i=start;i<source.length;i++){if(source[i]===open)depth++;else if(source[i]===close&&!--depth)return i;}return source.length;}

// A conservative source index, not a Rust compiler. Ambiguous type names are
// excluded instead of guessing a model. No project code is executed for hints.
export function indexProject(files){
 const components=new Map(),models=new Map(),qualifiedModels=new Map(),types=[],forms=new Map();
 for(const [path,source]of Object.entries(files)){
  if(path.endsWith('.ui')){try{const doc=parse(source);if(path.startsWith('components/'))components.set(doc.name,doc);if(doc.defaults.contextType)forms.set(doc.name,doc);}catch{} }
  if(!path.endsWith('.rs'))continue;
  const clean=maskRust(source);
  for(const match of clean.matchAll(/\bstruct\s+(\w+)\s*\{/g)){
   const start=match.index+match[0].length,end=closing(clean,start-1),fields=[];
   const body=clean.slice(start,end);
   for(const field of body.matchAll(/(?:^|,)\s*(?:pub(?:\([^)]*\))?\s+)?(\w+)\s*:\s*((?:[\w]+::)*(?:Property|Context))\s*</g)){
    const from=field.index+field[0].length,until=closing(body,from-1,'<','>'),kind=basename(field[2]);
    fields.push({name:field[1],type:body.slice(from,until).trim(),kind});
   }
   const name=match[1],qualified=modulePath(path)+'::'+name,model={name,fields,methods:[],qualified};types.push(qualified);qualifiedModels.set(qualified,qualifiedModels.has(qualified)?null:model);models.set(name,models.has(name)?null:model);
  }
 }
 for(const [path,original] of Object.entries(files).filter(([p])=>p.endsWith('.rs'))){
  const source=maskRust(original);
  for(const match of source.matchAll(/\bimpl\s+(?:[\w]+::)*(\w+)\s*\{/g)){
   const model=qualifiedModels.get(modulePath(path)+'::'+match[1]);if(!model)continue;const start=match.index+match[0].length,body=source.slice(start,closing(source,start-1));
   for(const method of body.matchAll(/\b(?:pub(?:\([^)]*\))?\s+)?fn\s+(\w+)\s*\(\s*&\s*self\s*,?\s*\)/g))model.methods.push(method[1]);
  }
 }
 function defaults(name,seen=new Set()){if(seen.has(name))return {};seen.add(name);const doc=components.get(name);return doc?{...defaults(doc.base,seen),...doc.defaults}:{};}
 const targetCache=new Map();
 return {components,models,qualifiedModels,types:[...new Set(types)],forms,defaults,overrideTargets(base){if(!targetCache.has(base))targetCache.set(base,overrideTargets(files,base));return targetCache.get(base);}};
}
function modelAt(index,type,path){let model=index.qualifiedModels.get(type)??index.models.get(basename(type));for(const key of path){const field=model?.fields.find(f=>f.name===key&&f.kind==='Context');model=index.qualifiedModels.get(field?.type)??index.qualifiedModels.get(model?.qualified.replace(/::\w+$/,'::'+field?.type))??index.models.get(basename(field?.type));}return model;}
function outsideCode(source,pos){
 // Lexer state survives incomplete comments and strings while the user types.
 const re=/\/\/[^\n]*(?:\n|$)|\/\*[\s\S]*?(?:\*\/|$)|'(?:\\.|[^'\\])*(?:'|$)|"(?:\\.|[^"\\])*(?:"|$)/g;
 for(const match of source.matchAll(re)){if(pos>match.index&&pos<=match.index+match[0].length){const s=match[0];if(s.startsWith('//')){if(s.endsWith('\n')&&pos===match.index+s.length)return null;return 'comment';}if(s.startsWith('/*'))return pos===match.index+s.length&&s.endsWith('*/')?null:'comment';return 'string';}}
 return null;
}

const stdlibFunctions=[
 ['min','min(a, b)','Минимум из чисел'],['max','max(a, b)','Максимум из чисел'],['clamp','clamp(val, min, max)','Ограничение диапазона'],
 ['abs','abs(n)','Модуль числа'],['round','round(n)','Округление'],['floor','floor(n)','Округление вниз'],['ceil','ceil(n)','Округление вверх'],
 ['pow','pow(base, exp)','Возведение в степень'],['sqrt','sqrt(n)','Квадратный корень'],['sign','sign(n)','Знак числа (-1, 0, 1)'],
 ['lerp','lerp(a, b, t)','Линейная интерполяция'],['remap','remap(v, inMin, inMax, outMin, outMax)','Переотображение диапазона'],['avg','avg(list)','Среднее значение'],
 ['len','len(x)','Длина строки, списка или словаря'],['count','count(list)','Количество элементов'],['sum','sum(list)','Сумма чисел'],['range','range(start, end)','Числовой диапазон'],
 ['trim','trim(s)','Обрезка пробелов'],['upper','upper(s)','Верхний регистр'],['lower','lower(s)','Нижний регистр'],['capitalize','capitalize(s)','Первая буква заглавная'],
 ['padStart','padStart(s, len, fill)','Дополнение слева'],['padEnd','padEnd(s, len, fill)','Дополнение справа'],['truncate','truncate(s, maxLen)','Обрезка строки с многоточием'],['repeat','repeat(s, count)','Повтор строки'],
 ['contains','contains(x, item)','Проверка вхождения'],['includes','includes(x, item)','Проверка вхождения'],['indexOf','indexOf(x, item)','Индекс элемента или подстроки'],
 ['startsWith','startsWith(s, prefix)','Проверка префикса'],['endsWith','endsWith(s, suffix)','Проверка суффикса'],['replace','replace(s, from, to)','Замена подстроки'],
 ['slice','slice(x, start, end)','Срез строки или списка'],['split','split(s, sep)','Разбиение строки'],['join','join(list, sep)','Склейка списка в строку'],
 ['reverse','reverse(x)','Разворот списка или строки'],['take','take(list, n)','Первые N элементов'],['skip','skip(list, n)','Пропуск первых N элементов'],
 ['unique','unique(list)','Уникальные элементы'],['find','find(list, key, val)','Поиск объекта по полю'],['compact','compact(list)','Удаление пустых значений'],
 ['flatten','flatten(list)','Выпрямление вложенных списков'],['chunk','chunk(list, size)','Разбиение на группы'],['zip','zip(a, b)','Объединение двух списков попарно'],
 ['first','first(list)','Первый элемент'],['last','last(list)','Последний элемент'],['groupBy','groupBy(list, key)','Группировка по полю'],
 ['keys','keys(obj)','Ключи словаря'],['values','values(obj)','Значения словаря'],['entries','entries(obj)','Пары [ключ, значение]'],['fromEntries','fromEntries(pairs)','Словарь из пар'],
 ['merge','merge(a, b)','Слияние словарей'],['pick','pick(obj, keys)','Выборка ключей словаря'],['omit','omit(obj, keys)','Исключение ключей словаря'],
 ['isEmpty','isEmpty(x)','Проверка на пустоту'],['coalesce','coalesce(a, b)','Первое не-null значение'],['formatNumber','formatNumber(n, decimals)','Форматирование числа'],
 ['plural','plural(n, one, few, many)','Склонение по числу'],['isValid','isValid(val, rule)','Проверка ограничения/шаблона'],
 ['alpha','alpha(color, pct)','Прозрачность цвета'],['mix','mix(c1, c2, pct)','Смешивание цветов'],['lighten','lighten(color, pct)','Осветление цвета'],['darken','darken(color, pct)','Затемнение цвета'],
 ['saturate','saturate(color, pct)','Насыщенность цвета'],['desaturate','desaturate(color, pct)','Обесцвечивание'],['invert','invert(color)','Инверсия цвета'],['contrast','contrast(c1, c2)','Контрастность WCAG'],
 ['String','String(x)','Преобразование в строку'],['Number','Number(x)','Преобразование в число'],['Bool','Bool(x)','Преобразование в Bool']
];
const builtinTypes=['String','Bool','Number','Int','Float','Color','Length','Duration','Angle','Asset','Any','List','Map','Record'];

export function completeCode({source,pos,path,files,index=indexProject({...files,[path]:source}),explicit=false}){
 if(!path?.endsWith('.ui'))return path?.endsWith('.rs')?completeRust(source,pos,index,explicit):null;
 const before=source.slice(0,pos),tree=parser.parse(source),leaf=tree.resolveInner(pos,-1),ancestors=[];
 for(let node=leaf;node;node=node.parent)ancestors.push(node);
 const blocks=ancestors.filter(n=>n.name==='Block').reverse();
 const block=blocks.at(-1),owner=block?.parent;
 let nodeType=text(source,owner?.getChild('TypeName'));
 const component=ancestors.find(n=>n.name==='Component');
 const componentName=text(source,component?.getChild('ComponentName'));
 let modelType;for(const b of blocks){const p=ownProps(source,b);if(p.contextType)modelType=quoted(p.contextType);}
 const currentProperty=ancestors.find(n=>n.name==='Property');
 const statement=currentProperty?source.slice(currentProperty.from,pos):before.slice(Math.max(before.lastIndexOf(';'),before.lastIndexOf('{'))+1);
 const property=statement.match(/^\s*([\w.]+)\s*(?::|<->|->)/)?.[1];
 const contextType=statement.match(/^\s*contextType\s*:\s*['"]([^'"]*)$/);
 const lexical=outsideCode(source,pos);
 if(lexical==='comment')return null;
 const attrMatch=before.match(/#\[\s*(\w*)$/);
 if(attrMatch&&lexical!=='string'){
  return {from:pos-attrMatch[1].length,options:[
   option('range','keyword','Диапазон #[range(min, max)]','range(0, 100)'),
   option('pattern','keyword','Шаблон #[pattern(\'email\')]',"pattern('email')"),
   option('use','keyword','Импорт файла #[use(\'./theme.ui\')]',"use('./theme.ui')"),
   option('design','keyword','Файл дизайн-состояний',"design('./Main.design.ui')")
  ],validFor:/^\w*$/};
 }
 const applyMatch=statement.match(/^\s*apply\s+(?:[\w]+\s*,\s*)*(\w*)$/);
 if(applyMatch&&lexical!=='string'){
  const styles=new Set();
  for(const decl of children(component?.getChild('Block'),'StyleDeclaration')){
   const n=text(source,decl.getChild('PropertyName')??decl.getChild('TypeName'));if(n)styles.add(n);
  }
  for(const [,s]of Object.entries(files))if(typeof s==='string')for(const m of s.matchAll(/\bstyle\s+([A-Za-z_]\w*)\s*\{/g))styles.add(m[1]);
  return {from:pos-applyMatch[1].length,options:[...styles].map(name=>option(name,'class','Пресет стиля')),validFor:/^\w*$/};
 }
 const tailStatement=before.slice(Math.max(before.lastIndexOf(';'),before.lastIndexOf('{'),before.lastIndexOf('}'))+1);
 const typeDeclMatch=tailStatement.match(/^\s*(?:required\s+)?(?:prop|state)\s+\w+\s*:\s*(?:[\w<>?]+\s*\|\s*)*(\w*)$/);
 if(typeDeclMatch&&lexical!=='string'){
  const customTypes=new Set();
  for(const decl of [...children(component?.getChild('Block'),'EnumDeclaration'),...children(component?.getChild('Block'),'StructDeclaration')]){
   const n=text(source,decl.getChild('TypeName'));if(n)customTypes.add(n);
  }
  return {from:pos-typeDeclMatch[1].length,options:[
   ...builtinTypes.map(t=>option(t,'type',t==='List'?'List<T>':t==='Record'?'Record<K, V>':t==='Map'?'Map<T>':'Встроенный тип Forma')),
   ...[...customTypes].map(t=>option(t,'type','Пользовательский тип'))
  ],validFor:/^\w*$/};
 }
 const pipeMatch=before.match(/\|>\s*(\w*)$/);
 if(pipeMatch&&lexical!=='string'){
  return {from:pos-pipeMatch[1].length,options:stdlibFunctions.map(([name,sig,desc])=>option(name,'function',`${sig} · ${desc}`,name+(source.slice(pos).trimStart().startsWith('(')?'':'()'))),validFor:/^\w*$/};
 }
 const base=text(source,children(component,'ComponentName')[1]);
 const header=before.match(/\boverride\s+(['"]?)([\w-]*)$/);
 if(header&&!currentProperty){
  const targets=index.overrideTargets(base),quote=header[1],suffix=source.slice(pos+(quote&&source[pos]===quote?1:0));
  return {from:pos-header[2].length-quote.length,to:pos+(quote&&source[pos]===quote?1:0),options:[...targets].map(([key,target])=>{
   const name=quote||!/^[A-Za-z_]\w*$/.test(key)?`${quote||"'"}${key}${quote||"'"}`:key;
   return option(key,'property',`${target.content?'Содержимое → ':''}${target.types.join(' | ')}`,name+(/^\s*[{:]|^\s+from\b/.test(suffix)?'':' {\n    \n}'));
  }),validFor:/^[\w'"-]*$/};
 }
 const patchTarget=owner?.name==='Override'?index.overrideTargets(base).get(text(source,owner.getChild('PropertyName'))||quoted(text(source,owner.getChild('String')))):null;
 if(patchTarget)nodeType=patchTarget.types[0];
 if(contextType){const prefix=contextType[1];return {from:pos-prefix.length,options:index.types.filter(name=>index.qualifiedModels.get(name)).map(name=>option(name,'type','Rust ViewModel')),validFor:/^[\w:]*$/};}
 if(lexical==='string')return null;
 const token=before.match(/[\w.]*$/)?.[0]??'',from=pos-token.length;
 if(!explicit&&!token&&!/[.:>]\s*$/.test(before))return null;
 const qualified=token.match(/^(state|props|events|tokens|i18n|viewport)\.(.*)$/);
 if(qualified){
  const [,root,rest]=qualified,parts=rest.split('.');parts.pop();const prefix=root+'.'+(parts.length?parts.join('.')+'.':'');let options=[];
  if(root==='state'){
   // A context assignment is resolved in the parent scope, before its override.
   if(property==='context'){modelType=undefined;for(const b of blocks.slice(0,-1)){const p=ownProps(source,b);if(p.contextType)modelType=quoted(p.contextType);}}
   const model=modelAt(index,modelType,parts),command=/(?<!<)->/.test(statement);
   const localStates=[];
   if(!parts.length){
    for(const decl of children(component?.getChild('Block'),'StateDeclaration')){
     const n=text(source,decl.getChild('PropertyName'));if(n)localStates.push(n);
    }
   }
   if(command){
    options=[
     ...(model?.methods??[]).map(name=>option(prefix+name,'function',`${model.name}::${name}()`,prefix+name+(source.slice(pos).trimStart().startsWith('(')?'':'()'))),
     ...(!model&&!parts.length?[
      ...localStates.map(name=>option('state.'+name,'function','Локальное состояние','state.'+name+(source.slice(pos).trimStart().startsWith('(')?'':'()'))),
      option('state.toggle','function',"state.toggle('field')"),
      option('state.set','function',"state.set('field', value)"),
      option('state.increment','function',"state.increment('field', 1)"),
      option('state.decrement','function',"state.decrement('field', 1)"),
      option('state.push','function',"state.push('list', item)"),
      option('state.remove','function',"state.remove('list', index)"),
      option('state.reset','function','Сбросить локальное состояние','state.reset()')
     ]:[])
    ];
   }else{
    options=[
     ...(model?.fields??[]).filter(f=>property!=='context'||f.kind==='Context').map(f=>option(prefix+f.name,'property',`${f.kind}<${f.type}>`)),
     ...(property!=='context'?localStates.map(name=>option('state.'+name,'property','Локальное состояние')):'')
    ];
   }
  }else if(root==='props'){
   const declared=ownProps(source,component?.getChild('Block'));
   for(const decl of children(component?.getChild('Block'),'PropDeclaration')){const n=text(source,decl.getChild('PropertyName'));if(n)declared[n]=true;}
   const base=children(component,'ComponentName')[1];
   options=Object.keys({...index.defaults(text(source,base)),...index.defaults(componentName),...declared}).filter(k=>!['contextType','events'].includes(k)).map(k=>option('props.'+k,'property','Свойство компонента'));
  }else if(root==='tokens'){
   const tokenNames=new Set();
   for(const decl of children(component?.getChild('Block'),'TokensDeclaration'))for(const p of children(decl,'PropertyName'))tokenNames.add(text(source,p));
   options=[...tokenNames].map(k=>option('tokens.'+k,'property','Дизайн-токен'));
   return {from,options,validFor:/^tokens\.[\w.]*$/};
  }else if(root==='i18n'){
   const i18nNames=new Set();
   for(const decl of children(component?.getChild('Block'),'I18nDeclaration'))for(const p of children(decl,'PropertyName'))i18nNames.add(text(source,p));
   options=[...i18nNames].map(k=>option('i18n.'+k,'property','Строка локализации'));
   return {from,options,validFor:/^i18n\.[\w.]*$/};
  }else if(root==='viewport'){
   options=[option('viewport.width','property','Ширина экрана (Number)'),option('viewport.height','property','Высота экрана (Number)'),option('viewport.compact','property','Компактный режим (< 640px)')];
   return {from,options,validFor:/^viewport\.[\w.]*$/};
  }else{
   const declaration=ownProps(source,component?.getChild('Block')).events??(path.startsWith('components/')?"['clicked']":'');
   const evNames=new Set([...declaration.matchAll(/['"](\w+)['"]/g)].map(m=>m[1]));
   for(const decl of children(component?.getChild('Block'),'EventDeclaration')){const n=text(source,decl.getChild('PropertyName'));if(n)evNames.add(n);}
   options=[...evNames].map(name=>option('events.'+name,'function','Событие формы','events.'+name+(source.slice(pos).trimStart().startsWith('(')?'':'()')));
  }
  return {from,options,validFor:new RegExp('^'+prefix.replace(/\./g,'\\.')+'\\w*$')};
 }
 let options=[];
 if(/->\s*\w*$/.test(statement))options=[option('events.','namespace','События формы'),option('state.','namespace','Команды ViewModel / состояния'),option('actions.','namespace','Действия превью')];
 else if(property){
  options=[option('state.','namespace','Текущий контекст'),option('props.','namespace','Параметры компонента'),option('tokens.','namespace','Дизайн-токены'),option('i18n.','namespace','Локализация'),option('viewport.','namespace','Параметры контейнера')];
  const value=index.defaults(nodeType)[property];
  if(typeof value==='boolean'||['disabled','checked','multiline','clip','indeterminate'].includes(property))options.push(option('true','keyword'),option('false','keyword'),option('!state.','namespace','Отрицание'));
  if(statement.includes('<->'))options=[option('state.','namespace','Наблюдаемое поле')];
  else{
   options.push(option('match','keyword','Выбор по значению'));
   for(const decl of children(component?.getChild('Block'),'LetDeclaration')){const n=text(source,decl.getChild('PropertyName'));if(n)options.push(option(n,'variable','Вычисляемое свойство let'));}
   for(const [name,sig,desc]of stdlibFunctions)options.push(option(name,'function',`${sig} · ${desc}`,name+'('));
  }
  if(property==='background')options.push(option('Brush','class','Кисть с состояниями','Brush { color: #ffffff; }'));
 }else{
  const declared=ownProps(source,block),root=owner?.name==='Component';
  if(owner?.name==='Override')return {from,options:(patchTarget?.properties??[]).filter(name=>!Object.hasOwn(declared,name)||name===token).map(name=>option(name,'property',patchTarget.types.join(' | '),/^\s*:/.test(source.slice(pos))?name:name+': ')),validFor:/^[\w.]*$/};
  const names=root?['contextType','events']:properties[nodeType]??[...common,...Object.keys(index.defaults(nodeType))];
  options=[...new Set([...names,...(!root&&!['Brush','Border','Reveal'].includes(nodeType)?['contextType','context']:[])])].filter(name=>!Object.hasOwn(declared,name)||name===token).map(name=>option(name,'property',name==='contextType'?'Тип ViewModel':name==='context'?'Контекст элемента':nodeType||'Форма',/^\s*(?::|<->|->)/.test(source.slice(pos))?name:name==='clicked'?'clicked -> events.':name+': '));
  if(root)options.push(option('prop','keyword','Входной параметр','prop '),option('required prop','keyword','Обязательный параметр','required prop '),option('state','keyword','Локальное состояние','state '),option('style','keyword','Именованный пресет стиля','style '),option('i18n','keyword','Словарь локализации','i18n {\n    \n}'),option('let','keyword','Вычисляемое свойство','let '),option('use','keyword','Импорт токенов и типов',"use './theme.ui';"),option('tokens','keyword','Дизайн-токены','tokens {\n    \n}'),option('struct','keyword','Структура данных','struct '),option('enum','keyword','Перечисление','enum '),option('event','keyword','Типизированное событие','event '),option('slot','keyword','Именованный слот','slot '),option('shortcut','keyword','Горячая клавиша',"shortcut 'CmdOrCtrl+K' -> actions."));
  if(!root)options.push(option('apply','keyword','Применить стиль','apply '),option('let','keyword','Вычисляемое свойство','let '),option('match','keyword','Условная разметка / свойства','match '));
  if(!root&&!properties[nodeType])options.push(option('clicked','event','Активация','clicked -> events.'),option('changed','event','Изменение значения','changed -> events.'),option('submitted','event','Отправка (Enter)','submitted -> events.'));
  if(['TextField','TextInput','TextArea','Slider'].includes(nodeType))options.push(option('value <->','property','Двусторонняя привязка','value <-> state.'),option('submitted','event','Отправка (Enter)','submitted -> actions.'));
  if(['Checkbox','Switch','RadioButton'].includes(nodeType))options.push(option('checked <->','property','Двусторонняя привязка','checked <-> state.'));
  if(!['Brush','Border','Reveal','PointerArea'].includes(nodeType))options.push(...[...new Set([...Object.keys(properties),...index.components.keys()])].map(name=>option(name,'class','Элемент Forma',name+' {\n    \n}')));
  if(!block)options=[option('component','keyword','Объявление формы','component Form {\n    \n}')];
 }
 return {from,options,validFor:/^\w*$/};
}

function completeRust(source,pos,index,explicit){
 const clean=maskRust(source),before=clean.slice(0,pos),last=source.slice(0,pos).search(/\S\s*$/);if(last>=0&&clean[last]===' '&&source[last]!==' ')return null;
 const token=before.match(/[\w:]*$/)?.[0]??'';if(!token&&!explicit)return null;
 const options=[...index.types,...index.forms.keys()].map(name=>option(name,'type','Тип проекта'));
 options.push(option('Property::new','function','Наблюдаемое поле','Property::new()'),option('Context::new','function','Контекст ViewModel','Context::new()'),option('Context::empty','function','Пустой контекст','Context::empty()'));
 for(const [name,form]of index.forms)options.push(option(name+'::new','function',`Форма · ${form.defaults.contextType}`,name+'::new()'));
 return {from:pos-token.length,options,validFor:/^[\w:]*$/};
}

export function createCompletionSource(getProject){
 let cachedFiles=null,cachedIndex=null;
 return context=>{
  const {files,path}=getProject(),source=context.state.doc.toString();
  // File objects in Studio are mutable. Compare contents, not their identity.
  const entries=Object.entries(files);
  if(!cachedFiles||entries.length!==cachedFiles.length||entries.some(([p,s],i)=>cachedFiles[i][0]!==p||cachedFiles[i][1]!==s)){cachedIndex=indexProject(files);cachedFiles=entries;}
  return completeCode({source,pos:context.pos,path,files,index:cachedIndex,explicit:context.explicit});
 };
}
