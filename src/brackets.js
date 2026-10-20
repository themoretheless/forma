import {EditorView,Decoration,ViewPlugin} from '@codemirror/view';
const DECO_INVALID=Decoration.mark({class:'cm-bracket-invalid'});
const DECO_DEPTH=Array.from({length:6},(_,i)=>Decoration.mark({class:`cm-depth-${i}`}));
export function bracketLevels(text){
  const result=[],stackChar=[],stackLevel=[];let quote=null,line=false,block=false;
  for(let i=0;i<text.length;i++){
    const c=text[i],next=text[i+1];
    if(line){if(c==='\n')line=false;continue;}
    if(block){if(c==='*'&&next==='/'){block=false;i++;}continue;}
    if(quote){if(c==='\\'){i++;continue;}if(c===quote)quote=null;continue;}
    if(c==='/'&&next==='/'){line=true;i++;continue;}
    if(c==='/'&&next==='*'){block=true;i++;continue;}
    if(c==="'"||c==='"'){quote=c;continue;}
    if(c==='{'||c==='['||c==='('){const level=stackChar.length;result.push({from:i,level});stackChar.push(c);stackLevel.push(level);}
    else if(c==='}'||c===']'||c===')'){const expected=c==='}'?'{':c===']'?'[':'(';if(stackChar.length>0&&stackChar[stackChar.length-1]===expected){stackChar.pop();result.push({from:i,level:stackLevel.pop()});}else result.push({from:i,level:-1});}
  }
  return result;
}
function marks(view){const levels=bracketLevels(view.state.doc.toString());const ranges=new Array(levels.length);for(let i=0;i<levels.length;i++){const {from,level}=levels[i];ranges[i]=(level<0?DECO_INVALID:DECO_DEPTH[level%6]).range(from,from+1);}return Decoration.set(ranges);}
export const rainbowBrackets=[ViewPlugin.fromClass(class{
  constructor(view){this.decorations=marks(view);}
  update(update){if(update.docChanged)this.decorations=marks(update.view);}
},{decorations:v=>v.decorations}),EditorView.baseTheme({
  '.cm-depth-0, .cm-depth-0 span':{color:'#f3df6f !important'},
  '.cm-depth-1, .cm-depth-1 span':{color:'#b399ff !important'},
  '.cm-depth-2, .cm-depth-2 span':{color:'#ff91ca !important'},
  '.cm-depth-3, .cm-depth-3 span':{color:'#b7db61 !important'},
  '.cm-depth-4, .cm-depth-4 span':{color:'#839bff !important'},
  '.cm-depth-5, .cm-depth-5 span':{color:'#ff967c !important'},
  '.cm-bracket-invalid, .cm-bracket-invalid span':{color:'#ff747e !important',textDecoration:'underline wavy'}
})];
