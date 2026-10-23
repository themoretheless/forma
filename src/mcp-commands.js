import {validateProject} from './browser-storage.js';
import {parse,resolve} from './language.js';
import {designStatesSummary,stateCreateEdit,stateDeleteEdit,statePropertyEdit,stateRenameEdit} from './design-states.js';
import {copyElement,insertElement,moveAmongSiblings,removeElement} from './element-edit.js';
import {insertableControls} from './control-catalog.js';
import {nativeSnapshot} from './native-snapshot.js';
import {buildNativeRustExport} from './native-export.js';
import {findNodeByStart} from './element-tree.js';

// The MCP transport invokes the same runtime and UI operations as the editor. The editor hands
// over one `studio` facade: live state through getters and setters (files, active, entry,
// scenario, selected, logs are reassigned by commands), the panels a command drives, and the
// operations the editor already has. Nothing here reaches into the editor's closure by name.
export function createIdeCommands(studio){
  const {$,hot}=studio;
  const snapshot=()=>({active:studio.active,entry:studio.entry,scenario:studio.scenario<0?null:studio.compiled?.states[studio.scenario]?.name??null,mode:studio.mode,error:studio.error,selected:studio.selected?.start??null,debug:{breakOn:studio.breakOn,paused:!!studio.pending,action:studio.pending?.action??null},renderer:studio.renderer,capabilities:{vectorRenderer:!!studio.vectorPreview,rustExecution:!!hot,rustDap:false}});
  const requireFile=path=>{if(!Object.hasOwn(studio.files,path))throw Error('File not found: '+path);};
  const findNode=start=>{const found=findNodeByStart(studio.compiled?.nodes,start);if(!found)throw Error('Component not found; refresh component_tree');return found;};
  const applyFiles=()=>{studio.selected=null;studio.open(studio.active);studio.persist();studio.compile(true);};
  return function ideCommand(command,args={}){
    const {files,entry,compiled,state,error,mode,renderer,vectorPreview}=studio;
    switch(command){
      case 'ide_status':return snapshot();
      case 'designer_tree':return studio.controlTree.snapshot();
      case 'designer_tree_view':studio.controlTree.setView(args);return studio.controlTree.snapshot();
      case 'designer_tree_select':studio.controlTree.selectId(args.id);return studio.controlTree.snapshot();
      case 'designer_tree_fold':studio.controlTree.fold(args.id,args.collapsed);return studio.controlTree.snapshot();
      case 'app_run':if(parse(files[entry]).defaults.contextType){hot.send('forma:form-run',{files,state});return {status:'requested',renderer:'generated-rust'};}if(renderer==='vector'){studio.nativeSource={entry,source:files[entry],nodes:structuredClone(compiled?.nodes??[])};if(error||!vectorPreview)throw Error(error||'Векторный renderer не готов');hot.send('forma:vector-run',studio.compileComponents(files,entry,state,{measureText:vectorPreview.measureText,designState:studio.scenario<0?null:compiled?.states[studio.scenario]?.name??null}));return {status:'requested',renderer};}if(error||!compiled)throw Error('Исправьте разметку');studio.render(false);try{hot.send('forma:native-run',{files,snapshot:nativeSnapshot($('preview'),compiled)});}finally{studio.render();}return {status:'requested'};
      case 'app_stop':hot.send('forma:rust-stop',{});hot.send('forma:native-stop',{});hot.send('forma:vector-stop',{});return {status:'requested'};
      case 'rust_run':if(!hot)throw Error('Requires local dev server');hot.send('forma:rust-run',{files});return {status:'requested',note:'Read events_read for compilation output and exit code'};
      case 'rust_stop':hot?.send('forma:rust-stop',{});return {status:'stop requested'};
      case 'project_read':return {...files};
      case 'project_export_native':return buildNativeRustExport(files,entry,args);
      case 'file_read':requireFile(args.path);return {path:args.path,content:files[args.path]};
      case 'file_open':requireFile(args.path);studio.open(args.path);break;
      case 'editor_fold':studio.codeEditor.fold(args);break;
      case 'file_write':{
        if(typeof args.path!=='string'||!args.path)throw Error('Invalid project path');validateProject({[args.path]:args.content});
        if(Object.hasOwn(files,args.path)&&args.expectedContent!==files[args.path])throw Error('File changed or expectedContent missing. Read it first.');
        files[args.path]=args.content;applyFiles();break;
      }
      case 'project_replace':{
        if(JSON.stringify(Object.entries(files).sort())!==JSON.stringify(Object.entries(args.expectedFiles).sort()))throw Error('Project changed. Read it first.');
        validateProject(args.files);
        studio.resetProjectEditing();const next=Object.fromEntries(Object.entries(args.files));studio.files=next;studio.active=Object.keys(next)[0];studio.entry=Object.keys(next).find(p=>p.endsWith('.ui')&&!p.endsWith('.design.ui'));studio.scenario=0;applyFiles();break;
      }
      case 'preview_renderer':studio.rendererPicker.value=args.renderer;studio.rendererPicker.onchange();break;
      case 'vector_status':return vectorPreview?.snapshot()??{ready:false};
      case 'preview_open':requireFile(args.path);studio.entry=args.path;studio.scenario=0;studio.compile(true);break;
      case 'preview_scenario':{if(args.name===null){if(!compiled?.states?.length)throw Error('The component has no design states');$('scenario').value=-1;$('scenario').onchange({target:$('scenario')});break;}const index=(compiled?.states??[]).findIndex(s=>s.name===args.name);if(index<0)throw Error('Design state not found');studio.scenario=index;$('scenario').value=index;$('scenario').onchange({target:$('scenario')});break;}
      // Same target as the states panel: the file that panel authors, never an arbitrary design file
      // the entry does not use. An agent that wants another one edits it through file_write.
      case 'design_states_read':return designStatesSummary(studio.designStateFile().source);
      case 'design_state_create':return studio.designStateEdit(source=>stateCreateEdit(source,args.name),args.expectedContent);
      case 'design_state_rename':return studio.designStateEdit(source=>stateRenameEdit(source,args.name,args.newName),args.expectedContent);
      case 'design_state_delete':return studio.designStateEdit(source=>stateDeleteEdit(source,args.name),args.expectedContent);
      // The state block is named, not indexed, so a rename between the read and this call cannot land
      // a property in the wrong block; `null` is the base block, which is what the panel's own row is.
      case 'design_state_property':return studio.designStateEdit(source=>statePropertyEdit(source,args.state??null,args.type,args.key,args.property,args.value),args.expectedContent);
      case 'preview_mode':if(mode!==args.mode)$('run').click();break;
      case 'preview_viewport':$(args.device).click();$('preview').classList.toggle('light',args.theme==='light');break;
      case 'component_tree':return {entry,nodes:compiled?.nodes??[],error};
      case 'component_spacing':return studio.spacingOverlay.read();
      case 'component_select':if(error)throw Error('Fix diagnostics before selecting compiled source');studio.select(findNode(args.start));break;
      case 'component_property':{
        if(error||files[entry]!==args.expectedContent)throw Error('Stale or invalid source. Read entry file first.');
        const n=findNode(args.start);if(!Object.hasOwn(n.props,args.property)||typeof n.props[args.property]==='object')throw Error('Only existing literal properties are supported');
        if(typeof n.props[args.property]!==typeof args.value)throw Error('Property type mismatch');
        studio.select(n);const input=studio.propertyInput(args.property);if(!input)throw Error('Свойство недоступно для правки: включите исходный вид');input.value=String(args.value);input.onchange();break;
      }
      // The palette's own list, so an agent inserts exactly what a designer can insert.
      case 'component_controls':return insertableControls(files);
      case 'component_insert':case 'component_move':case 'component_delete':case 'component_duplicate':{
        // All four splice the entry file the compiled tree came from, against its live text. The
        // splice reports the offset the new, moved or duplicated node takes — for a delete, the
        // parent that keeps the selection — and that is what the preview is asked to select:
        // without it the agent would have to re-read `component_tree` to find its own work.
        const source=studio.liveSource(entry);
        if(error||source!==args.expectedContent)throw Error('Stale or invalid source. Read the entry file first.');
        findNode(args.start);
        const change=command==='component_insert'
          ?insertElement(source,args.start,args.markup,false,args.side??'auto')
          :command==='component_move'
          ?moveAmongSiblings(source,args.start,args.target,args.side)
          // A duplicate is the node's own text spliced back after itself, so the key uniquifying
          // the paste path does covers a copied `key:` too. The copy keeps the source's
          // coordinates: the bridge also runs on the HTML preview, where no scene measured an
          // offset to take, and `component_property` can move it from there.
          :command==='component_delete'?removeElement(source,args.start)
          :insertElement(source,args.start,copyElement(source,args.start),false,'after');
        // A move onto the slot the node already holds is a no-op the primitive reports as null.
        // The caller still gets the file text back, because that is the content its next write
        // has to quote as `expectedContent`.
        if(!change)return {start:args.start,content:source,changed:false};
        studio.commitSource({file:entry,source,...change,reselect:change.start});
        return {start:change.start,content:source.slice(0,change.from)+change.insert+source.slice(change.to),changed:true};
      }
      case 'state_read':return structuredClone(state);
      case 'state_set':for(const [key,v]of Object.entries(args.values)){if(!Object.hasOwn(state,key)||typeof state[key]!==typeof v)throw Error('Unknown field or incompatible type: '+key);}Object.assign(state,args.values);studio.render();studio.output();break;
      case 'event_dispatch':if(renderer==='vector'){if(error)throw Error(error);if(mode!=='interact')throw Error('Включите взаимодействие');const n=findNode(args.start);if(!n.events.clicked)throw Error('Выберите кнопку с clicked');vectorPreview.activate();break;}if(error)throw Error('Fix diagnostics first');{const n=findNode(args.start);if(!n.events.clicked)throw Error('No clicked handler');if(resolve(n.props.disabled,state))throw Error('Component is disabled');studio.dispatch(n.events.clicked,n);break;}
      case 'debug_break':$('break').checked=args.enabled;$('break').onchange({target:$('break')});break;
      case 'debug_continue':if(!studio.pending)throw Error('Debugger is not paused');$('continue').click();break;
      case 'debug_reset':$('reset').click();break;
      case 'events_read':return [...studio.logs];
      case 'events_clear':studio.logs=[];studio.output();break;
      case 'diagnostics_read':return {error,entry};
      default:throw Error('Unknown IDE command: '+command);
    }
    return snapshot();
  };
}
