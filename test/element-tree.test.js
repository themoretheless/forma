import {test} from 'node:test';
import assert from 'node:assert/strict';
import {parse} from '../src/language.js';
import {attachSources,createElementTree} from '../src/element-tree.js';
import {compileComponents} from '../src/components.js';

test('snapshot IDs are distinct from keys and UTF-16 source offsets',()=>{
 const source="component Demo { Frame { Text { key:'same'; text:'😀'; } Frame { Text { key:'other'; } } } }";
 const ast=attachSources(parse(source),'Demo.ui');
 // Repeated template keys can occur in different expanded instances; the parser
 // still rejects duplicates within one source component.
 ast.nodes[0].children[1].children[0].props.key='same';
 const tree=createElementTree(ast.nodes);
 assert.deepEqual(tree.roots,[0]);
 assert.deepEqual(tree.nodes.map(n=>n.parent),[null,0,0,2]);
 assert.deepEqual(tree.nodes.filter(n=>n.key==='same').map(n=>n.id),[1,3]);
 for(const node of tree.nodes)assert.ok(source.slice(node.source.from,node.source.to).startsWith(node.type));
 ast.nodes[0].children[0].props.text='changed';
 assert.equal(tree.nodes[1].props.text,'😀');
 assert.throws(()=>tree.nodes[0].children.push(99),TypeError);
});

const files={
 'Demo.ui':"component Demo { Frame { Fancy { key:'search'; text:'Найти'; } } }",
 'components/Button.ui':"component Button { Rectangle { Border { key:'outline'; width:1; background:#111111; } Text { text:props.text; } PointerArea { clicked -> events.clicked(); } } }",
 'components/Fancy.ui':"component Fancy : Button { override outline from '../styles/Outline.ui' { width:3; }; }",
 'styles/Outline.ui':"Border { width:2; background:#abcdef; }",
};
test('linker preserves separate source trees and per-property override origins',()=>{
 const compiled=compileComponents(files,'Demo.ui');
 const instance=compiled.instanceTree.nodes[1];
 assert.equal(instance.type,'Fancy');assert.equal(instance.key,'search');
 assert.equal(instance.source.file,'Demo.ui');
 const border=compiled.templateTree.nodes.find(n=>n.type==='Border');
 assert.equal(border.key,'outline');assert.equal(border.source.file,'components/Button.ui');
 assert.equal(border.props.width,3);
 for(const [key,file,value] of [['width','components/Fancy.ui','3'],['background','styles/Outline.ui','#abcdef']]){
  const origin=border.propertySources[key];assert.equal(origin.file,file);
  assert.equal(files[file].slice(origin.from,origin.to),value);
 }
 assert.doesNotMatch(compiled.template,/key:/);
 assert.equal(compiled.templateTree.nodes.find(n=>n.type==='Text').props.text,'Найти');
});

test('conditional replacement records the selected node defining file',()=>{
 const f={...files,
  'components/Button.ui':"component Button { Rectangle { ContentPresenter { key:'content'; Text { text:'Base'; } } } }",
  'components/Fancy.ui':"component Fancy : Button { override content: match state.loading { true => Text { text:'Wait'; }; false => base.content; }; }",
 };
 for(const [loading,file,text] of [[true,'components/Fancy.ui','Wait'],[false,'components/Button.ui','Base']]){
  const tree=compileComponents(f,'Demo.ui',{loading}).templateTree;
  const node=tree.nodes.find(n=>n.type==='Text');
  assert.equal(node.source.file,file);assert.equal(node.props.text,text);
 }
});
