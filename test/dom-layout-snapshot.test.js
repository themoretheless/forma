// @ts-check
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createComponentCatalog, domLayoutSnapshot } from '../src/dom-layout-snapshot.js';

const rect = (left, top, width, height) => ({ left, top, width, height });

function element(start, classes, box, parentElement = null) {
  return {
    dataset: { start: String(start) },
    classList: classes,
    parentElement,
    getBoundingClientRect: () => box,
  };
}

function preview({ box, offsetWidth, offsetHeight, elements }) {
  return {
    firstElementChild: elements[0] ?? null,
    offsetWidth,
    offsetHeight,
    getBoundingClientRect: () => box,
    querySelectorAll: selector => {
      assert.equal(selector, '[data-start]');
      return elements;
    },
  };
}

test('no preview element gives no snapshot', () => {
  assert.equal(domLayoutSnapshot(null), null);
});

test('bounds are relative to the preview and the root element is left out', () => {
  const root = element(0, ['frame'], rect(100, 50, 400, 300));
  const row = element(10, ['row'], rect(120, 70, 200, 40), root);
  const button = element(20, ['button'], rect(130, 75, 80, 30), row);
  const snapshot = domLayoutSnapshot(
    preview({
      box: rect(100, 50, 400, 300),
      offsetWidth: 400,
      offsetHeight: 300,
      elements: [root, row, button],
    }),
  );
  assert.deepEqual(snapshot, {
    width: 400,
    height: 300,
    clip: false,
    scrollable: false,
    scrollOffset: [0, 0],
    controls: [{ index: 0, start: 20, bounds: [30, 25, 80, 30], coordinateBox: [20, 20, 200, 40] }],
    containers: [{ start: 10, bounds: [20, 20, 200, 40], coordinateBox: [0, 0, 400, 300] }],
  });
});

test('a zoomed preview is measured in its own unscaled pixels', () => {
  const root = element(0, ['frame'], rect(0, 0, 800, 600));
  const label = element(5, ['label'], rect(40, 20, 160, 60), root);
  const snapshot = domLayoutSnapshot(
    preview({
      box: rect(0, 0, 800, 600),
      offsetWidth: 400,
      offsetHeight: 300,
      elements: [root, label],
    }),
  );
  assert.equal(snapshot.width, 400);
  assert.equal(snapshot.height, 300);
  assert.deepEqual(snapshot.controls[0].bounds, [20, 10, 80, 30]);
  assert.deepEqual(snapshot.controls[0].coordinateBox, [0, 0, 400, 300]);
});

test('controls are numbered in document order, containers are not', () => {
  const root = element(0, ['frame'], rect(0, 0, 100, 100));
  const first = element(1, ['button'], rect(0, 0, 10, 10), root);
  const grid = element(2, ['grid'], rect(0, 20, 50, 50), root);
  const second = element(3, ['input'], rect(0, 20, 10, 10), grid);
  const orphan = element(4, ['label'], rect(0, 80, 10, 10));
  const snapshot = domLayoutSnapshot(
    preview({
      box: rect(0, 0, 100, 100),
      offsetWidth: 100,
      offsetHeight: 100,
      elements: [root, first, grid, second, orphan],
    }),
  );
  assert.deepEqual(
    snapshot.controls.map(c => [c.index, c.start]),
    [
      [0, 1],
      [1, 3],
      [2, 4],
    ],
  );
  assert.deepEqual(
    snapshot.containers.map(c => c.start),
    [2],
  );
  // Without a parent the preview itself is the coordinate box.
  assert.deepEqual(snapshot.controls[2].coordinateBox, [0, 0, 100, 100]);
});

test('an unmeasured preview falls back to a default size', () => {
  const snapshot = domLayoutSnapshot(
    preview({ box: rect(0, 0, 0, 0), offsetWidth: 0, offsetHeight: 0, elements: [] }),
  );
  assert.equal(snapshot.width, 360);
  assert.equal(snapshot.height, 220);
  assert.deepEqual(snapshot.controls, []);
});

test('the component catalog is rebuilt only when component files change', () => {
  let builds = 0;
  const catalog = createComponentCatalog(files => {
    builds += 1;
    return Object.keys(files).length;
  });
  const files = { 'ui/Main.ui': 'Frame {}', 'components/A.ui': 'Button {}' };
  assert.equal(catalog(files), 2);
  assert.equal(catalog({ ...files, 'ui/Main.ui': 'Frame { width: 1; }' }), 2);
  assert.equal(builds, 1, 'a file outside components/ does not rebuild');
  assert.equal(catalog({ ...files, 'components/A.ui': 'Button { }' }), 2);
  assert.equal(builds, 2, 'a component of another length rebuilds');
  assert.equal(catalog({ ...files, 'components/A.ui': 'Button { }', 'components/B.ui': '' }), 3);
  assert.equal(builds, 3);
});

test('an empty project still builds the catalog once', () => {
  let builds = 0;
  const catalog = createComponentCatalog(() => ++builds);
  assert.equal(catalog({}), 1);
  assert.equal(catalog({}), 1);
});
