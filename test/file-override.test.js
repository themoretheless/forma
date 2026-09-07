import {test} from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {compileComponents} from '../src/components.js';
import {parser} from '../src/forma-parser.js';

const files={
 'ui/Demo.ui':"component Demo { Frame { SearchButton {} } }",
 'components/Button.ui':"component Button { Rectangle { radius:12; background:#112233; Border { key:'outline'; width:1; background:#ffffff; } PointerArea { clicked -> events.clicked(); } } }",
 'components/SearchButton.ui':"component SearchButton : Button { override 'outline' from '../styles/Accent.ui' { width:2; } }",
 'styles/Accent.ui':"Border { background: Brush { color:#bed0ff; focus:#ffffff; transition:100ms; }; }",
};
test('file override merges properties, keeps siblings and permits local priority',()=>{
 const out=compileComponents(files,'ui/Demo.ui');
 assert.equal((out.template.match(/Border \{/g)||[]).length,1);
 assert.match(out.template,/width: 2;/);assert.match(out.template,/#bed0ff/);
 assert.match(out.template,/events.clicked/);assert.doesNotMatch(out.template,/key:/);
 const noLocal={...files,'components/SearchButton.ui':"component SearchButton : Button { override 'outline' from '../styles/Accent.ui'; }"};
 assert.match(compileComponents(noLocal,'ui/Demo.ui').template,/width: 1;/);
});
test('file override rejects missing targets/files, type mismatch and structural edits',()=>{
 for(const [path,source,pattern] of [
  ['styles/Accent.ui','Text { text: \'bad\'; }',/ожидался Border/],
  ['styles/Accent.ui','Border { Text {} }',/только свойства/],
  ['styles/Accent.ui',"Border { key:'other'; }",/не меняет key/],
  ['styles/Accent.ui','Border {} Border {}',/Лишний текст/],
  ['components/SearchButton.ui',"component SearchButton : Button { override 'missing' from '../styles/Accent.ui'; }",/найдено 0/],
  ['components/SearchButton.ui',"component SearchButton : Button { override 'outline' from './missing.ui'; }",/не найден/],
  ['components/SearchButton.ui',"component SearchButton : Button { override 'outline' from '../../outside.ui'; }",/пределы проекта/],
  ['components/SearchButton.ui',"component SearchButton : Button { override 'outline' from '../styles/Accent.ui' { clicked -> events.clicked(); } }",/только свойства/],
 ]) assert.throws(()=>compileComponents({...files,[path]:source},'ui/Demo.ui'),pattern);
});
test('file override supports chained inheritance and editor grammar',()=>{
 const f={...files,'components/Middle.ui':files['components/SearchButton.ui'].replace('SearchButton','Middle'),
 'components/SearchButton.ui':"component SearchButton : Middle { override 'outline' from '../styles/Accent.ui' { width:3; }; }"};
 assert.match(compileComponents(f,'ui/Demo.ui').template,/width: 3;/);
 for(const source of Object.values(f))parser.parse(source).iterate({enter(n){assert.notEqual(n.type.isError,true,source);}});
});
test('file override output loads and paints in the Rust WASM runtime',async()=>{
 const runtime=await import('../public/vector-pkg/forma.js');
 await runtime.default({module_or_path:readFileSync(new URL('../public/vector-pkg/forma_bg.wasm',import.meta.url))});
 const out=compileComponents(files,'ui/Demo.ui');const button=new runtime.Button();
 try{button.load_component(out.source,out.template);assert.equal(button.pixels(400,200,1).length,320000);}finally{button.free();}
});
