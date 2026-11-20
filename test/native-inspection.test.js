// @ts-check
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { nativeInspectionModel } from '../src/inspector/native-inspection.js';

const controls = [
  { type: 'Button', props: { key: 'save' } },
  { type: 'Text', props: {} },
];
const report = {
  nodes: [
    { control: null, parent: null, kind: 'Frame' },
    { control: null, parent: 0, kind: 'Row' },
    { control: 0, parent: 1, kind: 'Button' },
    { control: 1, parent: 1, kind: 'Text' },
    { control: 2, parent: 1, kind: 'Slider' },
  ],
  controls: [
    { index: 0, bounds: [10.4, 20.6, 100, 40], disabled: true },
    { index: 1, bounds: [0, 0, 50.5, 16] },
  ],
  selected: 0,
};

test('rows name the source node, its key, rounded bounds and the disabled flag', () => {
  const model = nativeInspectionModel(report, controls, true);
  assert.deepEqual(
    model.rows.map(r => r.label),
    ['Frame', 'Row', 'Button save · 10, 21, 100, 40 · disabled', 'Text  · 0, 0, 51, 16', 'Slider'],
  );
});

test('the root is flush, containers are indented once and controls twice', () => {
  const model = nativeInspectionModel(report, controls, true);
  assert.deepEqual(
    model.rows.map(r => r.indent),
    [0, 12, 24, 24, 24],
  );
});

test('only the selected control is pressed and only rows with a source can be followed', () => {
  const model = nativeInspectionModel(report, controls, true);
  assert.deepEqual(
    model.rows.map(r => r.pressed),
    [false, false, true, false, false],
  );
  assert.deepEqual(
    model.rows.map(r => r.disabled),
    [true, true, false, false, true],
  );
  assert.equal(model.rows[2].source, controls[0]);
  assert.equal(model.selected, controls[0]);
  assert.match(model.status, /работающего Rust runtime/);
});

test('a window started from another source shows the tree but follows nothing', () => {
  const model = nativeInspectionModel(report, controls, false);
  assert.ok(model.rows.every(r => r.disabled));
  assert.equal(model.selected, null);
  assert.match(model.status, /переход к коду отключён/);
  // The labels still come from the source nodes that were matched.
  assert.equal(model.rows[2].label, 'Button save · 10, 21, 100, 40 · disabled');
});

test('an empty report gives no rows and no selection', () => {
  assert.deepEqual(nativeInspectionModel({}, controls, true).rows, []);
  assert.equal(nativeInspectionModel({}, controls, true).selected, null);
  assert.equal(nativeInspectionModel({ selected: 5 }, controls, true).selected, null);
});
