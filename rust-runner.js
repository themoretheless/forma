import {prepareFormProject,mapFormDiagnostic} from './form-project.js';
import {spawn} from 'node:child_process';
import {mkdtemp,writeFile,mkdir} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import path from 'node:path';
export function createRustRunner(send){
  let process=null,busy=false;
  return {
    async run(files,options={}){
      if(busy){send({kind:'error',text:'Rust уже запущен'});return;}
      busy=true;
      try{
        let sourceMap={};
        if(options.generated){const prepared=prepareFormProject(files,options.state);files=prepared.files;sourceMap=prepared.sourceMap;send({kind:'stdout',text:`Сгенерировано форм: ${prepared.count}\n`});}
        if(typeof files['Cargo.toml']!=='string'||typeof files['src/main.rs']!=='string')throw Error('Нужны Cargo.toml и src/main.rs');
        const directory=await mkdtemp(path.join(tmpdir(),'forma-rust-'));
        for(const [file,source]of Object.entries(files)){
          if(!/^(Cargo\.(toml|lock)|src\/.+\.rs)$/.test(file))continue;
          if(file.split('/').some(p=>p==='..'||!p)||file.includes('\\')||typeof source!=='string')throw Error('Некорректный путь Rust-файла');
          const destination=path.join(directory,file);await mkdir(path.dirname(destination),{recursive:true});await writeFile(destination,source);
        }
        send({kind:'started',text:'cargo run — '+directory});
        process=spawn('cargo',['run','--offline','--manifest-path',path.join(directory,'Cargo.toml')],{cwd:directory,stdio:['ignore','pipe','pipe']});
        process.stdout.on('data',chunk=>send({kind:'stdout',text:chunk.toString()}));
        let errorBuffer='';
        process.stderr.on('data',chunk=>{errorBuffer+=chunk.toString();const lines=errorBuffer.split('\n');errorBuffer=lines.pop();if(lines.length)send({kind:'stderr',text:mapFormDiagnostic(lines.join('\n')+'\n',sourceMap)});});
        process.once('error',e=>{busy=false;process=null;send({kind:'error',text:e.message});});
        process.once('close',(code,signal)=>{if(errorBuffer)send({kind:'stderr',text:mapFormDiagnostic(errorBuffer,sourceMap)});busy=false;process=null;send({kind:'finished',code,text:`Завершено: ${signal??code}`});});
      }catch(e){busy=false;send({kind:'error',text:e.message});}
    },
    stop(){process?.kill('SIGTERM');}
  };
}
