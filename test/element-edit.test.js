import {test} from 'node:test';
import assert from 'node:assert/strict';
import {parse} from '../src/language.js';
import {moveElement,copyElement,insertElement,removeElement,gridCell,reorderElement,locateElement,resizeElement,resizeBlock,moveAmongSiblings,moveBlock,moveIntoContainer} from '../src/element-edit.js';
const apply=(s,c)=>s.slice(0,c.from)+c.insert+s.slice(c.to);
const sample="component Test { Frame { width: 400; Button { key: 'a'; text: 'a'; x: 10; y: 20; clicked -> actions.save(); } Button { key: 'a_2'; } } }";
const first=s=>parse(s).nodes[0].children[0];
test('move preserves events and labels, adds missing coordinates and clamps to origin',()=>{
 let n=first(sample),s=apply(sample,moveElement(sample,n.start,5,-30));
 assert.equal(first(s).props.x,15);assert.equal(first(s).props.y,0);assert.equal(first(s).events.clicked,'actions.save');
 n=parse(s).nodes[0].children[1];s=apply(s,moveElement(s,n.start,3,4));assert.equal(parse(s).nodes[0].children[1].props.y,4);
});
test('copy/paste renames collisions while preserving strings and events; deletion removes exactly one node',()=>{
 const text=copyElement(sample,first(sample).start);let s=apply(sample,insertElement(sample,first(sample).start,text));
 let ns=parse(s).nodes[0].children;assert.deepEqual(ns.map(n=>n.props.key),['a','a_3','a_2']);assert.equal(ns[1].props.text,'a');assert.equal(ns[1].events.clicked,'actions.save');
 s=apply(s,removeElement(s,ns[1].start));assert.equal(parse(s).nodes[0].children.length,2);
 assert.throws(()=>removeElement(s,parse(s).nodes[0].start),/Корневой/);
});
test('a duplicate reports the slot it took and a delete hands the container back',()=>{
 const page="component Test {\n  Frame {\n    Button { key: 'a'; }\n  }\n}";
 const root=s=>parse(s).nodes[0];
 // The bridge's duplicate is the copied text spliced back after the node, so the copy's own
 // offset is the only handle a caller has on it, and a delete must name what stays selected.
 const dup=insertElement(page,root(page).children[0].start,copyElement(page,root(page).children[0].start),false,'after');
 const added=apply(page,dup);
 assert.deepEqual(parse(added).nodes[0].children.map(n=>n.props.key),['a','a_2']);
 assert.equal(added.slice(dup.start,dup.start+6),'Button','the reported offset is the copy');
 assert.equal(dup.start,parse(added).nodes[0].children[1].start);
 const removed=removeElement(added,parse(added).nodes[0].children[1].start);
 // Like the designer's Delete key, the splice takes the node's text and leaves the slot's
 // whitespace behind, so the check is the tree, not the bytes.
 assert.deepEqual(parse(apply(added,removed)).nodes[0].children.map(n=>n.props.key),['a']);
 assert.equal(removed.start,root(added).start,'the container keeps the selection');
});
test('Grid movement preserves cell notation and does not introduce absolute coordinates',()=>{
 const s='component Test { Frame { columns: [100, *]; rows: [40, *]; Button { cell: [1, 1]; } } }';
 const result=apply(s,moveElement(s,first(s).start,100,0,{row:2,column:2}));assert.deepEqual(first(result).props.cell,[2,2]);assert.equal(first(result).props.x,undefined);
 assert.deepEqual(gridCell({bounds:[20,20,300,200],columns:[100,190],rows:[40,150],gap:[10,10]},160,85),{row:2,column:2});
});
test('expressions and malformed clipboard content fail without damaging source',()=>{
 const s='component Test { Frame { Button { x: state.x; } } }';assert.throws(()=>moveElement(s,first(s).start,1,0),/выражением/);
 assert.throws(()=>insertElement(sample,first(sample).start,'Button {} Button {}'),/один контрол/);
 assert.throws(()=>insertElement(sample,first(sample).start,'} } component Bad { Frame {}'),Error);
});
test('moving an implicit flow position starts at the rendered position',()=>{
 const s='component Test { Frame { Button {} } }';const result=apply(s,moveElement(s,first(s).start,5,10,null,[24,140]));assert.equal(first(result).props.x,29);assert.equal(first(result).props.y,150);
});

