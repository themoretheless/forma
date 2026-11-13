// The tokenizer reports UTF-8 byte offsets; the editor addresses UTF-16 code units.
import { test } from 'node:test';
import assert from 'node:assert/strict';

// The module imports CodeMirror's view layer, which needs no DOM until a view is created.
const { buildUtf8ToUtf16Map, detectTokenizerLanguage, kindColor } =
  await import('../src/tokenizer-highlight.js');

const encoder = new TextEncoder();

/** Every position a token may start or end at: the boundaries between code points. */
function boundaries(text) {
  const positions = [0];
  for (const character of text) positions.push(positions.at(-1) + character.length);
  return positions;
}

test('ASCII text needs no map', () => {
  assert.equal(buildUtf8ToUtf16Map(''), null);
  assert.equal(buildUtf8ToUtf16Map("component Demo { text: 'a'; }"), null);
});

test('every code point boundary maps from its UTF-8 offset to its UTF-16 index', () => {
  const samples = [
    'é',
    'aé',
    'привет',
    '日本語',
    '😀',
    'a😀b',
    "text: 'Привет, 世界 😀!';",
    'é', // combining mark
    '👨‍👩‍👧', // joined emoji
    '\ud83d', // lone high surrogate
    '\ude00', // lone low surrogate
    'a\ud83db', // high surrogate followed by a letter
    '߿ࠀ￿', // the last two-byte and the first and last three-byte code points
  ];
  for (const text of samples) {
    const map = buildUtf8ToUtf16Map(text);
    assert.ok(map, `a map for ${JSON.stringify(text)}`);
    for (const index of boundaries(text)) {
      const offset = encoder.encode(text.slice(0, index)).length;
      assert.equal(map[offset], index, `${JSON.stringify(text)} at ${index}`);
    }
    assert.equal(map[encoder.encode(text).length], text.length);
  }
});

test('the language of a file is taken from its name', () => {
  assert.deepEqual(detectTokenizerLanguage('ui/Demo.ui'), {
    lang: 'forma',
    mode: '',
    label: 'Forma UI',
  });
  assert.equal(detectTokenizerLanguage('UI/DEMO.UI').lang, 'forma');
  assert.equal(detectTokenizerLanguage('crates/forma/Cargo.lock').lang, 'toml');
  assert.equal(detectTokenizerLanguage('schema.gql').lang, 'graphql');
  for (const path of ['', 'README', 'archive.unknownext', undefined])
    assert.equal(detectTokenizerLanguage(path), null, String(path));
});

test('kinds that carry no ink are not painted, even one that has a colour of its own', () => {
  for (const kind of ['whitespace', 'newline', 'bom', 'identifier', 'padding'])
    assert.equal(kindColor(kind), null, kind);
  // `text` is listed with a colour and as invisible; invisible wins.
  assert.equal(kindColor('text'), null);
});

test('URL parts share a colour and their separators take another', () => {
  assert.equal(kindColor('u-zz-host'), kindColor('autolink'));
  assert.equal(kindColor('u-zz-path'), kindColor('u-zz-host'));
  assert.notEqual(kindColor('u-sep'), kindColor('u-zz-host'));
});

test('an unlisted kind is coloured by the family its name mentions', () => {
  const family = name => kindColor('zz-' + name + '-zz');
  assert.equal(family('error'), kindColor('invalid'));
  assert.equal(family('invalid'), kindColor('invalid'));
  assert.equal(family('int'), family('number'));
  assert.equal(family('float'), family('number'));
  assert.equal(family('class'), family('type'));
  for (const member of ['prop', 'key', 'attr'])
    assert.equal(family(member), family('func'), member);
  const families = ['comment', 'string', 'number', 'keyword', 'type', 'func', 'error'].map(family);
  assert.equal(new Set(families).size, families.length, 'each family has its own colour');
});

test('when a name mentions two families the earlier rule decides', () => {
  const family = name => kindColor('zz-' + name + '-zz');
  assert.equal(kindColor('zz-comment-string'), family('comment'));
  assert.equal(kindColor('zz-string-number'), family('string'));
  assert.equal(kindColor('zz-number-keyword'), family('number'));
  // `keyword` contains `key`, which alone would make it a member.
  assert.notEqual(family('keyword'), family('key'));
  assert.equal(kindColor('zz-type-func'), family('type'));
  assert.equal(kindColor('zz-func-error'), family('func'));
});

test('a kind no rule knows gets a stable colour from the palette', () => {
  const colours = new Set();
  for (const kind of ['zz-a', 'zz-b', 'zz-c', 'zz-d', 'zz-e', 'zz-f', 'zz-g', 'zz-h', 'qq', 'ww']) {
    const colour = kindColor(kind);
    assert.match(colour, /^#[0-9a-f]{6}$/);
    assert.equal(kindColor(kind), colour, 'the same kind keeps its colour');
    colours.add(colour);
  }
  assert.ok(colours.size > 1, 'different kinds spread over the palette');
  assert.ok(colours.size <= 8);
});
