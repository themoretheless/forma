import {test} from 'node:test';
import assert from 'node:assert/strict';
import {CASES, FRAMES, REPEATS, makeSchedule, summarizePairs} from '../scripts/interleave-bench.mjs';

test('interleaved schedule contains 48 unique adjacent balanced A/B runs', () => {
  const schedule = makeSchedule();
  assert.equal(REPEATS, 4);
  assert.equal(FRAMES, 240);
  assert.equal(CASES.length, 6);
  assert.equal(schedule.length, 48);
  assert.equal(new Set(schedule.map(row => `${row.pairId}-${row.variant}`)).size, 48);
  for (let index = 0; index < schedule.length; index += 2) {
    const [first, second] = schedule.slice(index, index + 2);
    assert.equal(first.pairId, second.pairId);
    assert.equal(first.caseIndex, second.caseIndex);
    assert.equal(first.repetition, second.repetition);
    assert.deepEqual(first.spec, CASES[first.caseIndex]);
    assert.deepEqual(second.spec, first.spec);
    assert.equal(first.position, 1);
    assert.equal(second.position, 2);
    assert.equal(first.variant + second.variant, first.order);
    assert.equal(second.order, first.order);
    assert.notEqual(first.variant, second.variant);
  }
  for (let caseIndex = 0; caseIndex < CASES.length; caseIndex++) {
    const pairs = schedule.filter(row => row.caseIndex === caseIndex && row.position === 1);
    assert.deepEqual(pairs.map(row => row.repetition), [1, 2, 3, 4]);
    assert.equal(pairs.filter(row => row.order === 'AB').length, 2);
    assert.equal(pairs.filter(row => row.order === 'BA').length, 2);
  }
});

test('summary uses median of paired changes, not ratio of independent medians', () => {
  const pairs = [[1, 2], [2, 4], [100, 100], [200, 200]]
    .map(([a, b]) => ({A: {value: a}, B: {value: b}}));
  const summary = summarizePairs(pairs, row => row.value);
  assert.equal(summary.aMedian, 51);
  assert.equal(summary.bMedian, 52);
  assert.equal(summary.pairedMedianPercent, 50);
  assert.notEqual(summary.pairedMedianPercent, (summary.bMedian / summary.aMedian - 1) * 100);
  assert.deepEqual(summary.aRange, [1, 200]);
  assert.deepEqual(summary.bRange, [2, 200]);
  assert.deepEqual(summary.pairedRangePercent, [0, 100]);
  assert.equal(pairs[0].A.value, 1); // Input order/values remain untouched.
});

test('missing or invalid members invalidate the whole metric instead of dropping pairs', () => {
  for (const value of [null, undefined, Number.NaN, Number.POSITIVE_INFINITY, -1, '1']) {
    const pairs = [{A: {value: 1}, B: {value: 2}}, {A: {value: 3}, B: {value}}];
    assert.equal(summarizePairs(pairs, row => row.value), null);
  }
});

test('zero A baseline retains absolute measurements but has no percentage ratio', () => {
  const summary = summarizePairs([
    {A: {value: 0}, B: {value: 2}},
    {A: {value: 2}, B: {value: 4}},
  ], row => row.value);
  assert.equal(summary.aMedian, 1);
  assert.equal(summary.bMedian, 3);
  assert.equal(summary.pairedMedianPercent, null);
  assert.equal(summary.pairedRangePercent, null);
  const zeroAfter = summarizePairs([{A: {value: 2}, B: {value: 0}}], row => row.value);
  assert.equal(zeroAfter.pairedMedianPercent, -100);
});
