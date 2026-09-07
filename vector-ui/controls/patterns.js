// Compositions contain independent controls. Their state comes from the host;
// the shapes and text stay in the reusable .ui components.
const item = (type, key, x, y, props = {}) => ({type, key, x, y, ...props});
export function choiceGroup({key, labels, selected = 0, x = 0, y = 0, width = 304, rowHeight = 32}) {
  return labels.map((text, i) => item('RadioButton', `${key}${i}`, x, y + i * rowHeight, {text, width, height: rowHeight, checked: i === selected, action: `${key}${i}`}));
}
export function buttonGroup({key, labels, selected = 0, x = 0, y = 0, width = 304, height = 32, type = 'SegmentButton', gap = 4}) {
  const cell = (width - gap * (labels.length - 1)) / labels.length;
  return labels.map((text, i) => item(type, `${key}${i}`, x + i * (cell + gap), y, {text, width: cell, height, selected: i === selected, action: `${key}${i}`}));
}
export function numberStepper({key, value, x = 0, y = 0}) {
  return [item('IconButton', `${key}Less`, x, y, {width:32,height:32,icon:'assets/minus.svg',text:'Уменьшить',action:`${key}Less`}),
    item('NumberValue', `${key}Value`, x+32,y,{width:60,text:String(value)}),
    item('IconButton', `${key}More`,x+92,y,{width:32,height:32,icon:'assets/plus.svg',text:'Увеличить',action:`${key}More`})];
}
export function dataTable({key, columns = ['REPO','STACK','SIZE'], rows, x=0, y=0, width=304}) {
  return [item('TableHeader',`${key}Header`,x,y,{width,first:columns[0],second:columns[1],third:columns[2]}),
    ...rows.flatMap((row,i)=>[item('TableRow',`${key}Row${i}`,x,y+32+i*36,{width,height:36,first:row[0],second:row[1],third:row[2]}),
      item('Separator',`${key}Line${i}`,x,y+32+i*36,{width})])];
}
export function avatarGroup({key, labels, x=0, y=0}) {
  return labels.map((text,i)=>item('Avatar',`${key}${i}`,x+i*28,y,{width:36,height:36,text,borderWidth:2,borderColor:{expr:'state.theme.surface'}}));
}
export function breadcrumb({key, labels, x=0, y=0, widths}) {
  let left=x;
  return labels.flatMap((text,i)=>{
    const width=widths[i], result=[item('GhostButton',`${key}${i}`,left,y,{text,width,height:28,action:`${key}${i}`})]; left+=width;
    if(i<labels.length-1){result.push(item('Paragraph',`${key}Slash${i}`,left,y,{text:'/',width:18,height:28,horizontalPadding:0}));left+=18;}
    return result;
  });
}
export function documentTabs({key, labels, selected=0, x=0,y=0,width=304,dirty=[]}) {
  const w=width/labels.length;
  return labels.flatMap((text,i)=>[
    item('TabButton',`${key}${i}`,x+i*w,y,{text,width:w-26,height:32,selected:i===selected,action:`${key}${i}`}),
    ...(dirty.includes(i)?[item('StatusDot',`${key}Dirty${i}`,x+(i+1)*w-31,y+4,{width:5,height:5})]:[]),
    item('IconButton',`${key}Close${i}`,x+(i+1)*w-26,y+2,{width:24,height:28,radius:5,text:`Закрыть ${text}`,icon:'assets/close.svg',action:`${key}Close${i}`})]);
}
export function dialog({key, title, description, confirm='Сохранить', x=0,y=0,width=304}) {
  return [item('DialogSurface',`${key}Surface`,x,y,{width,height:176}),
    item('Paragraph',`${key}Title`,x+4,y+12,{text:title,width:width-40,height:28,fontSize:16}),
    item('IconButton',`${key}Close`,x+width-36,y+10,{width:28,height:28,icon:'assets/close.svg',text:'Закрыть диалог',action:`${key}Close`}),
    item('Paragraph',`${key}Description`,x+4,y+55,{text:description,width:width-8,height:24,fontSize:12}),
    item('SecondaryButton',`${key}Cancel`,x+16,y+120,{width:122,height:32,text:'Отмена',action:`${key}Close`}),
    item('PrimaryButton',`${key}Confirm`,x+width-138,y+120,{width:122,height:32,text:confirm,action:`${key}Confirm`})];
}
export function codeBlock({key,lines,x=0,y=0,width=304}) {
  return lines.map((text,i)=>item('CodeLine',`${key}${i}`,x,y+i*24,{text,number:String(i+1),width}));
}
