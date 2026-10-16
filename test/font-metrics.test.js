import {test} from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import * as runtime from '../public/vector-pkg/forma.js';
import {compileComponents} from '../src/components.js';
import {parse} from '../src/language.js';
await runtime.default({module_or_path:readFileSync(new URL('../public/vector-pkg/forma_bg.wasm',import.meta.url))});

test('content tracks use the rendering font rather than character count',()=>{
 const files={'ui/Demo.ui':'component Demo { Frame { Button {} } }','components/Button.ui':"component Button { width:200; height:50; Rectangle { Frame { columns:[-,*]; Text { cell:1 1; text:'WWW'; fontSize:20; } Text { cell:1 2; text:'next'; } } } }"};
 assert.throws(()=>compileComponents(files,'ui/Demo.ui'),/метрик Rust/);
 const wide=runtime.text_metrics('WWW',20),narrow=runtime.text_metrics('iii',20);
 assert.ok(wide[0]>narrow[0]*2);
 const out=compileComponents(files,'ui/Demo.ui',{}, {measureText:runtime.text_metrics});
 const children=parse(out.template).nodes[0].children;
 assert.ok(Math.abs(children[0].props.width-wide[0])<0.0001);
 assert.ok(Math.abs(children[1].props.x-wide[0])<0.0001);
 const button=new runtime.Button();try{button.load_component(out.source,out.template);assert.ok(button.gpu_commands(1,true).length>0);}finally{button.free();}
});

test('Text with wrap: true splits long lines by word using font metrics',()=>{
 const files={
  'ui/Demo.ui':"component Demo { Frame { Text { width: 120; height: 90; wrap: true; text: 'Первая строка со словами для переноса'; fontSize: 16; } } }",
  'components/Button.ui':'component Button { Rectangle {} }',
 };
 const out=compileComponents(files,'ui/Demo.ui',{},{measureText:runtime.text_metrics});
 const texts=parse(out.template).nodes[0].children.filter(n=>n.type==='ContentText');
 assert.ok(texts.length>=2,'wrapped paragraph should produce multiple ContentText lines');
 assert.equal(texts.map(n=>n.props.text).join(' '),'Первая строка со словами для переноса');
 assert.ok(texts[1].props.y>texts[0].props.y);
});
