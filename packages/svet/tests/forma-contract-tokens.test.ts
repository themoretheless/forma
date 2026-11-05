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

for (const annotation of ['ragne(0,10)', 'required', '', 'pattern(\'\')', 'range(10,0)', "pattern('[')", "pattern('(a+)+$')"]) {
  it(`refuses unsupported or semantically invalid annotation ${annotation} in parser and validator`, () => {
    const source = `component App { #[${annotation}] prop x: String = 'seed'; Column {} }`;
    expect(validateFormaSource(source).valid).toBe(false);
    expect(() => parseFormaSource(source)).toThrow();
    expect(() => compileFormaComponent(source)).toThrow();
  });
}

for (const body of [
  '#[range(0,10)]',
  '#[range(0,10)] Column {}',
  '#[range(0,10)] state count: Number = 1; prop later: Number = 2;',
  '#[range(0,10)] let count = 1; prop later: Number = 2;',
  '#[range(0,10)] tokens { color: 1; } prop later: Number = 2;',
  '#[range(0,10)] #[range(0,5)] prop x: Number = 2;',
  `#[pattern('a')] #[pattern('b')] prop x: String = 'a';`,
]) {
  it(`refuses misplaced or duplicated contract ${body}`, () => {
    const source = `component App { ${body} }`;
    expect(() => parseFormaSource(source)).toThrow(/contract/i);
    expect(validateFormaSource(source).valid).toBe(false);
  });
}
it('permits different contracts stacked across comments on one required prop', () => {
  const source = `component App { #[range(0,10)] // retain both contracts
    #[pattern('^[0-9]+$')] required prop value: String = '2'; Column {} }`;
  expect(validateFormaSource(source).valid).toBe(true);
  const ast = parseFormaSource(source);
  expect(ast.propConstraints[0]).toMatchObject({ name:'value', required:true, range:[0,10], patternSource:'^[0-9]+$' });
});

for (const body of [
  'Column { #[range(0,10)] prop x: Number = 2; Text {} }',
  'Column { if true { #[range(0,10)] Text {} } }',
  'Column { for item in state.items { #[range(0,10)] Text {} } }',
  'tokens { #[range(0,10)] value: 2; } Column {}',
  'Column {} #[range(0,10)] prop x: Number = 2;',
]) {
  it(`refuses unsupported nested or late contract ${body}`, () => {
    const source = `component App { ${body} }`;
    expect(() => parseFormaSource(source)).toThrow(/contract/i);
    expect(validateFormaSource(source).valid).toBe(false);
  });
}
it('keeps annotation-looking text literal inside visual properties and comments', () => {
  const source = `component App { Column { // #[range(2,1)]
    Text { text:'#[range(2,1)]'; } } }`;
  expect(() => parseFormaSource(source)).not.toThrow();
  expect(validateFormaSource(source).valid).toBe(true);
});
