import {test} from 'node:test';
import assert from 'node:assert/strict';
import {parse} from '../src/language.js';
import {selectionIdentities,restoreSelection} from '../src/selection-restore.js';

// A page whose controls sit at three different depths, some addressed by a design key and some not —
// which is what an undo has to tell apart once every offset below the first edit has moved.
const page=`component Batch {
    Frame {
        key: 'shell';
        Column {
            Button { key: 'one'; text: 'Найти'; height: 36; }
            Button { text: 'Сброс'; height: 40; }
            Text { text: 'Подпись'; }
        }
        Button { key: 'last'; text: 'Ещё'; height: 44; }
    }
}`;
// One property written longer shifts every offset after it, which is the ordinary case an undo carries.
const grew=page.replace('height: 36','height: 136');
// The same control in the column's own block: a key names it wherever the file puts it.
const movedIn=page
  .replace("        Button { key: 'last'; text: 'Ещё'; height: 44; }\n",'')
  .replace("Text { text: 'Подпись'; }","Text { text: 'Подпись'; }\n            Button { key: 'last'; text: 'Ещё'; height: 44; }");
// The column stepped out of the group: the block it held is gone, the one outside it is not.
const withoutColumn=page.replace(/ +Column \{[\s\S]*?\n        \}\n/,'');
// A keyless control the file moved to another slot: a look-alike there must not answer for it.
const textMovedOut=page
  .replace('            Text { text: \'Подпись\'; }\n','')
  .replace("Button { key: 'last'; text: 'Ещё'; height: 44; }","Text { text: 'Подпись'; }\n        Button { key: 'last'; text: 'Ещё'; height: 44; }");
const tree=text=>parse(text).nodes[0];
function nodes(root){const out=[];const walk=node=>{out.push(node);(node.children??[]).forEach(walk);};walk(root);return out;}
const at=(root,type,key)=>{const found=nodes(root).find(n=>n.type===type&&(key===undefined||n.props.key===key));if(!found)throw Error(`no ${type}${key?` keyed ${key}`:''}`);return found;};
const pathOf=(root,node)=>{const walk=(n,path)=>n===node?path:(n.children??[]).map((child,i)=>walk(child,[...path,i])).find(p=>p!==null)??null;return walk(root,[]);};

test('an identity describes a control by its key or by the slot it occupies',()=>{
  const root=tree(page);
  const identities=selectionIdentities(root,[at(root,'Button','one').start,at(root,'Text').start,root.start]);
  assert.deepEqual(identities.map(id=>[id.key,id.type,id.path]),[
    ['one','Button',[0,0]],
    [null,'Text',[0,2]],
    ['shell','Frame',[]],
  ]);
  // The page root is a control the canvas can hold, so it answers to its own identity as well.
  assert.equal(pathOf(root,root).length,0);
});
test('an offset the page no longer holds gives no identity',()=>{
  const root=tree(page);
  assert.deepEqual(selectionIdentities(root,[root.children[0].start+1]),[]);
  assert.deepEqual(selectionIdentities(null,[root.start]),[],'a page that stopped compiling carries nothing');
});
test('a control survives an edit that moved every offset below it',()=>{
  const root=tree(page),column=at(root,'Column');
  const keyless=nodes(column).find(n=>n.type==='Button'&&n.props.key===undefined);
  assert.equal(keyless.props.text,'Сброс','the fixture relies on a control no key names');
  const starts=[keyless.start,at(root,'Text').start,at(root,'Button','last').start];
  const restored=restoreSelection(tree(grew),selectionIdentities(root,starts));
  assert.deepEqual(restored.nodes.map(n=>n.props.text),['Сброс','Подпись','Ещё']);
  assert.deepEqual(restored.starts,restored.nodes.map(n=>n.start));
  assert.ok(restored.starts[0]>keyless.start,'found by its slot, not by the text it had');
  assert.equal(restored.dropped,0);
});
test('a key carries a control across a move into another container',()=>{
  const root=tree(page),before=at(root,'Button','last');
  const next=tree(movedIn),after=at(next,'Button','last');
  assert.notDeepEqual(pathOf(next,after),pathOf(root,before),'the control really changed slot');
  const restored=restoreSelection(next,selectionIdentities(root,[before.start]));
  assert.deepEqual(restored.starts,[after.start]);
  assert.equal(restored.dropped,0,'a control the file still holds is the one the designer had picked');
});
test('a control the undo took out of the file is reported as gone, not replaced by its neighbour',()=>{
  const root=tree(page),column=at(root,'Column'),one=at(root,'Button','one');
  const next=tree(withoutColumn);
  assert.equal(nodes(next).some(n=>n.props.key==='one'),false,'the keyed control left with its block');
  const restored=restoreSelection(next,selectionIdentities(root,[column.start,one.start,at(root,'Button','last').start]));
  assert.deepEqual(restored.nodes.map(n=>n.props.key),['last'],'the control outside the block comes back');
  assert.equal(restored.dropped,2);
});
test('a look-alike in the slot a moved control left is not mistaken for it',()=>{
  const root=tree(page),text=at(root,'Text');
  const next=tree(textMovedOut),moved=at(next,'Text');
  assert.deepEqual(pathOf(next,moved),[1],'the text control is in the file, but not in its slot');
  const restored=restoreSelection(next,selectionIdentities(root,[text.start]));
  assert.deepEqual(restored.nodes,[],'without a key only the slot it held can answer for a control');
  assert.equal(restored.dropped,1);
});
test('the batch answers in the order it was given and holds each control once',()=>{
  const root=tree(page),column=at(root,'Column');
  const list=[at(root,'Button','last').start,column.start,at(root,'Button','one').start];
  const restored=restoreSelection(root,selectionIdentities(root,[...list,column.start]));
  assert.deepEqual(restored.starts,list,'the order the selection handed the batch over is kept');
});
test('a page that stopped compiling leaves nothing behind',()=>{
  const root=tree(page),identities=selectionIdentities(root,[root.start,at(root,'Column').start]);
  assert.deepEqual(restoreSelection(null,identities),{nodes:[],starts:[],dropped:2});
});
