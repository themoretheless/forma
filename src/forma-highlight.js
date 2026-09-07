import {LRLanguage,LanguageSupport,HighlightStyle,syntaxHighlighting} from '@codemirror/language';
import {styleTags,tags} from '@lezer/highlight';
import {parser} from './forma-parser.js';
import {colorSwatches} from './color-swatches.js';
import {rainbowBrackets} from './brackets.js';
const language=LRLanguage.define({parser:parser.configure({props:[styleTags({
  'component design match override from prop required event enum forward props if else for in key empty':tags.keyword,
  'TypeName ComponentName':tags.typeName,
  'PropertyName/Identifier':tags.propertyName,
  'AttributeName/Identifier':tags.meta,
  'Path/Identifier':tags.variableName,
  'Call/Path/Identifier':tags.function(tags.variableName),
  String:tags.string,
  'Number Star Dash':tags.number,
  Color:tags.color,
  Boolean:tags.bool,
  Null:tags.null,
  'Binding EventArrow MatchArrow Comparison Equality':tags.operator,
  'BinaryExpression/Star BinaryExpression/Dash UnaryExpression/Dash':tags.operator,
  '"+" "/" "%" "!" "&&" "||" "??" "?." "?" "="':tags.operator,
  'LineComment BlockComment':tags.comment,
  Wildcard:tags.keyword,
  '( )':tags.paren,
  '[ ]':tags.squareBracket,
  '{ }':tags.brace,
})]}),languageData:{commentTokens:{line:'//',block:{open:'/*',close:'*/'}},closeBrackets:{brackets:['(','[','{',"'",'"']}}});
const colors=HighlightStyle.define([
  {tag:[tags.keyword,tags.operator,tags.meta],color:'#c7a3ef'},
  {tag:tags.typeName,color:'#80d5ce'},
  {tag:tags.propertyName,color:'#9ec9f5'},
  {tag:tags.string,color:'#a7d5a0'},
  {tag:[tags.number,tags.color,tags.bool],color:'#efbc91'},
  {tag:tags.variableName,color:'#f1bc83'},
  {tag:tags.function(tags.variableName),color:'#dfcf95'},
  {tag:tags.comment,color:'#77859a',fontStyle:'italic'},
  {tag:[tags.paren,tags.squareBracket,tags.brace],color:'#8f9db4'},
]);
export const formaHighlight=[new LanguageSupport(language),syntaxHighlighting(colors),colorSwatches,rainbowBrackets];
