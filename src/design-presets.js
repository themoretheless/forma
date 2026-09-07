export function designPreset(name,node){
 const props=node.props??{},patch={};
 if(name==='disabled')return ['Frame','Scroll'].includes(node.type)?{}:{disabled:true};
 if(name==='long'){
  if(typeof props.text==='string')patch.text='Очень длинный заголовок для проверки размещения и обрезки текста';
  if(['TextField','TextArea','SearchField'].includes(node.type))patch.value='Длинный текст для проверки поля ввода и положения каретки';
 }
 if(name==='empty'){
  if(typeof props.text==='string')patch.text='';
  if(['TextField','TextArea','SearchField'].includes(node.type))patch.value='';
 }
 return patch;
}

// Collection fixtures are explicit designer-only scenes, never mutations of the
// user's document or a simulation of application bindings.
export function collectionScenario(name,scene,files){
 if(!['list-empty','list-12','list-100','loading','error'].includes(name))return;
 const root=scene[0],nodes=root.children[0]?.type==='Scroll'?root.children[0].children:root.children;
 const sample=nodes.find(n=>n.type==='TableRow');
 if(!sample)throw Error('Для сценария списка нужен хотя бы один TableRow в текущем интерфейсе');
 const width=Math.max(80,Number(root.props.width??360)-48),rowHeight=Number(sample.props.height??38);
 const make=(type,props)=>({...structuredClone(sample),type,children:[],props:{x:24,width,...props},events:{},bindings:{}});
 let rows=[];
 if(name==='list-12'||name==='list-100'){
  const count=name==='list-12'?12:100;
  rows=Array.from({length:count},(_,i)=>make('TableRow',{...structuredClone(sample.props),x:24,y:24+i*(rowHeight+6),width,height:rowHeight,key:'design-row-'+i,first:'Документ '+(i+1),second:i%2?'Готов':'В очереди',third:(i+1)*12+' KB'}));
 }else if(name==='loading'){
  if(!files['components/Skeleton.ui'])throw Error('Для загрузки нужен компонент Skeleton');
  rows=Array.from({length:6},(_,i)=>make('Skeleton',{y:24+i*(rowHeight+6),height:rowHeight,key:'design-loading-'+i}));
 }else if(name==='error'){
  if(!files['components/Alert.ui'])throw Error('Для ошибки нужен компонент Alert');
  rows=[make('Alert',{y:24,height:90,key:'design-error',text:'Не удалось загрузить список',description:'Проверка размещения сообщения об ошибке',tone:'danger'})];
 }else{
  if(!files['components/EmptyState.ui'])throw Error('Для пустого списка нужен компонент EmptyState');
  rows=[make('EmptyState',{y:24,height:140,key:'design-empty',text:'Список пуст',description:'Здесь появятся документы'})];
 }
 root.props={...root.props,padding:0,gap:0,clip:true};delete root.props.columns;delete root.props.rows;
 root.children=[{type:'Scroll',props:{},children:rows,events:{},bindings:{},start:root.start,end:root.end,source:root.source}];
}
