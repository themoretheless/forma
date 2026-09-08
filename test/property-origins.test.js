import {propertyOrigins} from '../src/property-origins.js';
import {test} from 'node:test';
import assert from 'node:assert/strict';
import {compileComponents} from '../src/components.js';
const files={
 'components/Button.ui':`component Button {width:160;height:40;fontSize:14;text:'Save';color:#ffffff;Rectangle {Text {key:'caption';text:props.text;color:props.color;fontSize:props.fontSize;}}}`,
 'components/LargeButton.ui':`component LargeButton : Button {fontSize:18;override caption {color:#000000;}}`,
 'ui/Demo.ui':`component Demo {Frame {LargeButton {text:state.title;}}}`,
};
test('resolved property origins trace inherited defaults, keyed patches and live instance expressions',()=>{
 const out=compileComponents(files,'ui/Demo.ui',{title:'Delete'});
 const label=out.visualNodes.find(n=>n.type==='Text');
 assert.equal(label.props.text,'Delete');
 assert.ok(label.propertyOrigins.text.some(o=>o.label==='state.title'));
 assert.ok(label.propertyOrigins.text.some(o=>o.source?.file==='ui/Demo.ui'));
 assert.ok(label.propertyOrigins.fontSize.some(o=>o.source?.file==='components/LargeButton.ui'));
 assert.match(label.propertyOrigins.color[0].label,/override caption/);
 for(const origins of Object.values(label.propertyOrigins))for(const {source}of origins)if(source)assert.ok(files[source.file].slice(source.from,source.to).length);
 const instance=out.visualNodes.find(n=>n.type==='LargeButton');assert.equal(instance.props.fontSize,18);
 assert.ok(instance.propertyOrigins.text.some(o=>o.label==='state.title'));
});

test('override diagnostics suggest nearby keys and explain incorrect properties with source links',()=>{
 assert.throws(()=>compileComponents({...files,'components/LargeButton.ui':`component LargeButton : Button {override capton {fontSize:18;}}`},'ui/Demo.ui',{title:'A'}),error=>{
  assert.match(error.message,/Возможно.*caption/);assert.equal(error.diagnostic.source.file,'components/LargeButton.ui');return true;
 });
 assert.throws(()=>compileComponents({...files,'components/LargeButton.ui':`component LargeButton : Button {override caption {fontSze:18;}}`},'ui/Demo.ui',{title:'A'}),error=>{
  assert.match(error.message,/Text.*fontSze.*fontSize/);assert.equal(error.diagnostic.related.file,'components/Button.ui');return true;
 });
});

test('origin traversal restores caller cycle guards and retains diamond dependencies',()=>{
 const source={file:'x.ui',from:1,to:2},seen=new Set(['root']);
 const props={a:{expr:'props.b'},b:{expr:'props.a'}};
 const result=propertyOrigins({expr:'props.a'},source,props,{a:source,b:source},{},seen);
 assert.deepEqual(seen,new Set(['root']));assert.equal(result.length,1);
 const value={parts:[{expr:'props.a'},{expr:'props.b'}]};
 assert.deepEqual(propertyOrigins(value,null,{a:{expr:'props.c'},b:{expr:'props.c'},c:{expr:'state.value'}},{},{},seen),[{label:'state.value'}]);
 assert.deepEqual(seen,new Set(['root']));
});
