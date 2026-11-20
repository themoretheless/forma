// The distances the canvas draws from a selected control to its nearest neighbour or the frame.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { layoutDistances } from '../src/canvas/layout-distances.js';

const FRAME = [400, 300];
const SELECTED = [100, 100, 50, 40]; // x 100..150, y 100..140, centre 125, 120
const bySide = distances => Object.fromEntries(distances.map(d => [d.side, d]));

test('with no neighbours every side measures to the frame', () => {
  assert.deepEqual(layoutDistances(SELECTED, [], FRAME), [
    { side: 'left', value: 100, from: [0, 120], to: [100, 120], target: 'Frame' },
    { side: 'right', value: 250, from: [150, 120], to: [400, 120], target: 'Frame' },
    { side: 'top', value: 100, from: [125, 0], to: [125, 100], target: 'Frame' },
    { side: 'bottom', value: 160, from: [125, 140], to: [125, 300], target: 'Frame' },
  ]);
});

test('a neighbour that faces a side replaces the frame on that side', () => {
  const neighbours = [
    [40, 110, 30, 20], // left, ends at x 70
    [180, 90, 20, 60], // right, starts at x 180
    [110, 30, 30, 50], // above, ends at y 80
    [90, 170, 100, 10], // below, starts at y 170
  ];
  assert.deepEqual(bySide(layoutDistances(SELECTED, neighbours, FRAME)), {
    left: { side: 'left', value: 30, from: [70, 120], to: [100, 120], target: 'сосед' },
    right: { side: 'right', value: 30, from: [150, 120], to: [180, 120], target: 'сосед' },
    top: { side: 'top', value: 20, from: [125, 80], to: [125, 100], target: 'сосед' },
    bottom: { side: 'bottom', value: 30, from: [125, 140], to: [125, 170], target: 'сосед' },
  });
});

test('a neighbour that does not overlap the control across the axis is ignored', () => {
  const diagonal = [
    [40, 10, 30, 20], // to the left but entirely above
    [180, 200, 20, 20], // to the right but entirely below
    [10, 30, 30, 50], // above but entirely to the left
    [300, 170, 50, 10], // below but entirely to the right
    [40, 140, 30, 20], // to the left, only touching the bottom edge
  ];
  assert.deepEqual(
    layoutDistances(SELECTED, diagonal, FRAME).map(d => [d.side, d.value, d.target]),
    [
      ['left', 100, 'Frame'],
      ['right', 250, 'Frame'],
      ['top', 100, 'Frame'],
      ['bottom', 160, 'Frame'],
    ],
  );
});

test('the nearest neighbour wins whatever the order', () => {
  const near = [60, 110, 30, 20], // ends at x 90, 10 away
    far = [10, 110, 30, 20]; // ends at x 40, 60 away
  for (const neighbours of [
    [near, far],
    [far, near],
  ])
    assert.equal(bySide(layoutDistances(SELECTED, neighbours, FRAME)).left.value, 10);
  // A neighbour farther than the frame edge does not replace the frame.
  const tight = layoutDistances([5, 100, 50, 40], [[-100, 110, 30, 20]], FRAME);
  assert.deepEqual([bySide(tight).left.value, bySide(tight).left.target], [5, 'Frame']);
});

test('a touching neighbour measures zero and a side outside the frame is not drawn', () => {
  const touching = bySide(layoutDistances(SELECTED, [[150, 100, 20, 40]], FRAME));
  assert.deepEqual([touching.right.value, touching.right.target], [0, 'сосед']);

  const outside = layoutDistances([380, 280, 50, 40], [], FRAME);
  assert.deepEqual(
    outside.map(d => d.side),
    ['left', 'top'],
  );
  // An overlapping neighbour is on no side.
  const overlapping = layoutDistances(SELECTED, [[120, 110, 50, 40]], FRAME);
  assert.ok(overlapping.every(d => d.target === 'Frame'));
});
