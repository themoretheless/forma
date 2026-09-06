import {test} from 'node:test';
import assert from 'node:assert/strict';
import {parse} from '../src/language.js';
import {parser} from '../src/forma-parser.js';
test('Brush is a typed property value, including nested Border brush',()=>{
 const source="component Button { Rectangle { background: Brush { color: props.background; focus: #ffffff; transition: 140ms; }; Border { width: 1; background: Brush { color: #bed0ff; }; } } }";
 const root=parse(source).nodes[0];assert.equal(root.props.background.type,'Brush');assert.equal(root.props.background.props.focus.expr,'#ffffff');assert.equal(root.children[0].props.background.type,'Brush');
 const errors=[];parser.parse(source).iterate({enter:n=>{if(n.type.isError)errors.push(n.from);}});assert.deepEqual(errors,[]);
 assert.throws(()=>parse(source.replace('140ms; };','140ms; }')));
});
