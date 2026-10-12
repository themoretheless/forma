import {history,undo,redo,isolateHistory} from '@codemirror/commands';
import {EditorState,StateEffect,StateField,Compartment,Transaction} from '@codemirror/state';
import {Decoration,EditorView} from '@codemirror/view';
import {codeFolding,foldService,foldedRanges,unfoldEffect,foldEffect,foldAll,unfoldAll} from '@codemirror/language';
import {blockRanges} from './folding.js';

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
  // The editor's text and selection are announced to the page through the hidden textarea, the way
  // a native field would: one `input` per batch of document changes, one `keyup` per selection move.
  function changed(docChanged,selectionSet){
    if(docChanged){cachedText=null;if(!programmatic){currentHighlight='';if(!inputQueued){inputQueued=true;queueMicrotask(()=>{inputQueued=false;textarea.dispatchEvent(new Event('input'));});}}}
    if(selectionSet&&!docChanged&&!selectionQueued){selectionQueued=true;queueMicrotask(()=>{selectionQueued=false;if(!inputQueued)textarea.dispatchEvent(new Event('keyup'));});}
  }
  const ranges=StateField.define({create:state=>blockRanges(state.doc.toString()),update:(value,tr)=>tr.docChanged?blockRanges(tr.newDoc.toString()):value});
  // Gutters, keymaps, completion, search and the Forma grammar arrive as a separate chunk (see
  // editor-features.js); both compartments start empty and are filled once it has loaded.
  const features=new Compartment();let loaded=null;
  const languageFor=path=>loaded?.languageFor(path)??[];
  const extensions=[features.of([]),history(),codeFolding(),languageConfig.of([]),ranges,selectionField,
    foldService.of((state,from,to)=>state.field(ranges).find(r=>r.from>from&&r.from<=to)),
    EditorView.contentAttributes.of({'aria-label':'Редактор исходного кода'}),
    EditorView.updateListener.of(update=>changed(update.docChanged,update.selectionSet)),
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
  // Drawing the editor is the costliest part of start-up (DOM, measuring, a forced layout of the whole
  // page), and the minimal layout keeps the code pane closed. Until the pane first gets a size, every
  // operation runs against the EditorState alone and raises the same notifications; the view is built
  // from that state when it is shown, or when a command needs it.
  let view=null,state=EditorState.create({doc:textarea.value,extensions});
  const current=()=>view?view.state:state;
  function dispatch(spec){
    if(view){view.dispatch(spec);return;}
    const tr=spec instanceof Transaction?spec:state.update(spec);state=tr.state;changed(tr.docChanged,!!tr.selection);
  }
  const setState=next=>{if(view)view.setState(next);else state=next;};
  const target=()=>({state:current(),dispatch});
  function ensureView(){
    if(view)return view;
    view=new EditorView({parent:host,state,scrollTo:EditorView.scrollIntoView(state.selection.main.from,{y:'center'})});state=null;
    view.dom.addEventListener('keydown',e=>{if((e.metaKey||e.ctrlKey)&&e.key==='s'){e.preventDefault();textarea.dispatchEvent(new KeyboardEvent('keydown',{key:'s',metaKey:e.metaKey,ctrlKey:e.ctrlKey}));}});
    return view;
  }
  const shown=new ResizeObserver(entries=>{if(entries.some(entry=>entry.contentRect.width>0&&entry.contentRect.height>0)){shown.disconnect();ensureView();}});
  shown.observe(host);
  // Showing the pane and focusing it happen in one step, before the observer above has run.
  textarea.focus=()=>ensureView().focus();
  const load=()=>import('./editor-features.js').then(module=>{
    loaded={features:module.editorFeatures(getProject),languageFor:module.languageFor};
    const wasProgrammatic=programmatic;programmatic=true;
    try{dispatch({effects:[features.reconfigure(loaded.features),languageConfig.reconfigure(languageFor(documentPath))]});}finally{programmatic=wasProgrammatic;}
  });
  (globalThis.requestIdleCallback??(callback=>setTimeout(callback,1)))(()=>load().catch(error=>console.error('Code editor features failed to load',error)));
  const documents=new Map();let documentPath=null,pendingPath=null;
  Object.defineProperties(textarea,{
    value:{get:()=>cachedText??=current().doc.toString(),set:text=>{programmatic=true;currentHighlight='';try{if(pendingPath!==documentPath){if(documentPath)documents.set(documentPath,current());const saved=documents.get(pendingPath);setState(saved?.doc.toString()===text?saved:EditorState.create({doc:text,extensions}));documentPath=pendingPath;dispatch({effects:[languageConfig.reconfigure(languageFor(documentPath)),features.reconfigure(loaded?.features??[])]});cachedText=text;}else if(current().doc.toString()!==text){dispatch({changes:{from:0,to:current().doc.length,insert:text}});}}finally{programmatic=false;}}},
    selectionStart:{get:()=>current().selection.main.from},
    selectionEnd:{get:()=>current().selection.main.to}
  });
  textarea.setSelectionRange=(start,end)=>{
    const effects=[];foldedRanges(current()).between(0,current().doc.length,(from,to)=>{if(from<=start&&to>=start)effects.push(unfoldEffect.of({from,to}));});
    effects.push(EditorView.scrollIntoView(start,{y:'center'}));
    dispatch({selection:{anchor:start,head:end},effects});
  };
  return {resetProject(){documents.clear();documentPath=null;pendingPath=null;currentHighlight='';},position(){const from=current().selection.main.from,line=current().doc.lineAt(from);return {line:line.number,column:from-line.from+1};},undo(){return undo(target());},redo(){return redo(target());},edit(change){dispatch({changes:change,userEvent:'input.inspector',annotations:isolateHistory.of('full')});},setLanguage(path){pendingPath=path;},fold({line,collapsed=true}){
    if(line===undefined){(collapsed?foldAll:unfoldAll)(ensureView());return;}
    if(line<1||line>current().doc.lines)throw Error('Line out of range');
    const location=current().doc.line(line),range=current().field(ranges).find(r=>r.from>location.from&&r.from<=location.to);
    if(!range)throw Error('No foldable block on this line');
    dispatch({effects:(collapsed?foldEffect:unfoldEffect).of(range)});
  },highlight(range){const key=range?`${range.start}:${range.end}`:'';if(key===currentHighlight)return;currentHighlight=key;
    const effects=[highlight.of(range)];
    if(range)foldedRanges(current()).between(0,current().doc.length,(from,to)=>{if(from<range.end&&to>range.start)effects.push(unfoldEffect.of({from,to}));});
    dispatch({effects});
  }};
}
