import {spawn} from 'node:child_process';
import {mkdir,writeFile} from 'node:fs/promises';
import {fileURLToPath} from 'node:url';
export function createNativeRunner(send){
  let child,busy=false;
  const root=fileURLToPath(new URL('./',import.meta.url));
  return {async run({files,snapshot}){
    if(busy){send({kind:'error',text:'Окно уже запущено'});return;}busy=true;
    try{
      if(!snapshot?.html||typeof files?.['src/actions.rs']!=='string')throw Error('Нужны корректный preview и src/actions.rs');
      await mkdir(root+'.forma',{recursive:true});
      await writeFile(root+'.forma/native-snapshot.json',JSON.stringify(snapshot));
      await writeFile(root+'.forma/native-actions.rs',files['src/actions.rs']);
      send({kind:'started',text:'Сборка нативного окна…'});
      child=spawn('cargo',['run','--offline','--manifest-path',root+'native-app/Cargo.toml','--',root+'.forma/native-snapshot.json'],{cwd:root,env:{...process.env,FORMA_ACTIONS:root+'.forma/native-actions.rs'},stdio:['ignore','pipe','pipe']});
      for(const channel of ['stdout','stderr'])child[channel].on('data',data=>send({kind:channel,text:data.toString()}));
      child.on('error',e=>{busy=false;send({kind:'error',text:e.message});});
      child.on('close',code=>{busy=false;child=null;send({kind:'finished',text:'Окно закрыто, код '+code});});
    }catch(e){busy=false;send({kind:'error',text:e.message});}
  },stop(){child?.kill('SIGTERM');}};
}
