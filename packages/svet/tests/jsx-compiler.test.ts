import { expect, it } from 'vitest';
import { transformSvetJsx } from '../src/engine/compiler.ts';

it('transforms every adjacent JSX expression rather than only the first', () => {
  expect(transformSvetJsx('const node = <p>{first()}{second()}</p>;')).toBe(
    'const node = <p>{() => (first())}{() => (second())}</p>;',
  );
});

it('does not mistake calls inside literals for reactive JavaScript', () => {
  const code = 'const node = <p>{"call()"}{/call\\(\\)/.source}</p>;';
  expect(transformSvetJsx(code)).toBe(code);
});

it('handles braces inside comments and regex literals', () => {
  const code = 'const node = <p>{/* } */ count() + /}/.test(label())}</p>;';
  expect(transformSvetJsx(code)).toBe(
    'const node = <p>{/* } */ () => (count() + /}/.test(label()))}</p>;',
  );
});

it('preserves unrelated JavaScript comparisons and object literals', () => {
  const code =
    'const check = a > b; const object = { value: count() }; const node = <p>{count()}</p>;';
  expect(transformSvetJsx(code)).toBe(
    'const check = a > b; const object = { value: count() }; const node = <p>{() => (count())}</p>;',
  );
});

it('preserves async and type-annotated callbacks while compiling nested JSX', () => {
  const code =
    'const node = <Widget onClick={async () => count()} render={(item: string) => <p>{count()}</p>} />;';
  expect(transformSvetJsx(code)).toBe(
    'const node = <Widget onClick={async () => count()} render={(item: string) => <p>{() => (count())}</p>} />;',
  );
});

it('makes signal-dependent object styles lazy', () => {
  expect(transformSvetJsx('const node = <p style={{ color: color() }} />;')).toBe(
    'const node = <p style={() => ({ color: color() })} />;',
  );
});

it('does not wrap calls deferred inside object methods but tracks computed method names', () => {
  expect(
    transformSvetJsx('const node = <Widget options={{ getValue() { return count(); } }} />;'),
  ).toBe('const node = <Widget options={{ getValue() { return count(); } }} />;');
  expect(
    transformSvetJsx('const node = <Widget options={{ [name()]() { return count(); } }} />;'),
  ).toBe('const node = <Widget options={() => ({ [name()]() { return count(); } })} />;');
});

it('validates static JSX even without expression containers', () => {
  expect(() => transformSvetJsx('const node = <p></q>;')).toThrow(SyntaxError);
});

it('does not wrap pure static built-in and constant calls in reactive thunks', () => {
  const staticCases = [
    'const SCALE = 4; const node = <p data-v={Math.max(1, Math.round(Math.PI * SCALE))}>{String(42)}</p>;',
    'const node = <p>{JSON.stringify({ ok: true, list: [1, 2] })}</p>;',
    'const node = <p>{" svet ".trim().toUpperCase()}</p>;',
    'const node = <p>{(42.125).toFixed(2)}</p>;',
    'const node = <p>{[1, 2, 3].join("-")}</p>;',
    'const node = <p>{encodeURIComponent("a b") + ":" + Number.isFinite(10)}</p>;',
    'const node = <p>{1 + Math.abs(-5)}</p>;',
  ];
  for (const code of staticCases) {
    expect(transformSvetJsx(code)).toBe(code);
  }
});

it('still wraps impure, reactive-argument, or lexically shadowed calls', () => {
  expect(transformSvetJsx('const node = <p>{Math.random()}</p>;')).toBe(
    'const node = <p>{() => (Math.random())}</p>;',
  );
  expect(transformSvetJsx('const node = <p>{new Date().toISOString()}</p>;')).toBe(
    'const node = <p>{() => (new Date().toISOString())}</p>;',
  );
  expect(transformSvetJsx('const node = <p>{Math.max(count(), 2)}</p>;')).toBe(
    'const node = <p>{() => (Math.max(count(), 2))}</p>;',
  );
  expect(transformSvetJsx('const node = <p>{Math.round(unknownValue)}</p>;')).toBe(
    'const node = <p>{() => (Math.round(unknownValue))}</p>;',
  );
  expect(
    transformSvetJsx(
      'function App() { const Math = { max: () => count() }; return <p>{Math.max(1, 2)}</p>; }',
    ),
  ).toBe(
    'function App() { const Math = { max: () => count() }; return <p>{() => (Math.max(1, 2))}</p>; }',
  );
  expect(
    transformSvetJsx(
      'function App() { const String = () => label(); return <p>{String("a")}</p>; }',
    ),
  ).toBe('function App() { const String = () => label(); return <p>{() => (String("a"))}</p>; }');
  expect(
    transformSvetJsx('const node = <p>{JSON.stringify({ get v() { return count(); } })}</p>;'),
  ).toBe('const node = <p>{() => (JSON.stringify({ get v() { return count(); } }))}</p>;');
});
