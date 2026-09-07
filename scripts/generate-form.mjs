import {readFileSync,readdirSync,mkdirSync,writeFileSync,existsSync} from 'node:fs';
import {resolve,relative,dirname,sep} from 'node:path';
import {fileURLToPath} from 'node:url';
import {generateForm} from '../src/form-codegen.js';
import {parse} from '../src/language.js';

// Build artifacts are written after every selected form validates.
export async function generateProject({project,entry,output,stateFile}) {
 project=resolve(project);
 if(entry){entry=entry.split(sep).join('/');if(entry.startsWith('/')||entry.split('/').includes('..'))throw Error('entry должен быть относительным путём внутри проекта');}
 const files={};
 function read(directory){if(!existsSync(resolve(project,directory)))return;for(const item of readdirSync(resolve(project,directory),{withFileTypes:true})){
   const name=directory+'/'+item.name;if(item.isSymbolicLink())throw Error(`Не поддерживается ссылка ${name}`);
   if(item.isDirectory())read(name);else if(/\.(ui|svg)$/.test(item.name))files[name]=readFileSync(resolve(project,name),'utf8');
 }}
 read('components');read('assets');
 if(entry)files[entry]=readFileSync(resolve(project,entry),'utf8');else read('ui');
 const entries=entry?[entry]:Object.keys(files).filter(name=>name.startsWith('ui/')&&name.endsWith('.ui')&&!name.endsWith('.design.ui')&&parse(files[name]).defaults.contextType);
 if(!entries.length)throw Error('Нет форм с contextType');
 const state=stateFile?JSON.parse(readFileSync(resolve(project,stateFile),'utf8')):{};
 let rust='';const sourceMap={},names=new Set();
 for(const entry of entries){
   const result=generateForm(files,entry,{state});
   if(names.has(result.formName))throw Error(`Повторное имя формы ${result.formName}`);names.add(result.formName);
   const offset=rust.split('\n').length-1;for(const [line,location]of Object.entries(result.sourceMap))sourceMap[Number(line)+offset]=location;
   rust+=result.rust+'\n';
 }
 mkdirSync(dirname(resolve(output)),{recursive:true});
 for(const [file,value]of [[output,rust],[output+'.map.json',JSON.stringify(sourceMap,null,2)+'\n']]){
   if(!existsSync(file)||readFileSync(file,'utf8')!==value)writeFileSync(file,value);
 }
 return {rust,sourceMap,formName:[...names].join(', ')};
}
if(process.argv[1]&&resolve(process.argv[1])===fileURLToPath(import.meta.url)){
 try{
   const args=process.argv.slice(2),options={};
   while(args.length){const flag=args.shift();if(!['--project','--entry','--output','--state'].includes(flag)||!args.length)throw Error('Usage: node scripts/generate-form.mjs --project DIR --output FILE [--entry ui/Form.ui] [--state state.json]');options[flag.slice(2)]=args.shift();}
   if(!options.project||!options.output)throw Error('Нужны --project и --output');
   const result=await generateProject({...options,stateFile:options.state});
   console.log(`Generated ${result.formName}: ${relative(process.cwd(),resolve(options.output))}`);
 }catch(error){console.error(error.message);process.exitCode=1;}
}