test('reorder swaps siblings, retains exact source and selection, and stops at boundaries',()=>{
 const nodes=parse(sample).nodes[0].children;
 const change=reorderElement(sample,nodes[1].start,-1),next=apply(sample,change),ordered=parse(next).nodes[0].children;
 assert.deepEqual(ordered.map(n=>n.props.key),['a_2','a']);assert.equal(ordered[0].start,change.start);
 assert.equal(ordered[1].events.clicked,'actions.save');
 assert.equal(reorderElement(next,ordered[0].start,-1),null);
 const back=reorderElement(next,ordered[0].start,1);assert.equal(apply(next,back),sample);assert.equal(back.start,nodes[1].start);
 assert.equal(reorderElement(sample,nodes[1].start,1),null);
});
test('reorder remains within Scroll and preserves Grid cell properties',()=>{
 const s="component Test { Frame { Scroll { Button { key: 'a'; cell: [1, 1]; } Button { key: 'b'; cell: [1, 2]; } } } }";
 const scroll=parse(s).nodes[0].children[0],change=reorderElement(s,scroll.children[0].start,1);
 const next=parse(apply(s,change)).nodes[0].children[0];assert.deepEqual(next.children.map(n=>n.props.cell),[[1,2],[1,1]]);
 assert.throws(()=>reorderElement(s,scroll.start,1),/дочерний/);
});
test('the palette names where the new node goes, and paste keeps its own rule',()=>{
 const s='component Test { Row { Text { text: \'a\'; } Column { Text { text: \'b\'; } } } }';
 const row=parse(s).nodes[0],[text,column]=row.children,box='Rectangle { width: 4; height: 4; }';
 assert.deepEqual(parse(apply(s,insertElement(s,row.start,box,false,'inside'))).nodes[0].children.map(n=>n.type),['Text','Column','Rectangle']);
 assert.deepEqual(parse(apply(s,insertElement(s,text.start,box,false,'after'))).nodes[0].children.map(n=>n.type),['Text','Rectangle','Column']);
 assert.deepEqual(parse(apply(s,insertElement(s,column.start,box,false,'inside'))).nodes[0].children[1].children.map(n=>n.type),['Text','Rectangle']);
 assert.throws(()=>insertElement(s,text.start,box,false,'inside'),/только контейнер/);
 assert.throws(()=>insertElement(s,row.start,box,false,'after'),/нет соседа/);
 // Duplicate and paste select a node, not a destination: only Frame and Scroll take a child,
 // so pasting while a Row is selected still produces a sibling Row.
 const page='component Test { Frame { Row { Text { text: \'a\'; } } Button { key: \'b\'; } } }';
 const flow=parse(page).nodes[0].children[0];
 assert.deepEqual(parse(apply(page,insertElement(page,flow.start,"Row { }"))).nodes[0].children.map(n=>n.type),['Row','Row','Button']);
});
test('an inserted control takes the indentation of the block it joins',()=>{
 const page="component Page {\n  Frame {\n    Scroll {\n      Button { key: 'a'; }\n    }\n    Row { gap: 4; }\n  }\n}\n";
 const block=source=>{const [frame]=parse(source).nodes;return {scroll:frame.children[0],row:frame.children[1]};};
 let change=insertElement(page,block(page).scroll.children[0].start,"Badge { }",false,'after'),next=apply(page,change);
 assert.equal(next.split('\n')[4],'      Badge { }');assert.equal(next.slice(change.start,change.start+9),'Badge { }');
 change=insertElement(next,block(next).row.start,"Text { text: 'x'; }",false,'inside');next=apply(next,change);
 assert.ok(next.includes("Row { gap: 4; Text { text: 'x'; } }"),'a one-line container keeps its shape');
 assert.equal(next.slice(change.start,change.start+4),'Text');
 // A copy arrives with the column of its own line, so the block shifts as a whole and the brace keeps its line.
 change=insertElement(next,block(next).scroll.start,"Rectangle {\n        width: 40;\n      }",false,'inside');next=apply(next,change);
 assert.deepEqual(next.split('\n').slice(3,9),["      Button { key: 'a'; }",'      Badge { }','      Rectangle {','        width: 40;','      }','    }']);
 assert.equal(next.slice(change.start,change.start+9),'Rectangle');assert.deepEqual(change.starts,[change.start]);
});
test('a dragged edge writes the size the control renders and anchors the opposite edge',()=>{
 const s="component Test { Frame { width: 400; Button { key: \'a\'; x: 40; y: 20; width: 60; height: 24; } } }";
 const n=first(s),change=resizeElement(s,n.start,{left:0,right:20,top:0,bottom:-4},[n.props.x,n.props.y,n.props.width,n.props.height]);
 let next=apply(s,change);assert.deepEqual([first(next).props.width,first(next).props.height,first(next).props.x],[80,20,40]);
 // The grabbed edge follows the pointer, so a leading edge carries the coordinate with it.
 const m=first(next);next=apply(next,resizeElement(next,m.start,{left:-10,right:0,top:0,bottom:0},[m.props.x,m.props.y,m.props.width,m.props.height]));
 assert.deepEqual([first(next).props.x,first(next).props.width],[30,90]);assert.equal(first(next).props.height,20);
});
test('a drag sizes an implicit box by what the canvas shows and stays inside the declared bounds',()=>{
 const s="component Test { Frame { width: 400; Button { key: \'a\'; x: 10; y: 20; } } }";
 const grown=apply(s,resizeElement(s,first(s).start,{left:0,right:20,top:0,bottom:10},[10,20,100,40]));
 assert.deepEqual([first(grown).props.width,first(grown).props.height,first(grown).props.x],[120,50,10]);
 const tight="component Test { Frame { width: 400; Button { key: \'a\'; width: 60; height: 24; minWidth: 40; } } }";
 assert.equal(first(apply(tight,resizeElement(tight,first(tight).start,{left:0,right:-50,top:0,bottom:0},[0,0,60,24]))).props.width,40);
 assert.equal(first(apply(s,resizeElement(s,first(s).start,{left:0,right:-140,top:0,bottom:0},[10,20,100,40]))).props.width,1);
});
test('a size the layout owns elsewhere refuses the drag',()=>{
 const page='component Test { Frame { width: 400; Button { width: 60; height: 24; } } }';
 const root=locateElement(page,parse(page).nodes[0].start);
 assert.equal(resizeBlock(root.node,root.parent),'Изменяйте размер дочернего контрола');
 assert.throws(()=>resizeElement(page,root.node.start,{left:0,right:1,top:0,bottom:0},[0,0,400,300]),/дочернего/);
 const boxed='component Test { Frame { Scroll { Button { width: 60; height: 24; } } } }';
 const scroll=locateElement(boxed,first(boxed).start);assert.equal(resizeBlock(scroll.node,scroll.parent),'Изменяйте размер дочернего контрола');
 const grid='component Test { Frame { columns: [100, *]; rows: [40, *]; Button { cell: [1, 1]; width: 60; height: 24; } } }';
 const cell=locateElement(grid,first(grid).start);assert.match(resizeBlock(cell.node,cell.parent),/треки/);
 const flexed="component Test { Row { Button { width: '2*'; height: 24; } } }";
 const row=locateElement(flexed,first(flexed).start);assert.match(resizeBlock(row.node,row.parent),/весом/);
 const bound='component Test { Frame { Button { width: state.w; height: 24; } } }';
 const ref=locateElement(bound,first(bound).start);assert.match(resizeBlock(ref.node,ref.parent),/выражением/);
});

