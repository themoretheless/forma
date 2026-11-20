import { test } from 'node:test';
import assert from 'node:assert/strict';
import { summarizeDisplayListDiagnostics } from '../src/display-list-diagnostics.js';

test('summarizeDisplayListDiagnostics aggregates items, clip depth, and resource IDs', () => {
  const sample = JSON.stringify({
    itemCount: 3,
    clipCount: 2,
    commandFloats: 100,
    edgeFloats: 16,
    clips: [
      { id: 0, parent: null, bounds: [0, 0, 200, 100], radius: 8, crisp: false },
      { id: 1, parent: 0, bounds: [10, 10, 80, 30], radius: 0, crisp: false },
    ],
    items: [
      { kind: 'SolidRect', clipId: 0, resourceId: null },
      { kind: 'StyledBox', clipId: 0, resourceId: 0 },
      { kind: 'VectorPath', clipId: 1, resourceId: 424242 },
    ],
  });
  const summary = summarizeDisplayListDiagnostics(sample);
  assert.equal(summary.itemCount, 3);
  assert.equal(summary.clipCount, 2);
  assert.equal(summary.maxClipDepth, 2);
  assert.deepEqual(summary.kinds, { SolidRect: 1, StyledBox: 1, VectorPath: 1 });
  assert.equal(summary.resourceIds.length, 2);
});
