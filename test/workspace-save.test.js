import { test } from 'node:test';
import assert from 'node:assert/strict';
import { workspaceSaveStatus } from '../src/workspace-save.js';

// The project is written to browser storage first; the workspace write then puts the open file
// on disk. The status must say which of the two actually happened.
test('a successful workspace write reports the file as saved to the project', () => {
  assert.deepEqual(workspaceSaveStatus({ ok: true }, 'open-scad-viewer'), {
    status: 'saved',
    text: 'Сохранено (open-scad-viewer)',
  });
});

test('an HTTP error does not claim the file reached the project', () => {
  assert.deepEqual(workspaceSaveStatus({ ok: false, status: 500 }, 'open-scad-viewer'), {
    status: 'saved',
    text: 'Сохранено в браузере (запись в проект не удалась: HTTP 500)',
  });
});

test('a network failure does not claim the file reached the project', () => {
  assert.deepEqual(workspaceSaveStatus(new TypeError('Failed to fetch'), 'open-scad-viewer'), {
    status: 'saved',
    text: 'Сохранено в браузере (запись в проект не удалась)',
  });
});
