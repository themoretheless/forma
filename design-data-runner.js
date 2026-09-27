import {execFile} from 'node:child_process';
import {promisify} from 'node:util';
import {mkdtemp,writeFile,open,rm} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import path from 'node:path';
const exec=promisify(execFile);
export async function evaluateDesignData(files,refs){
  if(!Array.isArray(refs)||refs.length>100||refs.some(r=>!/^design\.[A-Za-z_][A-Za-z0-9_]*(\(\))?$/.test(r)))throw Error('Поддерживаются design.NAME и design.method() без аргументов');
  if(typeof files?.['src/design.rs']!=='string')throw Error('Добавьте src/design.rs с pub const или pub fn');
  const dir=await mkdtemp(path.join(tmpdir(),'forma-design-'));
  try{
  await writeFile(path.join(dir,'design.rs'),files['src/design.rs']);
  const source='mod design; fn main() {\n'+refs.map((r,i)=>`std::fs::write("value-${i}", format!("{}", design::${r.slice('design.'.length)})).unwrap();`).join('\n')+'\n}';
  await writeFile(path.join(dir,'main.rs'),source);
  const executable=process.platform==='win32'?'design-data.exe':'design-data';
  await exec('rustc',['--edition=2021','main.rs','-o',executable],{cwd:dir,timeout:30000,maxBuffer:1024*1024});
  await exec(path.join(dir,executable),[],{cwd:dir,timeout:5000,maxBuffer:1024*1024});
  return await readDesignValues(dir,refs);
  }finally{await rm(dir,{recursive:true,force:true});}
}

// One bounded scratch buffer; never read arbitrary-size result files in parallel.
export async function readDesignValues(dir,refs){
 const limit=1024*1024,buffer=Buffer.allocUnsafe(limit+1),values={};let total=0;
 for(let i=0;i<refs.length;i++){
  const file=await open(path.join(dir,`value-${i}`),'r');
  try{
   if(!(await file.stat()).isFile())throw Error('Результат design должен быть обычным файлом');
   let used=0;
   while(used<=limit-total){
    const {bytesRead}=await file.read(buffer,used,limit-total+1-used,null);
    if(!bytesRead)break;used+=bytesRead;
   }
   if(used>limit-total)throw Error('Результаты design превышают 1 МиБ');
   total+=used;values[refs[i]]=buffer.toString('utf8',0,used);
  }finally{await file.close();}
 }
 return values;
}
