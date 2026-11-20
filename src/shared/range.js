// @ts-check
// The scale of a range control. A bound number lies between `min` and `max`, optionally on a
// `step` grid, while the widget works with a fraction from 0 to 1. Mirrors
// crates/forma/src/form_document/range.rs.

const finite = value => (typeof value === 'number' && Number.isFinite(value) ? value : undefined);

/**
 * Reads `min`, `max` and `step`; anything missing, non-numeric or inconsistent falls back to the
 * default scale of 0 to 1 with no grid, where the bound number is the fraction itself.
 *
 * @param {(name: string) => unknown} property
 */
export function rangeBounds(property) {
  const min = finite(property('min')) ?? 0,
    max = finite(property('max')) ?? 1,
    step = Math.max(0, finite(property('step')) ?? 0);
  return min < max && Number.isFinite(max - min) ? { min, max, step } : { min: 0, max: 1, step: 0 };
}

/** Where `value` lies on the scale, from 0 to 1. */
export function rangeFraction({ min, max }, value) {
  return Math.min(1, Math.max(0, (value - min) / (max - min)));
}

/** The value at `fraction` of the scale, moved to the step grid when there is one. */
export function rangeValue({ min, max, step }, fraction) {
  const value = min + Math.min(1, Math.max(0, fraction)) * (max - min);
  if (!(step > 0) || value <= min || value >= max) return value;
  // Steps are anchored at min, as in RangeModel::set_value.
  return Math.min(max, Math.max(min, min + Math.round((value - min) / step) * step));
}
