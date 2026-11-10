import { test } from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import { escapesRoot, isInsideDir } from '../dev-server/workspace-paths.js';

test('a name that merely starts with two dots stays inside the root', () => {
  assert.equal(escapesRoot(path.relative('/w', '/w/..config')), false);
  assert.equal(escapesRoot(path.relative('/w', '/w/a/..b')), false);
});

test('parent references and other roots escape', () => {
  assert.equal(escapesRoot(path.relative('/w', '/')), true);
  assert.equal(escapesRoot(path.relative('/w', '/w/../x')), true);
  assert.equal(escapesRoot('..'), true);
  assert.equal(escapesRoot(path.win32.relative('C:\\w', 'D:\\x'), path.win32), true);
});

test('moving a folder into itself is detected with either separator', () => {
  assert.equal(isInsideDir('/w/a', '/w/a/b'), true);
  assert.equal(isInsideDir('/w/a', '/w/ab'), false);
  assert.equal(isInsideDir('/w/a', '/w/a'), false);
  assert.equal(isInsideDir('C:\\w\\a', 'C:\\w\\a\\b', path.win32), true);
  assert.equal(isInsideDir('C:\\w\\a', 'C:\\w\\ab', path.win32), false);
});
