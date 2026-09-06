import {EditorView,Decoration,ViewPlugin} from '@codemirror/view';
export function bracketLevels(text){
  const result=[],stack=[];let quote=null,line=false,block=false;
  const closing={'}':'{',']':'[',')':'('};
  for(let i=0;i<text.length;i++){
    const c=text[i],next=text[i+1];
    if(line){if(c==='\n')line=false;continue;}
    if(block){if(c==='*'&&next==='/'){block=false;i++;}continue;}
    if(quote){if(c==='\\'){i++;continue;}if(c===quote)quote=null;continue;}
    if(c==='/'&&next==='/'){line=true;i++;continue;}
    if(c==='/'&&next==='*'){block=true;i++;continue;}
    if(c==="'"||c==='"'){quote=c;continue;}
    if('{[('.includes(c)){const mark={from:i,level:stack.length};result.push(mark);stack.push({char:c,mark});}
    else if(c in closing){if(stack.at(-1)?.char===closing[c])result.push({from:i,level:stack.pop().mark.level});else result.push({from:i,level:-1});}
  }
  return result;
}
function marks(view){return Decoration.set(bracketLevels(view.state.doc.toString()).map(({from,level})=>Decoration.mark({class:level<0?'cm-bracket-invalid':`cm-depth-${level%6}`}).range(from,from+1)));}
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
