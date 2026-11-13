// The tokenizer reports UTF-8 byte offsets; the editor addresses UTF-16 code units.
import { test } from 'node:test';
import assert from 'node:assert/strict';

// The module imports CodeMirror's view layer, which needs no DOM until a view is created.
const { buildUtf8ToUtf16Map, detectTokenizerLanguage } =
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
