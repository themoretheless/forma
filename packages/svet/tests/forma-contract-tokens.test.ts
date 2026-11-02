import { beforeEach, expect, it } from 'vitest';
import { compileFormaComponent, parseFormaSource, resetReactiveSystem, validateFormaSource } from '../src/engine';
beforeEach(() => resetReactiveSystem());

it('validates inline reversed ranges before HMR admission', () => {
  expect(validateFormaSource('component App { #[range(100,10)] prop x: Number = 5; Column {} }').valid).toBe(false);
});
it('applies an inline range to its prop at runtime', () => {
  const instance = compileFormaComponent('component App { #[range(0,10)] prop x: Number = 5; Column {} }');
  try { expect(instance.isValid()).toBe(true); instance.props.x.set(20); expect(instance.isValid()).toBe(false); }
  finally { instance.dispose(); }
});
it('preserves quoted brackets in inline pattern contracts', () => {
  const source = "component App { #[pattern('^[a-z]+$')] prop name: String = 'ok'; Column {} }";
  expect(validateFormaSource(source).valid).toBe(true);
  const instance = compileFormaComponent(source);
  try { expect(instance.isValid()).toBe(true); instance.props.name.set('123'); expect(instance.isValid()).toBe(false); }
  finally { instance.dispose(); }
});
it('ignores contract-looking text in strings/comments and refuses an unclosed token', () => {
  expect(validateFormaSource("component App { // #[range(100,10)]\n Text { text:'#[range(100,10)]'; } }").valid).toBe(true);
  expect(validateFormaSource('component App { #[range(0,10) prop x: Number = 5; }').valid).toBe(false);
  expect(() => parseFormaSource('component App { #[range(0,10) prop x: Number = 5; }')).toThrow(/Unclosed/);
});

for (const annotation of ['range(0)', 'range(0,10,20)', 'range(no,10)', 'pattern(123)', 'pattern([abc])']) {
  it(`refuses malformed known annotation ${annotation}`, () => {
    const source = `component App { #[${annotation}] prop x: Number = 5; Column {} }`;
    expect(validateFormaSource(source).valid).toBe(false);
    expect(() => parseFormaSource(source)).toThrow(/contract|annotation/i);
  });
}

it('applies a multiline quoted pattern rather than silently dropping it', () => {
  const source = `component App { #[pattern('^a
b$')] prop name: String = 'wrong'; Column {} }`;
  expect(validateFormaSource(source).valid).toBe(true);
  const ast = parseFormaSource(source);
  expect(ast.propConstraints[0].patternSource).toBe('^a\nb$');
  const instance = compileFormaComponent(source);
  try { expect(instance.isValid()).toBe(false); instance.props.name.set('a\nb'); expect(instance.isValid()).toBe(true); }
  finally { instance.dispose(); }
});
