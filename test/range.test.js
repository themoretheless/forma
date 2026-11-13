// @ts-check
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { rangeBounds, rangeFraction, rangeValue } from '../src/shared/range.js';

const bounds = values => rangeBounds(name => values[name]);

test('without bounds the scale is zero to one with no grid', () => {
  assert.deepEqual(bounds({}), { min: 0, max: 1, step: 0 });
  assert.equal(rangeFraction(bounds({}), 0.25), 0.25);
  assert.equal(rangeValue(bounds({}), 0.25), 0.25);
});

test('a value is placed on the scale and a fraction is taken back from it', () => {
  const scale = bounds({ min: 0, max: 100 });
  assert.equal(rangeFraction(scale, 40), 0.4);
  assert.equal(rangeValue(scale, 0.75), 75);
  const shifted = bounds({ min: -10, max: 10 });
  assert.equal(rangeFraction(shifted, 0), 0.5);
  assert.equal(rangeValue(shifted, 0.25), -5);
});

test('values outside the scale stop at its ends', () => {
  const scale = bounds({ min: 0, max: 100 });
  assert.equal(rangeFraction(scale, 250), 1);
  assert.equal(rangeFraction(scale, -3), 0);
  assert.equal(rangeValue(scale, 2), 100);
  assert.equal(rangeValue(scale, -1), 0);
});

test('a step moves the value to a grid anchored at the minimum', () => {
  const scale = bounds({ min: 0, max: 100, step: 5 });
  assert.equal(rangeValue(scale, 0.73), 75);
  assert.equal(rangeValue(scale, 0.72), 70);
  const anchored = bounds({ min: 1, max: 10, step: 4 });
  assert.equal(rangeValue(anchored, 0.5), 5, '5.5 is nearest to 5 on the grid 1, 5, 9');
  assert.equal(rangeValue(anchored, 1), 10, 'the end is reachable although it is off the grid');
  assert.equal(rangeValue(anchored, 0.95), 9);
});

test('an empty, reversed or non-numeric scale falls back to the default', () => {
  for (const values of [
    { min: 5, max: 5 },
    { min: 10, max: 0 },
    { min: 'a', max: Infinity },
    { min: NaN, max: 1, step: -2 },
  ])
    assert.deepEqual(
      { ...bounds(values), step: 0 },
      { min: 0, max: 1, step: 0 },
      JSON.stringify(values),
    );
  assert.equal(bounds({ step: -2 }).step, 0);
});

const { compileComponents } = await import('../src/components.js');
const { readFileSync, readdirSync } = await import('node:fs');
const { materializeTheme } = await import('../crates/forma/controls/tokens.js');

/** The standard component library with the light theme substituted, as the gallery loads it. */
function library() {
  const directory = new URL('../crates/forma/controls/components/', import.meta.url);
  const files = {};
  for (const name of readdirSync(directory))
    files['components/' + name] = readFileSync(new URL(name, directory), 'utf8');
  return materializeTheme(files, 'light');
}

const rangeInput = template => Number(/RangeInput \{ value: ([-\d.e]+);/.exec(template)?.[1]);

test('the compiler places a bound number on the scale of the slider', () => {
  const files = {
    ...library(),
    'ui/Demo.ui': `component Demo { state level: Number = 40; Frame {
      Slider { key: 'level'; min: 0; max: 100; step: 5; value <-> state.level; } } }`,
  };
  const metrics = { measureText: () => [80, 20] };
  const compiled = compileComponents(files, 'ui/Demo.ui', { level: 40 }, metrics);
  assert.ok(Math.abs(rangeInput(compiled.template) - 0.4) < 1e-9, '40 of 100');
  assert.deepEqual(compiled.previewControls[0].rangeBounds, { min: 0, max: 100, step: 5 });
  assert.match(
    compiled.template,
    /RangeInput \{ value: [\d.e-]+; step: 0\.05; Minimum/,
    'step as a fraction',
  );

  // Without a scale the number is the fraction itself, as before.
  files['ui/Demo.ui'] = `component Demo { state level: Number = 0.25; Frame {
      Slider { key: 'level'; value <-> state.level; } } }`;
  const plain = compileComponents(files, 'ui/Demo.ui', { level: 0.25 }, metrics);
  assert.equal(rangeInput(plain.template), 0.25);
  assert.doesNotMatch(plain.template, /step:/, 'no grid without a step');

  // A value off the scale is shown at its end instead of failing on the thumb's column width.
  const off = compileComponents(files, 'ui/Demo.ui', { level: 40 }, metrics);
  assert.equal(rangeInput(off.template), 1);
});
