// Typed Rust form generation shares the Studio parser and component linker.
// The output is a build artifact, never an application source file to edit.
import {parse} from './language.js';
import {linkComponentDefinitions} from './components.js';
import {svgShapes} from './svg-shapes.js';
import {expressionReferences} from './expressions.js';

const identifier=/^[A-Za-z_][A-Za-z0-9_]*$/;
const rustPath=/^(?:[A-Za-z_][A-Za-z0-9_]*::)*[A-Za-z_][A-Za-z0-9_]*$/;
const reserved=new Set('as break const continue crate else enum extern false fn for if impl in let loop match mod move mut pub ref return self Self static struct super trait true type unsafe use where while async await dyn abstract become box do final macro override priv typeof unsized virtual yield try gen'.split(' '));
const snake=name=>name.replace(/([A-Z]+)([A-Z][a-z])/g,'$1_$2').replace(/([a-z0-9])([A-Z])/g,'$1_$2').toLowerCase();
function rustString(value){let hashes='#';while(value.includes('"'+hashes))hashes+='#';return `r${hashes}"${value}"${hashes}`;}

export function generateForm(files,entry,{state={},measureText}={}) {
 if(/[\r\n]/.test(entry))throw Error('Путь формы не должен содержать перевод строки');
 const source=files[entry];if(typeof source!=='string')throw Error(`Форма не найдена: ${entry}`);
 let doc;try{doc=parse(source);}catch(error){throw Error(`${entry}: ${error.message}`);}
 const fail=(message,offset=0)=>{throw Error(`${entry}:${source.slice(0,offset).split('\n').length}: ${message}`);};
 const validName=(name,offset)=>{if(!identifier.test(name??'')||reserved.has(name))fail(`Недопустимое Rust-имя «${name}»`,offset);return name;};
 const typeName=(type,offset)=>{if(typeof type!=='string'||!rustPath.test(type))fail('contextType ожидает Rust-тип в строке, например \'crate::CustomerVm\'',offset);return type;};
 const rootType=typeName(doc.defaults.contextType,doc.defaultRanges.contextType?.from);
 validName(doc.name,0);
 if(doc.base||doc.designs.length||Object.keys(doc.slots).length)fail('Генерируемая форма не наследуется от компонента и не использует design/override на корне');
 for(const key of Object.keys(doc.defaults))if(!['contextType','events',...Object.keys(doc.propDefinitions??{})].includes(key))fail(`Неизвестное объявление формы ${key}`,doc.defaultRanges[key].from);
 const declared=[...(doc.defaults.events??[]),...Object.keys(doc.eventDefinitions??{})];
 if(!Array.isArray(declared)||declared.some(name=>typeof name!=='string'))fail('events ожидает список имён: [\'saveRequested\']',doc.defaultRanges.events?.from);
 const eventTypes=new Map();const usedNames=new Set(['new','with_properties','context','set_context','clear_context','sync','update','runtime','is_dirty','on_dirty']);
 for(const name of declared){validName(name,0);if(eventTypes.has(name))fail(`Повторное событие ${name}`);eventTypes.set(name,{type:null,controls:[],args:doc.eventDefinitions?.[name]??[]});const method='on_'+snake(name);if(usedNames.has(method))fail(`Конфликт Rust-метода ${method}`);usedNames.add(method);}
 const layoutTypes=new Set(['Frame','Scroll','Row','Column','Grid','Stack']);
 const statements=[],fields=[],methods=[],controls=[],eventStructs=[];let serial=0,nodeSerial=0;
 const linked=linkComponentDefinitions(files);const usedDefinitions=new Set();
 const collectionBlueprints=new Map();
 function collectBlueprints(node){if(node.type==='For'&&node.collection?.expr?.startsWith('state.'))collectionBlueprints.set(node.collection.expr,node);for(const child of [...node.children??[],...node.elseChildren??[],...node.emptyChildren??[]])collectBlueprints(child);}
 doc.nodes.forEach(collectBlueprints);
 const path=(expr,offset)=>{
   if(typeof expr!=='string'||!expr.startsWith('state.'))fail('Ожидается путь state.field',offset);
   return expr.slice(6).split('.').map(name=>validName(name,offset));
 };
 const project=(parent,segments)=>{let current=parent;for(const field of segments){const next=`scope_${serial++}`;statements.push(`let ${next} = ::forma::binding::Context::project(&${current}, |vm| vm.${field}.clone());`);current=next;}return current;};
 function scope(node,parent,parentType){
   let current,type=parentType;
   if(node.props.context!==undefined){
     type=typeName(node.props.contextType,node.statementRanges.context?.from);
     current=project(parent,path(node.props.context?.expr,node.statementRanges.context.from));
     statements.push(`let ${current}: ::forma::binding::Context<${type}> = ${current};`);

   }else{
     if(node.props.contextType!==undefined)fail('contextType у узла требует context',node.start);
     current=`scope_${serial++}`;statements.push(`let ${current} = ::forma::binding::Context::child(&${parent});`);
   }
   return [current,type];
 }
 function collectionProjection(loop,collection){
   const references=expressionReferences({key:loop.key,children:loop.children}).filter(ref=>ref.startsWith(loop.item+'.'));
   const nested=new Map();let bare=false;
   function scan(value){
     if(!value||typeof value!=='object')return;
     if(value.expr===loop.item)bare=true;
     if(value.type==='For'&&value!==loop&&value.collection?.expr?.startsWith(loop.item+'.'))nested.set(value.collection.expr.slice(loop.item.length+1),value);
     for(const expr of Object.values(value.bindings??{}))if(expr.startsWith(loop.item+'.'))references.push(expr);
     for(const child of Object.values(value))scan(child);
   }
   scan({key:loop.key,children:loop.children});
   const projection=(root,paths,prefix='')=>{
     const groups=new Map();for(const [head,...tail] of paths){if(!groups.has(head))groups.set(head,[]);groups.get(head).push(tail);}
     return `::forma::form_document::Value::Object(::std::collections::BTreeMap::from([${[...groups].map(([field,tails])=>{
       validName(field,loop.start);const part=prefix+field,child=nested.get(part);
       const value=child?collectionProjection(child,root+'.'+field):tails.some(t=>!t.length)?`::forma::binding::ModelValue::to_value(&${root}.${field})`:projection(root+'.'+field,tails,part+'.');
       return `(${rustString(field)}.into(),${value})`;
     }).join(',')}]))`;
   };
   const item=bare?`::forma::binding::ModelValue::to_value(${loop.item})`:projection(loop.item,references.map(ref=>ref.slice(loop.item.length+1).split('.')));
   return `::forma::form_document::Value::List(${collection}.iter().map(|${loop.item}| ${item}).collect())`;
 }
 function walk(node,parent,parentType,loops=new Map()){
   if(Object.keys(node.slots).length)fail('override объявляется в наследнике component',node.start);
   const nodeIndex=nodeSerial++;node.__formIndex=nodeIndex;
   const [current,type]=scope(node,parent,parentType);
   if(node.type==='If'||node.type==='For'){
     if(node.type==='For'&&loops.has(node.item))fail('Имя вложенного for перекрывает внешнее: '+node.item,node.start);
     const deps=expressionReferences(node.type==='If'?node.condition:node.collection).filter(p=>p.startsWith('state.'));
     for(const expr of deps){
       const segments=path(expr,node.start),field=segments.pop(),owner=project(current,segments);
       if(node.type==='For'&&expr===node.collection?.expr){
         statements.push(`inner.observe_projected(${nodeIndex}, ${rustString(expr.slice(6))}, &${owner}, |vm| vm.${field}.clone(), |items| ${collectionProjection(node,'items')});`);
         loops=new Map(loops);loops.set(node.item,{owner,field,indices:['__index.'+node.item],access:[]});
       }else statements.push(`inner.observe(${nodeIndex}, ${rustString(expr.slice(6))}, &${owner}, |vm| vm.${field}.clone());`);
     }
     if(node.type==='For'&&!loops.has(node.item)){
       const segments=node.collection?.expr?.split('.')??[],parentLoop=loops.get(segments[0]);
       if(!parentLoop||segments.length<2)fail('Коллекция for требует путь state.collection или поле внешнего элемента',node.start);
       const access=segments.slice(1).map(name=>validName(name,node.start)).join('.');
       loops=new Map(loops);loops.set(node.item,{...parentLoop,indices:[...parentLoop.indices,'__index.'+node.item],access:[...parentLoop.access,access]});
     }
     for(const child of node.children)walk(child,current,type,loops);
     for(const child of node.elseChildren??node.emptyChildren??[])walk(child,current,type,loops);
     return;
   }
   const definition=linked.definitions[node.type];
   const allowed=new Set(['key','context','contextType','row.span','column.span',...(layoutTypes.has(node.type)?'x y width height padding gap background columns rows clip radius overflow layout cell row column minWidth maxWidth minHeight maxHeight align justify':'x y width height text fontSize font.size color radius disabled background hoverBackground pressedBackground disabledBackground borderWidth borderColor focusBorderColor transitionDuration cell row column minWidth maxWidth minHeight maxHeight').split(' '),...Object.keys(definition?.defaults??{}),...Object.keys(definition?.propDefinitions??{}),...(definition?.matches??[]).flatMap(m=>m.branches.flatMap(b=>Object.keys(b.props??{})))]);
   if(!definition&&!layoutTypes.has(node.type))fail('Неизвестный компонент '+node.type,node.start);
   for(const key of [...Object.keys(node.props),...Object.keys(node.bindings)])if(!allowed.has(key))fail(`Неизвестное свойство ${node.type}.${key}`,node.statementRanges[key]?.from??node.start);
   const dependencies=new Set();
   function collect(value){for(const ref of expressionReferences(value))if(ref.startsWith('state.'))dependencies.add(ref);}

   collect(Object.fromEntries(Object.entries(node.props).filter(([key])=>!['context','contextType'].includes(key))));collect(node.matches);collect(node.eventArgs);if(nodeIndex===0){collect(Object.fromEntries(Object.entries(doc.defaults).filter(([k])=>!['contextType','events'].includes(k))));collect(doc.matches);}
   function collectDefinition(name,seen=new Set()) { if(seen.has(name))return;seen.add(name);const definition=linked.definitions[name];if(!definition)return;usedDefinitions.add(name);collect(definition);function nested(v){if(v?.type)collectDefinition(v.type,seen);if(v&&typeof v==='object')Object.values(v).forEach(nested);}nested(definition); }
   collectDefinition(node.type);
   if(!layoutTypes.has(node.type)){
     collect(node.children);
     function supplied(value){if(value?.type)collectDefinition(value.type);if(value&&typeof value==='object')for(const [key,item]of Object.entries(value))if(!['source','propertySources'].includes(key))supplied(item);}
     supplied(node.props);supplied(node.children);
   }
   for(const expr of dependencies){
     let staticValue=state;for(const part of expr.slice(6).split('.'))staticValue=staticValue?.[part];
     if(staticValue!==undefined)continue;
     const segments=path(expr,node.start),field=segments.pop(),owner=project(current,segments);
     const blueprint=collectionBlueprints.get(expr);
     if(blueprint){statements.push(`inner.observe_projected(${nodeIndex}, ${rustString(expr.slice(6))}, &${owner}, |vm| vm.${field}.clone(), |items| ${collectionProjection(blueprint,'items')});`);continue;}
     const property=Object.entries(node.props).find(([,value])=>value?.expr?.replace(/^!/, '')===expr)?.[0];
     statements.push(`// @forma-source ${entry}:${source.slice(0,node.statementRanges[property]?.from??node.start).split('\n').length}`);
     const sample=definition?.defaults[property]??linked.fallback[property];
     const contract=definition?.propDefinitions?.[property]?.type;
     const observe=contract?.endsWith('?')?'observe':contract==='Number'||typeof sample==='number'||['x','y','padding','gap'].includes(property)?'observe_number':contract==='Bool'||typeof sample==='boolean'?'observe_bool':contract==='String'||typeof sample==='string'?'observe_text':'observe';
     statements.push(`inner.${observe}(${nodeIndex}, ${rustString(expr.slice(6))}, &${owner}, |vm| vm.${field}.clone());`);
   }
   if(layoutTypes.has(node.type)){
     if(Object.keys(node.bindings).length||Object.keys(node.events).length)fail('Контейнер не имеет событий ввода и двусторонних свойств',node.start);
     if(node.props.key!==undefined){const method=validName(snake(node.props.key),node.start);if(usedNames.has(method))fail(`Конфликт Rust-метода ${method}`,node.start);usedNames.add(method);const field=`container_${nodeIndex}`;fields.push(`${field}: ::forma::binding::Context<${type}>`);controls.push(field);statements.push(`let ${field} = ${current}.clone();`);methods.push(`pub fn ${method}(&self) -> ::forma::binding::Context<${type}> { self.${field}.clone() }`);}
     node.children.forEach(child=>walk(child,current,type,loops));return;
   }
   const index=fields.filter(f=>f.includes("binding::Control<")).length,variable=`control_${index}`;controls.push(variable);
   statements.push(`let ${variable} = inner.add_control(${index}, ${current}.clone())?;`);
   statements.push(`inner.bind_node(${index}, ${nodeIndex});`);
   if(node.props.key!==undefined){
     validName(node.props.key,node.start);const method=snake(node.props.key);validName(method,node.start);
     if(usedNames.has(method))fail(`Конфликт Rust-метода ${method}`,node.start);usedNames.add(method);
     fields.push(`${variable}: ::forma::binding::Control<${type}>`);
     methods.push(`pub fn ${method}(&self) -> ::forma::binding::Control<${type}> { self.${variable}.clone() }`);
   }else fields.push(`${variable}: ::forma::binding::Control<${type}>`);
   const bindings=new Map(Object.entries(node.bindings).map(([key,expr])=>[key,{expr,twoWay:true}]));
   for(const [key,value]of Object.entries(node.props))if(key!=='context'&&value?.expr?.includes('state.'))bindings.set(key,{expr:value.expr,twoWay:false});
   for(const [key,{expr,twoWay}]of bindings){
     const offset=node.statementRanges[key].from;
     if(twoWay&&!['value','checked','selected'].includes(key))fail(`Не поддерживается двусторонняя привязка ${key}`,offset);
     const local=loops.get(expr.split('.')[0]);
     if(twoWay&&local){
       const parts=expr.split('.').slice(1).map(name=>validName(name,offset));
       if(!parts.length)fail('Привязка элемента коллекции требует поле',offset);
       statements.push(`${variable}.bind_collection_context(${rustString(key)}, &[${local.indices.map(rustString).join(',')}], ${rustString(expr)}, &${local.owner}, |vm| vm.${local.field}.clone(), |item,_indices,value| { ${local.access.map((field,index)=>`let item=item.${field}.get_mut(_indices[${index+1}]).ok_or("Nested collection item was removed")?;`).join(' ')} item.${parts.join('.')} = ::forma::binding::decode(value)?; Ok(()) });`);
       continue;
     }
     if(twoWay&&!expr.startsWith('state.'))fail('Неизвестная область двусторонней привязки '+expr,offset);
     if(!['value','text','disabled'].includes(key)&&!(['checked','selected'].includes(key)&&twoWay)||!expr.startsWith('state.'))continue;
     const method=key==='value'&&(node.type==='Slider'||typeof definition?.defaults.value==='number'||definition?.propDefinitions?.value?.type==='Number')?'range':key;
     const segments=path(expr,offset),field=segments.pop();
     const owner=project(current,segments);const line=source.slice(0,offset).split('\n').length;
     statements.push(`// @forma-source ${entry}:${line}`);
     if(owner===current)statements.push(`${variable}.bind_${method}_path(|vm| vm.${field}.clone()${key==='value'?`, ${twoWay}`:''});`);
     else statements.push(`${variable}.bind_${method}_context(&${owner}, |vm| vm.${field}.clone()${key==='value'?`, ${twoWay}`:''});`);

   }
   for(const [event,action]of Object.entries(node.events)){
     if(!['clicked','changed'].includes(event))fail('Неизвестное событие '+event,node.start);
     if(action.startsWith('state.')){if(node.eventArgs?.[event]?.length)fail('Аргументы команд используйте через типизированное event',node.start);statements.push(`// @forma-source ${entry}:${source.slice(0,node.statementRanges[event].from).split('\n').length}`);const segments=path(action,node.start),method=segments.pop(),owner=project(current,segments);statements.push(`{ let context = ${owner}.clone(); ${variable}.on_${event}(move |_| { if let Some(vm) = context.get() { vm.${method}(); } }); }`);continue;}
     if(!action.startsWith('events.'))fail('Событие ожидает events.name() или state.method()',node.start);
     const name=action.slice(7);const descriptor=eventTypes.get(name);
     if(!descriptor)fail(`Событие ${name} не объявлено в events`,node.statementRanges[event].from);
     if(descriptor.type&&descriptor.type!==type)fail(`Событие ${name} используется с разными типами контекста`,node.start);
     for(const argument of descriptor.args)if(['context','control_index'].includes(argument.name))fail('Имя аргумента события зарезервировано: '+argument.name,node.start);
     const args=node.eventArgs?.[event]??[];if(args.length!==descriptor.args.length)fail(`Событие ${name} ожидает ${descriptor.args.length} аргументов`,node.start);
     statements.push(`inner.declare_event(${nodeIndex}, ${rustString(event)}, &[${descriptor.args.map(a=>rustString(a.type)).join(',')}]);`);
     descriptor.type=type;descriptor.controls.push({variable,event});

   }
 }
 if(doc.nodes.length!==1||!layoutTypes.has(doc.nodes[0].type)||doc.nodes[0].type==='Scroll')fail('Форма должна содержать один корневой Frame, Row, Column, Grid или Stack');
 walk(doc.nodes[0],'context',rootType);
 const rustArgumentType=type=>type.endsWith('?')?`Option<${rustArgumentType(type.slice(0,-1))}>`:({String:'String',Bool:'bool',Boolean:'bool',Number:'f64',Float:'f64',Int:'i64'})[type]??fail(`Неподдерживаемый тип аргумента ${type}`);
 for(const [name,event]of eventTypes){
   if(!event.type)fail(`Событие ${name} объявлено, но не используется`);
   const eventType=event.args.length?`${doc.name}${name[0].toUpperCase()+name.slice(1)}Event`:`::forma::binding::Event<${event.type}>`;
   if(event.args.length)eventStructs.push(`pub struct ${eventType} { pub context: ::std::rc::Rc<${event.type}>, pub control_index: usize, ${event.args.map(a=>`pub ${validName(a.name,0)}: ${rustArgumentType(a.type)}`).join(',')} }`);
   methods.push(`pub fn on_${snake(name)}(&self, handler: impl Fn(${eventType}) + 'static) {
       let handler = ::std::rc::Rc::new(handler);
       ${event.controls.map(({variable,event:kind})=>`{ let handler = handler.clone(); self.${variable}.on_${kind}(move |event| handler(${event.args.length?`${eventType}{${event.args.map((a,i)=>`${a.name}: ::forma::binding::decode(&event.arguments[${i}]).expect("validated event argument")`).join(',')},context:event.context,control_index:event.control_index}`:'event'})); }`).join('\n')}
   }`);
 }

 const v=value=>{
   if(value?.expr?.startsWith('state.')) {let found=state;for(const key of value.expr.slice(6).split('.'))found=found?.[key];if(found!==undefined)return v(found);}
   if(value===null)return 'V::Null';
   if(value?.expression){const e=value.expression;const out=e.kind==='unary'?`E::Unary(${rustString(e.operator)}.into(),${v(e.argument)})`:e.kind==='binary'?`E::Binary(${rustString(e.operator)}.into(),${v(e.left)},${v(e.right)})`:e.kind==='conditional'?`E::Conditional(${v(e.condition)},${v(e.then)},${v(e.else)})`:e.kind==='member'?`E::Member(${v(e.object)},${v(e.property)},${!!e.optional})`:e.kind==='call'?`E::Call(${rustString(e.name)}.into(),vec![${e.args.map(v).join(',')}])`:fail('Неизвестное выражение '+e.kind);return `V::Expression(Box::new(${out}))`;}
   if(value?.interpolation)return `V::Interpolation(vec![${value.interpolation.map(v).join(',')}])`;
   if(typeof value==='string')return `V::Text(${rustString(value)}.into())`;
   if(typeof value==='boolean')return `V::Bool(${value})`;
   if(typeof value==='number')return `V::Number(${Number(value).toFixed(12)})`;
   if(Array.isArray(value))return `V::List(vec![${value.map(v).join(',')}])`;
   if(value?.expr)return `V::Expr(${rustString(value.expr)}.into())`;
   if(value&&Object.hasOwn(value,'match'))return `V::Match(Box::new(${v(value.match)}),vec![${value.branches.map(b=>`(${v(b.pattern)},${v(b.value)})`).join(',')}])`;
   if(value?.type&&value.props&&Array.isArray(value.children))return `V::Node(Box::new(${n(value)}))`;
   if(value&&typeof value==='object')return `V::Object(${p(value)})`;
   fail('Неподдерживаемое значение '+JSON.stringify(value));
 };
 const p=props=>`::forma::form_document::props(vec![${Object.entries(props).filter(([k])=>!['context','contextType'].includes(k)).map(([k,value])=>`(${rustString(k)},${v(value)})`).join(',')}])`;
 const metadata=node=>({...node.props,...(node.forward?.length?{__forward:node.forward}:{}),
   ...((node.matches??[]).length?{__matches:node.matches.map(m=>({match:m.match,branches:m.branches.map(b=>({pattern:b.pattern,value:b.props}))}))}:{}),
   ...(node.eventArgs?{__event_args:node.eventArgs}:{}),
   ...(node.__formIndex!==undefined?{__node:node.__formIndex}:{}),...(node.__formIndex===0?{__scope:definitionDefaults({...doc,defaults:Object.fromEntries(Object.entries(doc.defaults).filter(([k])=>!['contextType','events'].includes(k)))})}:{})});
 const n=(node,scene=false)=>{
   let props={...metadata(node),...Object.fromEntries(Object.entries(node.bindings??{}).map(([k,expr])=>[k,{expr}]))};
   if(scene&&node.type==='Slider'&&(node.bindings?.value||expressionReferences(node.props?.value).length))props.__range_normalized=true;
   const children=[...(node.children??[])];
   if(node.type==='If'){props.condition=node.condition;if(node.elseChildren?.length)children.push({type:'Else',props:{},children:node.elseChildren});}
   if(node.type==='For'){Object.assign(props,{item:node.item,collection:node.collection,key:node.key});if(node.emptyChildren?.length)children.push({type:'Empty',props:{},children:node.emptyChildren});}
   if(scene&&!layoutTypes.has(node.type)&&!['If','For','Else','Empty'].includes(node.type)&&!node.props?.key)props.key='__generated_'+node.__formIndex;
   return `N {kind:${rustString(node.type)}.into(),props: ${p(props)},children:vec![${children.map(c=>n(c,scene)).join(',')}],action:${node.events?.clicked?`Some(${rustString(scene?'actions.'+node.events.clicked.split('.').at(-1):node.events.clicked)}.into())`:'None'}}`;
 };
 const definitionDefaults=d=>({...d.defaults,...(Object.keys(d.propDefinitions??{}).length?{__contracts:d.propDefinitions}:{}),...(Object.keys(d.enums??{}).length?{__enums:d.enums}:{}),...((d.matches??[]).length?{__matches:d.matches.map(m=>({match:m.match,branches:m.branches.map(b=>({pattern:b.pattern,value:b.props}))}))}:{})});

 const images=Object.entries(files).filter(([name])=>name.endsWith('.svg')).map(([name,source])=>{
   const view=source.match(/viewBox\s*=\s*['"]([^'"]+)['"]/)?.[1].trim().split(/[\s,]+/).map(Number);
   const width=view?.[2]??Number(source.match(/\bwidth=['"]([^'"]+)/)?.[1]),height=view?.[3]??Number(source.match(/\bheight=['"]([^'"]+)/)?.[1]);
   const first=svgShapes(source,[0,0,width,height],'#010203'),second=svgShapes(source,[0,0,width,height],'#030201');
   return `(${rustString(name)}.into(), ::forma::form_document::Image {width:${width.toFixed(12)},height:${height.toFixed(12)},shapes:vec![${first.map((shape,i)=>`(vec![${shape.points.map(([x,y])=>`[${x.toFixed(12)},${y.toFixed(12)}]`).join(',')}],${rustString(shape.color!==second[i].color?'currentColor':shape.color)}.into())`).join(',')}]})`;
 });
 const program=`{ #[allow(unused_imports)] use ::forma::form_document::{Value as V,Expression as E,Node as N,Definition as D,Document}; Document {name:${rustString(doc.name)}.into(),root:${n(doc.nodes[0],true)},definitions: ::std::collections::BTreeMap::from([${Object.entries(linked.definitions).filter(([name])=>usedDefinitions.has(name)).map(([name,d])=>`(${rustString(name)}.into(),D {defaults: ${p(definitionDefaults(d))},nodes:vec![${d.nodes.map(node=>n(node)).join(',')}]})`).join(',')}]),fallback: ${p(linked.fallback)},images: ::std::collections::BTreeMap::from([${images.join(',')}]),states:vec![] } }`;
 const rust=`// @generated from ${entry.replace(/[\r\n]/g,' ')}. Do not edit.
${eventStructs.join('\n')}
pub struct ${doc.name} {
    inner: ::forma::binding::Form,
    context: ::forma::binding::Context<${rootType}>,
    ${fields.map(field=>field+',').join('\n    ')}
}
impl ${doc.name} {
    pub fn new(value: ::std::rc::Rc<${rootType}>) -> Result<Self, String> { Self::with_properties(value, ::std::collections::BTreeMap::new()) }
    pub fn with_properties(value: ::std::rc::Rc<${rootType}>, properties: ::forma::form_document::Properties) -> Result<Self, String> {
        let context = ::forma::binding::Context::new(value);
        let mut program = ${program};
        let allowed_properties: &[&str] = &[${[...new Set([...Object.keys(doc.defaults).filter(k=>!['contextType','events'].includes(k)),...Object.keys(doc.propDefinitions??{}),...(doc.matches??[]).flatMap(m=>m.branches.flatMap(b=>Object.keys(b.props??{})))])].map(rustString).join(',')}];
        for name in properties.keys() { if !allowed_properties.contains(&name.as_str()) { return Err(format!("Unknown form property {}", name)); } }
        if let Some(::forma::form_document::Value::Object(scope)) = program.root.props.get_mut("__scope") { scope.extend(properties); }
        let mut inner = ::forma::binding::Form::from_document(program)?;
        ${statements.join('\n        ')}
        inner.sync()?;
        Ok(Self { inner, context, ${controls.join(', ')} })
    }
    pub fn context(&self) -> ::forma::binding::Context<${rootType}> { self.context.clone() }
    pub fn set_context(&mut self, value: ::std::rc::Rc<${rootType}>) -> Result<(), String> { self.context.set(value); self.inner.sync() }
    pub fn clear_context(&mut self) -> Result<(), String> { self.context.clear(); self.inner.sync() }
    pub fn sync(&mut self) -> Result<(), String> { self.inner.sync() }
    pub fn is_dirty(&self) -> bool { self.inner.is_dirty() }
    pub fn on_dirty(&self, callback: impl Fn() + 'static) { self.inner.on_dirty(callback); }
    pub fn runtime(&mut self) -> Result<&::forma::Runtime, String> { self.inner.sync()?; Ok(self.inner.runtime()) }
    pub fn update<R>(&mut self, input: impl FnOnce(&mut ::forma::Runtime) -> R) -> Result<R, String> { self.inner.update(input) }
    ${methods.join('\n    ')}
}
`;
 const sourceMap={};let location=null;
 rust.split('\n').forEach((line,index)=>{const match=line.match(/\/\/ @forma-source (.*):(\d+)$/);if(match)location={file:match[1],line:Number(match[2])};else if(location&&line.trim()){sourceMap[index+1]=location;location=null;}});
 return {rust,sourceMap,formName:doc.name};
}
