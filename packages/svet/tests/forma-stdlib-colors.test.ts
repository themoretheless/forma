import { expect, it } from 'vitest';
import { formaStdlib, transformFormaPipes } from '../src/engine/forma-stdlib';

const call = (name: string, ...args: unknown[]) => formaStdlib[name]?.(...args);

it('reads three-digit and six-digit hex colours the same way', () => {
  expect(call('alpha', '#f80', 0.5)).toBe('rgba(255, 136, 0, 0.5)');
  expect(call('alpha', '#ff8800', 0.5)).toBe('rgba(255, 136, 0, 0.5)');
  expect(call('alpha', 'abc', 1)).toBe('rgba(170, 187, 204, 1)');
  // Anything that is not hex reads as black rather than failing.
  expect(call('alpha', '#zzzzzz', 1)).toBe('rgba(0, 0, 0, 1)');
});

it('computes WCAG relative luminance and contrast', () => {
  expect(call('luminance', '#ffffff')).toBe(1);
  expect(call('luminance', '#fff')).toBe(1);
  expect(call('luminance', '#000000')).toBe(0);
  expect(call('luminance', '#ff0000')).toBe(0.2126);
  expect(call('luminance', '#00ff00')).toBe(0.7152);
  expect(call('luminance', '#0000ff')).toBe(0.0722);
  expect(call('contrast', '#000000', '#ffffff')).toBe(21);
  expect(call('contrast', '#fff', '#000')).toBe(21);
  expect(call('contrast', '#777777', '#777777')).toBe(1);
});

it('lightens and darkens by a fraction or by a byte amount, clamped to the byte range', () => {
  expect(call('lighten', '#000000', 0.5)).toBe('#808080');
  expect(call('lighten', '#f0f0f0', 64)).toBe('#ffffff');
  expect(call('darken', '#ffffff', 1)).toBe('#000000');
  expect(call('darken', '#101010', 32)).toBe('#000000');
});

it('rewrites pipes into nested calls from left to right', () => {
  expect(transformFormaPipes('a')).toBe('a');
  expect(transformFormaPipes('a |> f')).toBe('f(a)');
  expect(transformFormaPipes('a |> f()')).toBe('f(a)');
  expect(transformFormaPipes('a |> f(b, c)')).toBe('f(a, b, c)');
  expect(transformFormaPipes('a |> f(b) |> g() |> h')).toBe('h(g(f(a, b)))');
  expect(transformFormaPipes(' |> f')).toBe('f()');
});
