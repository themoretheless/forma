import assert from 'node:assert/strict';

// Tests that read source as text must not depend on how it is formatted.
// `squash` drops all whitespace, so patterns are written without any.
export const squash = text => text.replace(/\s+/g, '');

// Replace exactly one occurrence and fail loudly when the source no longer contains it:
// a silent no-op would leave the real import in place and test the wrong module.
export function replaceOnce(source, pattern, replacement) {
  const matches =
    source.match(new RegExp(pattern.source, pattern.flags.replace('g', '') + 'g')) ?? [];
  assert.equal(
    matches.length,
    1,
    `expected exactly one match of ${pattern} in the source, found ${matches.length}`,
  );
  return source.replace(pattern, replacement);
}
