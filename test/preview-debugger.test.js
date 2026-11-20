import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createPreviewDebugger } from '../src/preview/preview-debugger.js';

function debuggerWith() {
  const calls = [];
  const preview = createPreviewDebugger({
    argumentsOf: (node, event) => node.args?.[event] ?? [],
    execute: (action, args) => calls.push(['execute', action, args]),
    log: text => calls.push(['log', text]),
    paused: action => calls.push(['paused', action]),
    resumed: () => calls.push(['resumed']),
  });
  return { preview, calls };
}
const button = { type: 'Button', args: { clicked: ['a', 2], changed: [] } };

test('without a breakpoint an event is logged and run at once', () => {
  const { preview, calls } = debuggerWith();
  preview.dispatch('actions.go', button);
  preview.dispatch('actions.edit', button, 'changed');
  assert.deepEqual(calls, [
    ['log', 'Событие Button.clicked → actions.go("a", 2)'],
    ['execute', 'actions.go', ['a', 2]],
    ['log', 'Событие Button.changed → actions.edit()'],
    ['execute', 'actions.edit', []],
  ]);
  assert.equal(preview.pending, null);
});

test('with a breakpoint the event waits until it is resumed', () => {
  const { preview, calls } = debuggerWith();
  preview.setBreak(true);
  assert.equal(preview.breakOn, true);
  preview.dispatch('actions.go', button);
  assert.deepEqual(preview.pending, { action: 'actions.go', n: button, args: ['a', 2] });
  assert.deepEqual(calls, [
    ['log', 'Событие Button.clicked → actions.go("a", 2)'],
    ['paused', 'actions.go'],
  ]);

  calls.length = 0;
  assert.equal(preview.resume(), true);
  // The waiting state is cleared before the action runs, so the action may dispatch again.
  assert.deepEqual(calls, [['resumed'], ['execute', 'actions.go', ['a', 2]]]);
  assert.equal(preview.pending, null);
  assert.equal(preview.resume(), false);
});

test('while one event waits the next is dropped', () => {
  const { preview, calls } = debuggerWith();
  preview.setBreak(true);
  preview.dispatch('actions.go', button);
  calls.length = 0;
  preview.dispatch('actions.other', button);
  assert.deepEqual(calls, [['log', 'Событие пропущено: debugger приостановлен']]);
  assert.equal(preview.pending.action, 'actions.go');
});

test('reset drops the waiting event without running it', () => {
  const { preview, calls } = debuggerWith();
  preview.setBreak(1);
  preview.dispatch('actions.go', button);
  calls.length = 0;
  preview.reset();
  assert.deepEqual(calls, [['resumed']]);
  assert.equal(preview.pending, null);
  assert.equal(preview.breakOn, true, 'the breakpoint stays on');
  preview.setBreak(0);
  preview.dispatch('actions.go', button);
  assert.deepEqual(calls.at(-1), ['execute', 'actions.go', ['a', 2]]);
});

test('an action that dispatches while resuming is handled as a new event', () => {
  const calls = [];
  const preview = createPreviewDebugger({
    argumentsOf: () => [],
    execute: action => {
      calls.push(action);
      if (action === 'first') preview.dispatch('second', button);
    },
    log() {},
    paused() {},
    resumed() {},
  });
  preview.setBreak(true);
  preview.dispatch('first', button);
  preview.resume();
  assert.deepEqual(calls, ['first']);
  assert.equal(preview.pending.action, 'second');
});
