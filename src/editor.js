import {basicSetup} from 'codemirror';
import {EditorState,StateEffect,StateField,Compartment} from '@codemirror/state';
import {EditorView,Decoration} from '@codemirror/view';
import {foldService,foldedRanges,unfoldEffect,foldEffect,foldAll,unfoldAll} from '@codemirror/language';
import {blockRanges} from './folding.js';
import {formaHighlight} from './forma-highlight.js';

const highlight=StateEffect.define();
const selectionField=StateField.define({
  create:()=>Decoration.none,
  update(value,tr){
    if(tr.docChanged)value=Decoration.none;
    for(const effect of tr.effects)if(effect.is(highlight)){
      const range=effect.value,marks=[];
      if(range){const first=tr.state.doc.lineAt(range.start).number,last=tr.state.doc.lineAt(range.end).number;
        for(let line=first;line<=last;line++)marks.push(Decoration.line({class:'cm-designer-selected'}).range(tr.state.doc.line(line).from));}
      value=Decoration.set(marks);
    }
    return value;
  },
  provide:field=>EditorView.decorations.from(field)
});
export function mountEditor(textarea){
  const host=document.createElement('div');host.className='source-editor';textarea.before(host);
  textarea.hidden=true;document.getElementById('lines').hidden=true;
  let programmatic=false,currentHighlight='';
  const languageConfig=new Compartment();
  const ranges=StateField.define({create:state=>blockRanges(state.doc.toString()),update:(value,tr)=>tr.docChanged?blockRanges(tr.newDoc.toString()):value});
  const view=new EditorView({parent:host,state:EditorState.create({doc:textarea.value,extensions:[basicSetup,languageConfig.of(formaHighlight),ranges,selectionField,
    foldService.of((state,from,to)=>state.field(ranges).find(r=>r.from>from&&r.from<=to)),
    EditorView.contentAttributes.of({'aria-label':'Редактор исходного кода'}),
    EditorView.updateListener.of(update=>{
      if(update.docChanged&&!programmatic){currentHighlight='';queueMicrotask(()=>textarea.dispatchEvent(new Event('input')));}
      if(update.selectionSet)queueMicrotask(()=>textarea.dispatchEvent(new Event('keyup')));
    }),
    EditorView.theme({
      '&':{height:'100%',backgroundColor:'#13161c',color:'#c9d6f2',fontSize:'12px'},
      '.cm-scroller':{fontFamily:'"IBM Plex Mono", monospace',lineHeight:'23px',overflow:'auto'},
      '.cm-content':{padding:'16px 0 30px',caretColor:'#b9caff'},
      '.cm-gutters':{backgroundColor:'#13161c',color:'#617089',border:'none'},
      '.cm-activeLine,.cm-activeLineGutter':{backgroundColor:'transparent'},
      '.cm-foldGutter .cm-gutterElement':{cursor:'pointer',color:'#a9bbdf',padding:'0 5px'},
      '.cm-foldPlaceholder':{backgroundColor:'#293753',color:'#c7d7ff',border:'1px solid #50678b',borderRadius:'4px',padding:'0 5px'},
      '.cm-designer-selected':{backgroundColor:'#789bff21',boxShadow:'inset 3px 0 #9ab3ff'},
      '&.cm-focused':{outline:'none'},
      '.cm-selectionBackground':{backgroundColor:'#354969 !important'}
    },{dark:true})
  ]})});
  Object.defineProperties(textarea,{
    value:{get:()=>view.state.doc.toString(),set:text=>{programmatic=true;currentHighlight='';view.dispatch({changes:{from:0,to:view.state.doc.length,insert:text}});programmatic=false;}},
    selectionStart:{get:()=>view.state.selection.main.from},
    selectionEnd:{get:()=>view.state.selection.main.to}
  });
  textarea.setSelectionRange=(start,end)=>{
    const effects=[];foldedRanges(view.state).between(0,view.state.doc.length,(from,to)=>{if(from<=start&&to>=start)effects.push(unfoldEffect.of({from,to}));});
    effects.push(EditorView.scrollIntoView(start,{y:'center'}));
    view.dispatch({selection:{anchor:start,head:end},effects});
  };
  view.dom.addEventListener('keydown',e=>{if((e.metaKey||e.ctrlKey)&&e.key==='s'){e.preventDefault();textarea.dispatchEvent(new KeyboardEvent('keydown',{key:'s',metaKey:e.metaKey,ctrlKey:e.ctrlKey}));}});
  return {setLanguage(path){view.dispatch({effects:languageConfig.reconfigure(path.endsWith('.ui')?formaHighlight:[])});},fold({line,collapsed=true}){
    if(line===undefined){(collapsed?foldAll:unfoldAll)(view);return;}
    if(line<1||line>view.state.doc.lines)throw Error('Line out of range');
    const location=view.state.doc.line(line),range=view.state.field(ranges).find(r=>r.from>location.from&&r.from<=location.to);
    if(!range)throw Error('No foldable block on this line');
    view.dispatch({effects:(collapsed?foldEffect:unfoldEffect).of(range)});
  },highlight(range){const key=range?`${range.start}:${range.end}`:'';if(key===currentHighlight)return;currentHighlight=key;
    const effects=[highlight.of(range)];
    if(range)foldedRanges(view.state).between(0,view.state.doc.length,(from,to)=>{if(from<range.end&&to>range.start)effects.push(unfoldEffect.of({from,to}));});
    view.dispatch({effects});
  }};
}
