import {test} from 'node:test';
import assert from 'node:assert/strict';
import {EditorState} from '@codemirror/state';
import {editorFeatures,languageFor} from '../src/editor-features.js';
import {formaHighlight} from '../src/forma-highlight.js';

test('markup files get the Forma language and everything else plain text',()=>{
 assert.equal(languageFor('ui/Window.ui'),formaHighlight);
 assert.deepEqual(languageFor('src/actions.rs'),[]);
 assert.deepEqual(languageFor(undefined),[]);
});
test('the lazily loaded feature set is a valid extension list',()=>{
 const extensions=editorFeatures(()=>({files:{'ui/A.ui':'component A {}'},path:'ui/A.ui'}));
 assert.ok(Array.isArray(extensions)&&extensions.length>0);
 const state=EditorState.create({doc:'component A {}',extensions:[extensions,languageFor('ui/A.ui')]});
 assert.equal(state.doc.toString(),'component A {}');
});
