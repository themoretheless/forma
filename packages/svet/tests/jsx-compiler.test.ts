import { expect, it } from 'vitest';
import { transformSvetJsx } from '../src/engine';

it('transforms every adjacent JSX expression rather than only the first', () => {
  expect(transformSvetJsx('const node = <p>{first()}{second()}</p>;'))
    .toBe('const node = <p>{() => (first())}{() => (second())}</p>;');
});

it('does not mistake calls inside literals for reactive JavaScript', () => {
  const code = 'const node = <p>{"call()"}{/call\\(\\)/.source}</p>;';
  expect(transformSvetJsx(code)).toBe(code);
});

it('handles braces inside comments and regex literals', () => {
  const code = 'const node = <p>{/* } */ count() + /}/.test(label())}</p>;';
  expect(transformSvetJsx(code)).toBe('const node = <p>{/* } */ () => (count() + /}/.test(label()))}</p>;');
});

it('preserves unrelated JavaScript comparisons and object literals', () => {
  const code = 'const check = a > b; const object = { value: count() }; const node = <p>{count()}</p>;';
  expect(transformSvetJsx(code)).toBe('const check = a > b; const object = { value: count() }; const node = <p>{() => (count())}</p>;');
});

it('preserves async and type-annotated callbacks while compiling nested JSX', () => {
  const code = 'const node = <Widget onClick={async () => count()} render={(item: string) => <p>{count()}</p>} />;';
  expect(transformSvetJsx(code)).toBe('const node = <Widget onClick={async () => count()} render={(item: string) => <p>{() => (count())}</p>} />;');
});

it('makes signal-dependent object styles lazy', () => {
  expect(transformSvetJsx('const node = <p style={{ color: color() }} />;'))
    .toBe('const node = <p style={() => ({ color: color() })} />;');
});

it('does not wrap calls deferred inside object methods but tracks computed method names', () => {
  expect(transformSvetJsx('const node = <Widget options={{ getValue() { return count(); } }} />;'))
    .toBe('const node = <Widget options={{ getValue() { return count(); } }} />;');
  expect(transformSvetJsx('const node = <Widget options={{ [name()]() { return count(); } }} />;'))
    .toBe('const node = <Widget options={() => ({ [name()]() { return count(); } })} />;');
});

it('validates static JSX even without expression containers', () => {
  expect(() => transformSvetJsx('const node = <p></q>;')).toThrow(SyntaxError);
});
