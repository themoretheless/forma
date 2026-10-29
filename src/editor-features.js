import {autocompletion,acceptCompletion,closeBrackets,closeBracketsKeymap,completionKeymap,snippet} from '@codemirror/autocomplete';
import {defaultKeymap,historyKeymap} from '@codemirror/commands';
import {EditorState} from '@codemirror/state';
import {crosshairCursor,drawSelection,dropCursor,highlightActiveLine,highlightActiveLineGutter,highlightSpecialChars,keymap,lineNumbers,rectangularSelection} from '@codemirror/view';
import {bracketMatching,foldGutter,foldKeymap,indentOnInput,syntaxHighlighting,defaultHighlightStyle} from '@codemirror/language';
import {highlightSelectionMatches,searchKeymap} from '@codemirror/search';
import {createCompletionSource} from './completion.js';
import {formaHighlight} from './forma-highlight.js';
import {tokenizerHighlight} from './tokenizer-highlight.js';

// Everything the code editor needs only once someone reads or types in it: gutters, keymaps,
// completion, search, bracket and syntax highlighting with the Forma parser. It loads as its own
// chunk after start-up; until then the editor keeps its text, history and folds without it.
//
// The same list the `codemirror` package ships as basicSetup, spelled out so the meta package can
// stay out of the bundle. Its lint keymap is dropped: no linter is installed here, so the two dead
// bindings were the only thing pulling @codemirror/lint into the first load. History and fold
// state live in the editor itself, because edits and folds happen before this chunk arrives.
export function editorFeatures(getProject){
  const completion=createCompletionSource(getProject);
  const complete=context=>{const result=completion(context);if(!result)return null;return {...result,options:result.options.map(item=>typeof item.apply==='string'&&item.apply.includes('{\n')?{...item,apply:snippet(item.apply.replace('    \n','    ${}\n'))}:item)};};
  return [
    lineNumbers(),highlightActiveLineGutter(),highlightSpecialChars(),foldGutter(),drawSelection(),dropCursor(),EditorState.allowMultipleSelections.of(true),indentOnInput(),syntaxHighlighting(defaultHighlightStyle,{fallback:true}),bracketMatching(),closeBrackets(),autocompletion(),rectangularSelection(),crosshairCursor(),highlightActiveLine(),highlightSelectionMatches(),keymap.of([...closeBracketsKeymap,...defaultKeymap,...searchKeymap,...historyKeymap,...foldKeymap,...completionKeymap]),
    autocompletion({override:[complete],activateOnTyping:true,activateOnCompletion:item=>item.type==='namespace'||typeof item.apply==='string'&&item.apply.endsWith('.')}),
    keymap.of([{key:'Tab',run:acceptCompletion}]),
  ];
}

// The language a file is edited with: the Forma grammar for markup, multi-language Rust tokenizer WASM otherwise.
export const languageFor=path=>path?.endsWith('.ui')?formaHighlight:tokenizerHighlight(path);
