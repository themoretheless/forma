/** @vitest-environment jsdom */
import { beforeEach, expect, it } from 'vitest';
import { compileFormaAst, getGraphTelemetry, parseFormaSource, resetReactiveSystem } from '../src/engine';
beforeEach(() => resetReactiveSystem());
const source = `component App { state count: Number = 1; Column { Text { text:state.count; } } }`;
const changes: Array<[string, (a: any) => void]> = [
 ['missing element tag', a => delete a.rootAstNodes[0].tag],
 ['empty element tag', a => a.rootAstNodes[0].tag = ' '],
 ['missing if condition', a => a.rootAstNodes[0].kind = 'if'],
 ['missing for item', a => {a.rootAstNodes[0].kind = 'for';a.rootAstNodes[0].listExpr = 'state.items';}],
 ['missing for list', a => {a.rootAstNodes[0].kind = 'for';a.rootAstNodes[0].itemVar = 'item';}],
 ['missing match expression', a => {a.rootAstNodes[0].kind = 'match';a.rootAstNodes[0].matchArms = [];}],
 ['missing match arms', a => {a.rootAstNodes[0].kind = 'match';a.rootAstNodes[0].matchExpr = 'state.count';}],
 ['missing slot name', a => a.rootAstNodes[0].kind = 'slot'],
 ['empty key expression', a => a.rootAstNodes[0].keyExpr = ' '],
 ['unknown kind', a => a.rootAstNodes[0].kind = 'mystery'],
 ['missing children', a => delete a.rootAstNodes[0].children],
 ['non-string property', a => a.rootAstNodes[0].properties.width = 12],
 ['null root', a => a.rootAstNodes = [null]],
 ['undefined root', a => a.rootAstNodes = [undefined]],
 ['empty branch cycle', a => a.rootAstNodes[0].emptyChildren = a.rootAstNodes],
 ['sparse roots', a => a.rootAstNodes = new Array(1)],
 ['sparse styles', a => a.rootAstNodes[0].appliedStyles = new Array(1)],
 ['non-array roots', a => a.rootAstNodes = {}],
 ['direct cycle', a => a.rootAstNodes[0].children.push(a.rootAstNodes[0])],
 ['indirect cycle', a => a.rootAstNodes[0].children[0].children.push(a.rootAstNodes[0])],
 ['else cycle', a => a.rootAstNodes[0].elseChildren = a.rootAstNodes],
 ['match cycle', a => a.rootAstNodes[0].matchArms = [{pattern:'_',propertyAssignments:{},childNodes:a.rootAstNodes}]],
];
for(const [name,change] of changes) it(`rejects ${name} before construction or live document mutation`,()=>{
 const ast=parseFormaSource(source);change(ast);
 expect(()=>compileFormaAst(ast)).toThrow(/AST.*tree/i);expect(getGraphTelemetry().nodes).toBe(0);
 const inst=compileFormaAst(parseFormaSource(source)),container=document.createElement('div'),handle=inst.mount(container);
 const old=container.firstChild,nodes=getGraphTelemetry().nodes;
 try{
  expect(()=>inst.hotReload(ast)).toThrow(/AST.*tree/i);
  expect(container.firstChild).toBe(old);expect(getGraphTelemetry().nodes).toBe(nodes);
  inst.state.count.set(2);expect(container.textContent).toBe('2');
 }finally{handle.unmount();inst.dispose();}
 expect(getGraphTelemetry().nodes).toBe(0);
});
it('permits a shared acyclic child and renders both occurrences',()=>{
 const ast=parseFormaSource(source);ast.rootAstNodes[0].children.push(ast.rootAstNodes[0].children[0]);
 const inst=compileFormaAst(ast),container=document.createElement('div'),handle=inst.mount(container);
 try{expect(container.textContent).toBe('11');inst.state.count.set(2);expect(container.textContent).toBe('22');}
 finally{handle.unmount();inst.dispose();}expect(getGraphTelemetry().nodes).toBe(0);
});

it('admits a deep acyclic tree without recursive validation',()=>{
 const ast=parseFormaSource(source), template=ast.rootAstNodes[0];let cursor=template;
 for(let i=0;i<20000;i++){const child={...template,children:[]};cursor.children=[child];cursor=child;}
 const inst=compileFormaAst(ast);inst.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('renders wrapped Text, multiline TextInput, and two-way Slider in DOM',()=>{
 const ui=`component App {
  state note: String = 'line1\\nline2';
  state level: Number = 42;
  Column {
   Text { key: 'wrapped'; text: 'Soft wrapped text'; wrap: true; fontSize: 18; }
   TextInput { key: 'notes'; multiline: true; readOnly: true; value <-> state.note; }
   Slider { key: 'slider'; min: 0; max: 100; value <-> state.level; }
  }
 }`;
 const inst=compileFormaAst(parseFormaSource(ui)),container=document.createElement('div'),handle=inst.mount(container);
 try{
  const wrapped=inst.getElementByKey('wrapped')!;
  expect(wrapped.style.whiteSpace).toBe('pre-wrap');
  expect(wrapped.style.overflowWrap).toBe('break-word');
  expect(wrapped.style.fontSize).toBe('18px');
  const notes=inst.getElementByKey('notes') as HTMLTextAreaElement;
  expect(notes.tagName).toBe('TEXTAREA');
  expect(notes.readOnly).toBe(true);
  expect(notes.value).toBe('line1\nline2');
  const slider=inst.getElementByKey('slider') as HTMLInputElement;
  expect(slider.tagName).toBe('INPUT');
  expect(slider.type).toBe('range');
  expect(slider.value).toBe('42');
  slider.value='75';
  slider.dispatchEvent(new Event('input'));
  expect(inst.state.level()).toBe(75);
 }finally{handle.unmount();inst.dispose();}
 expect(getGraphTelemetry().nodes).toBe(0);
});