test('a move is refused where the layout places the control itself',()=>{
 const leaf=(s,key)=>{let found;const walk=nodes=>{for(const n of nodes){if(n.props.key===key)found=n;walk(n.children??[]);}};walk(parse(s).nodes);return found;};
 const movable=(s,{cell}={})=>{const node=leaf(s,'a');return moveBlock(node,locateElement(s,node.start).parent,cell);};
 const stacked="component Test { Frame { Stack { width: 200; height: 120; Button { key: 'a'; width: 60; height: 24; } } } }";
 assert.equal(movable(stacked),'В Stack положение задаёт раскладка: перенос недоступен');
 assert.throws(()=>moveElement(stacked,leaf(stacked,'a').start,5,5,null,[10,10]),/раскладка/);
 assert.equal(leaf(stacked,'a').props.x,undefined,'a refused move leaves no coordinate behind');
 const bare="component Test { Frame { Grid { Button { key: 'a'; width: 60; height: 24; } } } }";
 assert.match(movable(bare),/Grid положение задаёт раскладка/,'tracks of their own decide a Grid child too');
 const grid="component Test { Frame { columns: [100, *]; rows: [40, *]; Button { key: 'a'; cell: [1, 1]; width: 60; height: 24; } } }";
 assert.match(movable(grid),/целевую ячейку/,'a tracks child moves by the cell a drag is aimed at');
 assert.equal(movable(grid,{cell:{row:2,column:2}}),null,'the cell a drag lands on is the move');
 assert.equal(movable("component Test { Frame { Column { Button { key: 'a'; width: 60; height: 24; } } } }"),null,'a flow container reads the coordinate back');
});

