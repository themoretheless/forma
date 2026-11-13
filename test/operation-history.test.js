import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createOperationHistory } from '../src/operation-history.js';

/** An operation that records what was done to `log` and can be told to fail. */
function operation(log, label, { undo = true, redo = true } = {}) {
  return {
    label,
    undo: async () => (log.push(`undo ${label}`), undo),
    redo: async () => (log.push(`redo ${label}`), redo),
  };
}

test('operations are undone newest first and redone in the order they were undone', async () => {
  const log = [],
    said = [];
  const history = createOperationHistory({ announce: text => said.push(text) });
  assert.equal(history.canUndo, false);
  assert.equal(await history.undo(), false);
  history.push(operation(log, 'a'));
  history.push(operation(log, 'b'));
  assert.equal(await history.undo(), true);
  assert.equal(await history.undo(), true);
  assert.equal(history.canUndo, false);
  assert.equal(await history.redo(), true);
  assert.deepEqual(log, ['undo b', 'undo a', 'redo a']);
  assert.deepEqual(said, ['Отменено: b', 'Отменено: a', 'Повторено: a']);
  assert.equal(history.canUndo, true);
  assert.equal(history.canRedo, true);
});

test('a new operation clears what could have been redone', async () => {
  const log = [];
  const history = createOperationHistory();
  history.push(operation(log, 'a'));
  await history.undo();
  history.push(operation(log, 'b'));
  assert.equal(history.canRedo, false);
  assert.equal(await history.redo(), false);
});

test('a step that reports failure drops the operation without announcing it', async () => {
  const log = [],
    said = [];
  const history = createOperationHistory({ announce: text => said.push(text) });
  history.push(operation(log, 'stale', { undo: false }));
  assert.equal(await history.undo(), true);
  assert.equal(history.canUndo, false);
  assert.equal(history.canRedo, false);
  assert.deepEqual(said, []);

  history.push(operation(log, 'half', { redo: false }));
  await history.undo();
  assert.equal(await history.redo(), true);
  assert.equal(history.canUndo, false);
  assert.deepEqual(said, ['Отменено: half']);
});

test('only the newest operations up to the limit are kept', async () => {
  const log = [];
  const history = createOperationHistory({ limit: 2 });
  for (const label of ['a', 'b', 'c']) history.push(operation(log, label));
  while (await history.undo());
  assert.deepEqual(log, ['undo c', 'undo b']);
});
