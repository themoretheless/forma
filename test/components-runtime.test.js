import {test} from 'node:test';
import assert from 'node:assert/strict';
import {existsSync,readFileSync} from 'node:fs';
import {compileComponents} from '../src/components.js';

test('linked inheritance renders all branches through the actual Rust/WASM core', {skip:!existsSync(new URL('../public/vector-pkg/forma_vector.js',import.meta.url))},async()=>{
 const runtime=await import('../public/vector-pkg/forma_vector.js');
 await runtime.default({module_or_path:readFileSync(new URL('../public/vector-pkg/forma_vector_bg.wasm',import.meta.url))});
 const read=name=>readFileSync(new URL('../vector-ui/examples/'+name,import.meta.url),'utf8');
 const files={'components/Button.ui':read('Button.slots.ui'),'components/ImageButton.ui':read('ImageButton.component.ui'),'ui/Demo.ui':read('ImageButton.ui'),'assets/search.svg':read('search.svg')};
 const load=f=>{const out=compileComponents(f,'ui/Demo.ui');const b=new runtime.Button();b.load_component(out.source,out.template);return b;};
 const normal=load(files);const compact=load({...files,'ui/Demo.ui':files['ui/Demo.ui'].replace('compact: false','compact: true')});const base=load({...files,'ui/Demo.ui':files['ui/Demo.ui'].replace('showIcon: true','showIcon: false')});
 try {
  const a=normal.pixels(400,200,1),b=compact.pixels(400,200,1),c=base.pixels(400,200,1);
  assert.equal(a.length,400*200*4);assert.notDeepEqual(a,b);assert.notDeepEqual(a,c);assert.notDeepEqual(b,c);
  assert.equal(normal.action(),'actions.search');normal.activate();normal.focus(true);compact.preserve_interaction(normal);assert.equal(compact.clicks(),1);assert.equal(compact.is_focused(),true);
  compact.activate();assert.equal(compact.clicks(),2);assert.equal(compact.label(),'Найти документы');
 } finally {normal.free();compact.free();base.free();}
});