test('a drop moves a control to any slot of its sibling run and reports where it landed',()=>{
 const page="component Test {\n  Frame {\n    Button { key: 'a'; }\n    Row { gap: 4; }\n    Text { text: 'x'; }\n  }\n}\n";
 const kids=s=>parse(s).nodes[0].children,types=s=>kids(s).map(n=>n.type);
 let change=moveAmongSiblings(page,kids(page)[2].start,kids(page)[0].start,'before'),next=apply(page,change);
 assert.deepEqual(types(next),['Text','Button','Row']);assert.equal(next.slice(change.start,change.start+4),'Text');assert.equal(change.start,kids(next)[0].start);
 change=moveAmongSiblings(next,kids(next)[0].start,kids(next)[2].start,'after');next=apply(next,change);
 assert.deepEqual(types(next),['Button','Row','Text']);
 assert.equal(next,page,'two slots back the block is byte for byte the source it started from');
});
test('a moved control keeps its own text while the whitespace stays in the slot',()=>{
 const page="component Test {\n  Frame {\n    Button { key: 'a'; }\n\n    Text {\n      text: 'x';\n    }\n  }\n}\n";
 const kids=parse(page).nodes[0].children,next=apply(page,moveAmongSiblings(page,kids[1].start,kids[0].start,'before'));
 assert.deepEqual(next.split('\n').slice(2,7),["    Text {","      text: 'x';","    }","","    Button { key: 'a'; }"]);
 assert.equal(next.split('\n')[1],'  Frame {','the run keeps its own indentation');
});
test('a drop outside the sibling run is refused and the slot it stands in changes nothing',()=>{
 const page="component Test { Frame { Button { key: 'a'; } Column { Text { key: 'deep'; } } Row { } } }";
 const kids=parse(page).nodes[0].children,deep=kids[1].children[0];
 assert.equal(moveAmongSiblings(page,kids[2].start,kids[2].start,'before'),null);
 assert.throws(()=>moveAmongSiblings(page,deep.start,kids[0].start,'before'),/соседей/);
 assert.throws(()=>moveAmongSiblings(page,kids[0].start,deep.start,'before'),/соседей/);
 assert.throws(()=>moveAmongSiblings(page,kids[0].start,kids[2].start,'inside'),/до или после/);
 assert.throws(()=>moveAmongSiblings(page,parse(page).nodes[0].start,kids[0].start,'before'),/дочерний/);
 // A Scroll inside a container is an ordinary child here, unlike the page scroller on the canvas.
 const boxed='component Test { Frame { Scroll { } Rectangle { width: 4; } } }';
 const nested=parse(boxed).nodes[0].children;
 assert.deepEqual(types(apply(boxed,moveAmongSiblings(boxed,nested[0].start,nested[1].start,'after'))),['Rectangle','Scroll']);
 function types(s){return parse(s).nodes[0].children.map(n=>n.type);}
});

