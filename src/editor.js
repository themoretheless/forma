import {autocompletion,acceptCompletion,snippet} from '@codemirror/autocomplete';
import {createCompletionSource} from './completion.js';
import {undo,redo,isolateHistory} from '@codemirror/commands';
import {basicSetup} from 'codemirror';
import {EditorState,StateEffect,StateField,Compartment} from '@codemirror/state';
import {EditorView,Decoration,keymap} from '@codemirror/view';
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
export function mountEditor(textarea,{getProject=()=>({files:{},path:null})}={}){
  const host=document.createElement('div');host.className='source-editor';textarea.before(host);
  textarea.hidden=true;document.getElementById('lines').hidden=true;
  let programmatic=false,currentHighlight='',cachedText=textarea.value,inputQueued=false,selectionQueued=false;
  const languageConfig=new Compartment();
  const ranges=StateField.define({create:state=>blockRanges(state.doc.toString()),update:(value,tr)=>tr.docChanged?blockRanges(tr.newDoc.toString()):value});
  const completion=createCompletionSource(getProject);
  const complete=context=>{const result=completion(context);if(!result)return null;return {...result,options:result.options.map(item=>typeof item.apply==='string'&&item.apply.includes('{\n')?{...item,apply:snippet(item.apply.replace('    \n','    ${}\n'))}:item)};};
  const extensions=[basicSetup,autocompletion({override:[complete],activateOnTyping:true,activateOnCompletion:item=>item.type==='namespace'||typeof item.apply==='string'&&item.apply.endsWith('.')}),keymap.of([{key:'Tab',run:acceptCompletion}]),languageConfig.of(formaHighlight),ranges,selectionField,
    foldService.of((state,from,to)=>state.field(ranges).find(r=>r.from>from&&r.from<=to)),
    EditorView.contentAttributes.of({'aria-label':'Редактор исходного кода'}),
    EditorView.updateListener.of(update=>{
      if(update.docChanged){cachedText=null;if(!programmatic){currentHighlight='';if(!inputQueued){inputQueued=true;queueMicrotask(()=>{inputQueued=false;textarea.dispatchEvent(new Event('input'));});}}}
      if(update.selectionSet&&!update.docChanged&&!selectionQueued){selectionQueued=true;queueMicrotask(()=>{selectionQueued=false;if(!inputQueued)textarea.dispatchEvent(new Event('keyup'));});}
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
      '.cm-tooltip-autocomplete':{backgroundColor:'#1d2533',border:'1px solid #465775'},
      '.cm-tooltip-autocomplete ul li[aria-selected]':{backgroundColor:'#354969',color:'#ffffff'},
      '.cm-completionDetail':{color:'#9badc9',fontSize:'11px'},
      '.cm-selectionBackground':{backgroundColor:'#354969 !important'}
    },{dark:true})
  ];
  const view=new EditorView({parent:host,state:EditorState.create({doc:textarea.value,extensions})});
  const documents=new Map();let documentPath=null,pendingPath=null;
  Object.defineProperties(textarea,{
    value:{get:()=>cachedText??=view.state.doc.toString(),set:text=>{programmatic=true;currentHighlight='';try{if(pendingPath!==documentPath){if(documentPath)documents.set(documentPath,view.state);const saved=documents.get(pendingPath);view.setState(saved?.doc.toString()===text?saved:EditorState.create({doc:text,extensions}));documentPath=pendingPath;view.dispatch({effects:languageConfig.reconfigure(documentPath?.endsWith('.ui')?formaHighlight:[])});cachedText=text;}else if(view.state.doc.toString()!==text){view.dispatch({changes:{from:0,to:view.state.doc.length,insert:text}});}}finally{programmatic=false;}}},
    selectionStart:{get:()=>view.state.selection.main.from},
    selectionEnd:{get:()=>view.state.selection.main.to}
  });
  textarea.setSelectionRange=(start,end)=>{
    const effects=[];foldedRanges(view.state).between(0,view.state.doc.length,(from,to)=>{if(from<=start&&to>=start)effects.push(unfoldEffect.of({from,to}));});
    effects.push(EditorView.scrollIntoView(start,{y:'center'}));
    view.dispatch({selection:{anchor:start,head:end},effects});
  };
  view.dom.addEventListener('keydown',e=>{if((e.metaKey||e.ctrlKey)&&e.key==='s'){e.preventDefault();textarea.dispatchEvent(new KeyboardEvent('keydown',{key:'s',metaKey:e.metaKey,ctrlKey:e.ctrlKey}));}});
  return {position(){const from=view.state.selection.main.from,line=view.state.doc.lineAt(from);return {line:line.number,column:from-line.from+1};},undo(){return undo(view);},redo(){return redo(view);},edit(change){view.dispatch({changes:change,userEvent:'input.inspector',annotations:isolateHistory.of('full')});},setLanguage(path){pendingPath=path;},fold({line,collapsed=true}){
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
