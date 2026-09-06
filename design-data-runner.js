import {execFile} from 'node:child_process';
import {promisify} from 'node:util';
import {mkdtemp,writeFile,readFile} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import path from 'node:path';
const exec=promisify(execFile);
export async function evaluateDesignData(files,refs){
  if(!Array.isArray(refs)||refs.length>100||refs.some(r=>!/^design\.[A-Za-z_][A-Za-z0-9_]*(\(\))?$/.test(r)))throw Error('Поддерживаются design.NAME и design.method() без аргументов');
  if(typeof files?.['src/design.rs']!=='string')throw Error('Добавьте src/design.rs с pub const или pub fn');
  const dir=await mkdtemp(path.join(tmpdir(),'forma-design-'));
  await writeFile(path.join(dir,'design.rs'),files['src/design.rs']);
  const source='mod design; fn main() {\n'+refs.map((r,i)=>`std::fs::write("value-${i}", format!("{}", design::${r.slice('design.'.length)})).unwrap();`).join('\n')+'\n}';
  await writeFile(path.join(dir,'main.rs'),source);
  await exec('rustc',['--edition=2021','main.rs','-o','design-data'],{cwd:dir,timeout:30000,maxBuffer:1024*1024});
  await exec(path.join(dir,'design-data'),[],{cwd:dir,timeout:5000,maxBuffer:1024*1024});
  return Object.fromEntries(await Promise.all(refs.map(async(r,i)=>[r,await readFile(path.join(dir,`value-${i}`),'utf8')])));
}