// A control that leaves one container for another travels as its own text: the block it came from
// loses no leftover line, and the block it joins decides both the indentation and the corner its
// coordinate counts from.
const byType=(s,type)=>{let found;const walk=nodes=>{for(const n of nodes){if(!found&&n.type===type)found=n;walk(n.children??[]);}};walk(parse(s).nodes);return found;};
const panelPage="component Test {\n  Frame {\n    Button { key: 'a'; x: 10; y: 20; width: 40; height: 20; clicked -> actions.save(); }\n    Column { gap: 4; Text { text: 'inside'; } }\n  }\n}\n";
test('a drop onto another container hands the control over with the point that container reads',()=>{
 const change=moveIntoContainer(panelPage,byType(panelPage,'Button').start,byType(panelPage,'Column').start,[7,3]);
 const next=apply(panelPage,change);
 assert.deepEqual(next.split('\n'),["component Test {","  Frame {","    Column { gap: 4; Text { text: 'inside'; } Button { key: 'a'; x: 7; y: 3; width: 40; height: 20; clicked -> actions.save(); } }","  }","}",""]);
 const moved=byType(next,'Button');
 assert.equal(moved.props.key,'a','the control keeps its own key: nothing collided and nothing was renamed');
 assert.equal(moved.events.clicked,'actions.save');
 assert.deepEqual([moved.props.x,moved.props.y],[7,3],'the coordinate counts from the container it entered');
 assert.equal(next.slice(change.start,change.start+6),'Button','the reported offset names the moved control');
});
test('a container moves with everything inside it and leaves no empty line behind',()=>{
 const page="component Test {\n  Frame {\n    Row {\n      Button { key: 'a'; }\n    }\n    Column { gap: 4; }\n  }\n}\n";
 const next=apply(page,moveIntoContainer(page,byType(page,'Row').start,byType(page,'Column').start,null));
 assert.deepEqual(parse(next).nodes[0].children.map(n=>n.type),['Column']);
 assert.deepEqual(byType(next,'Column').children.map(n=>n.type),['Row']);
 assert.deepEqual(byType(next,'Row').children.map(n=>n.props.key),['a'],'the subtree travels whole');
 assert.equal(next.includes('\n\n'),false,'the run it left keeps one separator per pair of children');
});
test('a panel that places its children itself is handed the control without a point of its own',()=>{
 const page="component Test { Frame { Button { key: 'a'; x: 5; y: 6; } Stack { width: 100; height: 50; Text { text: 'x'; } } } }";
 const next=apply(page,moveIntoContainer(page,byType(page,'Button').start,byType(page,'Stack').start,null));
 assert.deepEqual(parse(next).nodes[0].children.map(n=>n.type),['Stack']);
 assert.deepEqual([byType(next,'Button').props.x,byType(next,'Button').props.y],[5,6],'the coordinate rides along untouched');
});
test('a coordinate the designer bound to an expression stays code after a move',()=>{
 const page="component Test { Frame { Button { x: state.w; y: 4; } Column { gap: 4; } } }";
 const next=apply(page,moveIntoContainer(page,byType(page,'Button').start,byType(page,'Column').start,[9,9]));
 assert.ok(next.includes('x: state.w;'),'a bound coordinate is not a number to overwrite');
 assert.ok(next.includes('y: 9;'),'the axis nothing binds takes the drop point');
});
test('a container cannot host itself or its own panel, and a leaf has no room for a child',()=>{
 const page="component Test { Frame { Column { Row { Button { key: 'a'; } } } Text { text: 'x'; } } }";
 const root=parse(page).nodes[0],row=byType(page,'Row'),column=byType(page,'Column'),text=byType(page,'Text');
 assert.throws(()=>moveIntoContainer(page,column.start,row.start),/потомок/,'a panel cannot be put inside what it holds');
 assert.throws(()=>moveIntoContainer(page,row.start,row.start),/самого себя/);
 assert.throws(()=>moveIntoContainer(page,byType(page,'Button').start,text.start),/только контейнер/);
 assert.throws(()=>moveIntoContainer(page,root.start,column.start),/дочерний/);
});
