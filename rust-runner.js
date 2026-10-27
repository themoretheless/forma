import {prepareFormProject,mapFormDiagnostic} from './form-project.js';
import {spawn} from 'node:child_process';
import {mkdtemp,writeFile,mkdir,rm} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import path from 'node:path';
const maxFileCount=64,maxTotalLength=1_000_000;
export function createRustRunner(send,dependencies={}){
  const {
    spawn:spawnProcess=spawn,
    mkdtemp:makeTemporaryDirectory=mkdtemp,
    writeFile:writeSourceFile=writeFile,
    mkdir:makeDirectory=mkdir,
    rm:removeDirectory=rm,
  }=dependencies;
  let process=null,busy=false;
  return {
    async run(files,options={}){
      if(busy){send({kind:'error',text:'Rust уже запущен'});return;}
      busy=true;
      let directory=null,cleaned=false;
      const cleanup=()=>{
        if(cleaned||!directory)return;
        cleaned=true;
        const target=directory;directory=null;
        Promise.resolve(removeDirectory(target,{recursive:true,force:true})).catch(()=>{});
      };
      try{
        if(!files||typeof files!=='object'||Array.isArray(files))throw Error('Ожидаются файлы проекта');
        let sourceMap={};
        if(options.generated){const prepared=prepareFormProject(files,options.state,{nativeApp:options.native});files=prepared.files;sourceMap=prepared.sourceMap;send({kind:'stdout',text:`Сгенерировано форм: ${prepared.count}\n`});}
        if(typeof files['Cargo.toml']!=='string'||typeof files['src/main.rs']!=='string')throw Error('Нужны Cargo.toml и src/main.rs');
        const entries=Object.entries(files).filter(([file])=>/^(Cargo\.(toml|lock)|src\/.+\.rs)$/.test(file));
        if(entries.length>maxFileCount)throw Error(`Слишком много Rust-файлов (лимит ${maxFileCount})`);
        let totalLength=0;
        for(const [file,source]of entries){
          if(file.split('/').some(p=>p==='..'||!p)||file.includes('\\')||typeof source!=='string')throw Error('Некорректный путь Rust-файла');
          totalLength+=source.length;
          if(totalLength>maxTotalLength)throw Error(`Размер Rust-проекта превышает лимит ${maxTotalLength} символов`);
        }
        directory=await makeTemporaryDirectory(path.join(tmpdir(),'forma-rust-'));
        for(const [file,source]of entries){
          const destination=path.join(directory,file);await makeDirectory(path.dirname(destination),{recursive:true});await writeSourceFile(destination,source);
        }
        send({kind:'started',text:'cargo run — '+directory});
        const extraArgs=[
          ...(options.backend==='egui'||options.backend==='svet'?['--backend',options.backend]:[]),
          ...(options.platform==='ios'||options.platform==='android'?['--platform',options.platform]:[]),
        ];
        const args=['run','--offline',...(options.native?['--features','native']:[]),'--manifest-path',path.join(directory,'Cargo.toml'),...(extraArgs.length?['--',...extraArgs]:[])];
        const env=(options.backend||options.platform)?{...globalThis.process?.env,...(options.backend?{FORMA_BACKEND:options.backend}:{}),...(options.platform?{FORMA_PLATFORM:options.platform}:{})}:undefined;
        process=spawnProcess('cargo',args,{cwd:directory,stdio:['ignore','pipe','pipe'],...(env?{env}:{})});
        process.stdout.on('data',chunk=>send({kind:'stdout',text:chunk.toString()}));
        let errorBuffer='';
        process.stderr.on('data',chunk=>{errorBuffer+=chunk.toString();const lines=errorBuffer.split('\n');errorBuffer=lines.pop();if(lines.length)send({kind:'stderr',text:mapFormDiagnostic(lines.join('\n')+'\n',sourceMap)});});
        process.once('error',e=>{busy=false;process=null;cleanup();send({kind:'error',text:e.message});});
        process.once('close',(code,signal)=>{if(errorBuffer)send({kind:'stderr',text:mapFormDiagnostic(errorBuffer,sourceMap)});busy=false;process=null;cleanup();send({kind:'finished',code,text:`Завершено: ${signal??code}`});});
      }catch(e){busy=false;cleanup();send({kind:'error',text:e.message});}
    },
    stop(){process?.kill('SIGTERM');}
  };
}
