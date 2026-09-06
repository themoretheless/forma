export const sizeProperties=new Set(['width','height','minWidth','maxWidth','minHeight','maxHeight']);
export function sizeValue(value){
  if(value==='content'||value==='-')return 'fit-content';
  if(typeof value==='number'&&Number.isFinite(value)&&value>=0)return value+'px';
  if(typeof value==='string'&&(value==='auto'||/^(?:\d+(?:\.\d+)?)(?:px|%)$/.test(value)))return value;
  throw Error('Размер: ожидается content, auto, неотрицательное число, px или %');
}
