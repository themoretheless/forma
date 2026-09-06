import {test} from 'node:test';
import assert from 'node:assert/strict';
import {parse} from '../src/language.js';
import {buildControlTree,treeRows} from '../src/control-tree.js';

const path='ui/Demo.ui';
const source="component Demo { Frame { Text { key: 'title'; text: 'Header'; } Frame { Button { text: 'Search'; } } } }";
test('source hierarchy retains nesting, key labels and accurate source offsets',()=>{
 const roots=buildControlTree(parse(source),path),rows=treeRows(roots);
 assert.deepEqual(rows.map(r=>r.level),[1,2,3,3,4]);
 assert.deepEqual(rows.map(r=>r.label),['Demo','Frame','Text','Frame','Button']);
 assert.equal(rows[2].detail,'#title');assert.equal(rows[4].detail,'Search');
 for(const row of rows.filter(r=>r.node)){assert.equal(source.slice(row.node.start,row.node.start+row.node.type.length),row.node.type);assert.equal(row.path,path);}
});
test('folding hides descendants without removing siblings, and filter keeps ancestors',()=>{
 const roots=buildControlTree(parse(source),path),rows=treeRows(roots);const collapsed=new Set([rows[3].id]);
 assert.deepEqual(treeRows(roots,collapsed).map(r=>r.label),['Demo','Frame','Text','Frame']);
 assert.deepEqual(treeRows(roots,collapsed,'search').map(r=>r.label),['Demo','Frame','Frame','Button']);
 assert.equal(treeRows(roots,collapsed,'absent').length,0);
});
test('template view includes override branches but does not pretend they are active runtime nodes',()=>{
 const source="component ImageButton : Button { override content: match props.compact { true => Image { source: 'icon.svg'; }; false => base.content; }; }";
 const rows=treeRows(buildControlTree(parse(source),'components/ImageButton.ui'));
 assert.equal(rows[0].label,'ImageButton : Button');assert.match(rows[1].label,/override content: · match props.compact/);
 assert.equal(rows[2].label,'true ⇒ Image');assert.ok(rows[2].node.start>0);assert.match(rows[3].label,/false ⇒ base.content/);assert.equal(rows[3].node,null);
});
test('resource brushes are not controls and typed content nodes are shown',()=>{
 const rows=treeRows(buildControlTree(parse("component Button { Rectangle { background: Brush { color: #ffffff; }; ContentPresenter { key: 'content'; content: Frame { Text { text: 'Hi'; } }; } } }"),'components/Button.ui'));
 assert.deepEqual(rows.map(r=>r.label),['Button','Rectangle','ContentPresenter','content: Frame','Text']);
});
test('structural ids survive property edits and distinguish identical controls',()=>{
 const before=treeRows(buildControlTree(parse(source),path));const after=treeRows(buildControlTree(parse(source.replace('Header','A much longer heading')),path));
 assert.deepEqual(before.map(r=>r.id),after.map(r=>r.id));assert.equal(new Set(before.map(r=>r.id)).size,before.length);
});
