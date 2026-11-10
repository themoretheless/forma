import { test } from 'node:test';
import assert from 'node:assert/strict';
import { escapeHtml, parentDirOf, countLines } from '../src/shared/text.js';

test('escapeHtml covers text and both attribute quotes, and tolerates empty values', () => {
  assert.equal(
    escapeHtml(`<a href="x" title='y'>&</a>`),
    '&lt;a href=&quot;x&quot; title=&#39;y&#39;&gt;&amp;&lt;/a&gt;',
  );
  assert.equal(escapeHtml(null), '');
  assert.equal(escapeHtml(undefined), '');
  assert.equal(escapeHtml(42), '42');
});

test('parentDirOf returns the directory part or an empty string', () => {
  assert.equal(parentDirOf('ui/forms/Main.ui'), 'ui/forms');
  assert.equal(parentDirOf('Main.ui'), '');
  assert.equal(parentDirOf(), '');
});

test('countLines counts newlines before the offset, one-based', () => {
  assert.equal(countLines(''), 1);
  assert.equal(countLines('a\nb\nc'), 3);
  assert.equal(countLines('a\nb\nc', 2), 2);
  assert.equal(countLines('a\nb\nc', 1), 1);
});
