import {fileURLToPath} from 'node:url';
import {generateForm} from './src/form-codegen.js';
import {parse} from './src/language.js';

// Stage generated source next to the user's Rust modules. The virtual project
// remains unchanged; each run recompiles all forms against the current model.
export function prepareFormProject(files,state={}) {
 if(typeof files['Cargo.toml']!=='string'||typeof files['src/main.rs']!=='string')throw Error('Для живой формы нужны Cargo.toml и src/main.rs с ViewModel и запуском формы');
 const output={...files},sourceMap={};let generated='',count=0;
 for(const [entry,source]of Object.entries(files)){
  if(!entry.startsWith('ui/')||!entry.endsWith('.ui'))continue;
  const doc=parse(source);if(!doc.defaults.contextType)continue;
  const result=generateForm(files,entry,{state});const offset=generated.split('\n').length-1;
  for(const [line,location]of Object.entries(result.sourceMap))sourceMap[Number(line)+offset]=location;
  generated+=result.rust+'\n';count++;
 }
 if(!count)throw Error('Нет форм с contextType');
 if(Object.hasOwn(files,'src/forma_generated.rs'))throw Error('src/forma_generated.rs зарезервирован для результата генерации');
 output['src/forma_generated.rs']=generated;
 const runtime=fileURLToPath(new URL('./vector-ui',import.meta.url));
 const manifest=output['Cargo.toml'];
 if(!/^forma\s*=\s*\{[^\n]*\bpath\s*=/m.test(manifest))throw Error('Cargo.toml: укажите forma = { path = "…" } в dependencies');
 output['Cargo.toml']=manifest.replace(/^(forma\s*=\s*\{[^\n]*?\bpath\s*=\s*)"[^"]*"/m,(_,prefix)=>prefix+JSON.stringify(runtime));
 return {files:output,sourceMap,count};
}
export function mapFormDiagnostic(text,sourceMap) {
 return text.replace(/(?:[^\s]*\/)?src\/forma_generated\.rs:(\d+):(\d+)/g,(original,line)=>{
  const source=sourceMap[line];return source?`${source.file}:${source.line}:1 (generated: ${original})`:original;
 });
}
