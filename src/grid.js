import {resolve} from './language.js';
export const gridProperties=new Set(['columns','rows','column','row','cell','column.span','row.span']);
export function trackList(value,state){
  const values=Array.isArray(value)?value:[value];
  if(!values.length)throw Error('Список размеров не должен быть пустым');
  return values.map(v=>{
    const x=resolve(v,state);
    if(typeof x==='number'&&Number.isFinite(x)&&x>=0)return x+'px';
    if(x==='content'||x==='-')return 'max-content';
    if(typeof x==='string'&&/^(?:\d+(?:\.\d+)?)?\*$/.test(x))return (x==='*'?'1':x.slice(0,-1))+'fr';
    if(typeof x==='string'&&(x==='auto'||/^(?:\d+(?:\.\d+)?)(?:px|%|fr)$/.test(x)))return x;
    throw Error(`Недопустимый размер сетки: ${x}. Используйте число, px, %, *, 2*, content или auto`);
  }).join(' ');
}
export function gridStyles(node,state,parent=null){
  const p={...node.props},result={};
  if('cell'in p){
    if('row'in p||'column'in p)throw Error('cell нельзя сочетать с row или column');
    if(!Array.isArray(p.cell)||p.cell.length!==2)throw Error('cell: ожидаются два числа — строка и колонка');
    const [row,column]=p.cell.map(v=>resolve(v,state));
    if(!Number.isInteger(row)||row<1||!Number.isInteger(column)||column<1)throw Error('cell: строка и колонка должны быть целыми числами от 1');
    p.row=row;p.column=column;
  }
  if(node.type==='Grid'||'columns'in p||'rows'in p){
    if(!['Frame','Grid'].includes(node.type))throw Error('columns и rows доступны только для Frame или Grid');
    result.display='grid';result.alignContent='start';
    result.gridTemplateColumns='columns'in p?trackList(p.columns,state):'minmax(0, 1fr)';
    if('rows'in p)result.gridTemplateRows=trackList(p.rows,state);
  }
  if(parent?.type==='Stack'){result.gridColumnStart='1';result.gridRowStart='1';}
  for(const axis of ['column','row']){
    for(const key of [axis,axis+'.span'])if(key in p){const v=resolve(p[key],state);if(!Number.isInteger(v)||v<1)throw Error(`${key}: ожидается целое число от 1`);}
    const prefix=axis==='column'?'gridColumn':'gridRow';
    const inGrid=parent?.type==='Frame'&&('columns'in parent.props||'rows'in parent.props);
    if(inGrid&&!(axis in p)){
      result[prefix+'Start']='1';
      result[prefix+'End']='-1';
    }
    if(axis in p)result[prefix+'Start']=String(resolve(p[axis],state));
    if(axis+'.span'in p)result[prefix+'End']='span '+resolve(p[axis+'.span'],state);
  }
  return result;
}
