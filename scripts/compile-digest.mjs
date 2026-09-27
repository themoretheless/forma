// Equivalence digest for the JS component compiler: hashes every compile output
// channel, not only the template transport, so a refactor must be byte-identical.
import {readFile} from 'node:fs/promises';
import {createHash} from 'node:crypto';
import {resolve} from 'node:path';
import {pathToFileURL} from 'node:url';

const root=resolve(import.meta.dirname,'..');
const {createComponentCompiler,compileComponents}=await import(pathToFileURL(resolve(root,'src/components.js')));
const read=async name=>readFile(resolve(root,'vector-ui/examples',name),'utf8');
const components={
  'components/Button.ui':await read('Button.slots.ui'),
  'components/ImageButton.ui':await read('ImageButton.component.ui'),
  'assets/search.svg':await read('search.svg'),
};
const hash=v=>createHash('sha256').update(typeof v==='string'?v:JSON.stringify(v)).digest('hex').slice(0,16);
const compile=createComponentCompiler();
const out=[];
for(const count of [1,3,32,64,256]){
  const files={...components,'Demo.ui':`component Demo { Frame { width: 900; height: 600; ${Array.from({length:count},(_,i)=>`ImageButton { text: 'Item ${i}'; width: 180; height: 50; }`).join(' ')} } }`};
  const r=compile(files,'Demo.ui');
  const cold=compileComponents(files,'Demo.ui');
  const same=hash(r.visualNodes)===hash(cold.visualNodes)&&hash(r.template)===hash(cold.template);
  out.push({count,source:hash(r.source),template:hash(r.template),visualNodes:hash(r.visualNodes),
    instanceTree:hash(r.instanceTree),templateTree:hash(r.templateTree),previewNodes:hash(r.previewNodes),
    previewControls:hash(r.previewControls),transportBytes:Buffer.byteLength(r.template),sessionMatchesCold:same});
}
console.log(JSON.stringify(out,null,1));
