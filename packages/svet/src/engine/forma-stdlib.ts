/**
 * # Forma Standard Library (`formaStdlib`) & Pipe Expression Evaluator (`forma-stdlib.ts`)
 *
 * Implements Forma's complete `stdlib`: math (`clamp`, `lerp`, `remap`, `sum`, `avg`),
 * strings & i18n (`plural`, `upper`, `lower`, `capitalize`, `truncate`, `padStart`),
 * collections (`filter`, `map`, `join`, `take`, `skip`, `unique`), and WCAG color
 * functions (`lighten`, `darken`, `alpha`, `luminance`, `contrast`).
 */

function parseHexToRgb(hex: string): [number, number, number] {
  const clean = hex.replace(/^#/, '').trim();
  if (clean.length === 3) {
    return [
      parseInt(clean[0] + clean[0], 16),
      parseInt(clean[1] + clean[1], 16),
      parseInt(clean[2] + clean[2], 16),
    ];
  }
  const num = parseInt(clean.slice(0, 6), 16) || 0;
  return [(num >> 16) & 255, (num >> 8) & 255, num & 255];
}

function rgbToHex(r: number, g: number, b: number): string {
  const clampByte = (n: number) => Math.max(0, Math.min(255, Math.round(n)));
  return (
    '#' +
    [clampByte(r), clampByte(g), clampByte(b)]
      .map((x) => x.toString(16).padStart(2, '0'))
      .join('')
  );
}

function relativeLuminance(hex: string): number {
  const [r, g, b] = parseHexToRgb(hex).map((v) => {
    const s = v / 255;
    return s <= 0.03928 ? s / 12.92 : Math.pow((s + 0.055) / 1.055, 2.4);
  });
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

export const formaStdlib: Record<string, (...args: any[]) => any> = {
  // Math
  min: (...args: number[]) => Math.min(...args),
  max: (...args: number[]) => Math.max(...args),
  clamp: (val: number, min: number, max: number) => Math.min(Math.max(val, min), max),
  abs: (val: number) => Math.abs(val),
  round: (val: number) => Math.round(val),
  floor: (val: number) => Math.floor(val),
  ceil: (val: number) => Math.ceil(val),
  pow: (a: number, b: number) => Math.pow(a, b),
  sqrt: (val: number) => Math.sqrt(val),
  sign: (val: number) => Math.sign(val),
  lerp: (a: number, b: number, t: number) => a + (b - a) * t,
  remap: (v: number, inMin: number, inMax: number, outMin: number, outMax: number) =>
    outMin + ((v - inMin) / (inMax - inMin || 1)) * (outMax - outMin),
  sum: (arr: number[]) => (Array.isArray(arr) ? arr.reduce((a, b) => a + Number(b), 0) : 0),
  avg: (arr: number[]) =>
    Array.isArray(arr) && arr.length > 0
      ? arr.reduce((a, b) => a + Number(b), 0) / arr.length
      : 0,

  // Strings & i18n
  len: (v: any) =>
    v == null
      ? 0
      : typeof v === 'string' || Array.isArray(v)
        ? v.length
        : Object.keys(v).length,
  trim: (s: string) => String(s ?? '').trim(),
  upper: (s: string) => String(s ?? '').toUpperCase(),
  toUpperCase: (s: string) => String(s ?? '').toUpperCase(),
  lower: (s: string) => String(s ?? '').toLowerCase(),
  toLowerCase: (s: string) => String(s ?? '').toLowerCase(),
  capitalize: (s: string) => {
    const str = String(s ?? '');
    return str.charAt(0).toUpperCase() + str.slice(1);
  },
  contains: (s: any, sub: any) =>
    Array.isArray(s) ? s.includes(sub) : String(s ?? '').includes(String(sub)),
  startsWith: (s: string, prefix: string) => String(s ?? '').startsWith(prefix),
  endsWith: (s: string, suffix: string) => String(s ?? '').endsWith(suffix),
  replace: (s: string, from: string, to: string) => String(s ?? '').replaceAll(from, to),
  slice: (s: any, start: number, end?: number) => (s ? s.slice(start, end) : ''),
  split: (s: string, sep: string) => String(s ?? '').split(sep),
  padStart: (s: string, targetLen: number, pad = ' ') => String(s ?? '').padStart(targetLen, pad),
  padEnd: (s: string, targetLen: number, pad = ' ') => String(s ?? '').padEnd(targetLen, pad),
  repeat: (s: string, count: number) => String(s ?? '').repeat(Math.max(0, count)),
  truncate: (s: string, maxLen: number, ellipsis = '…') => {
    const str = String(s ?? '');
    return str.length > maxLen
      ? str.slice(0, Math.max(0, maxLen - ellipsis.length)) + ellipsis
      : str;
  },
  plural: (count: number, one: string, few: string, many?: string) => {
    const n = Math.abs(Math.trunc(Number(count) || 0));
    if (many === undefined) {
      return n === 1 ? one : few;
    }
    const mod10 = n % 10;
    const mod100 = n % 100;
    if (mod10 === 1 && mod100 !== 11) return one;
    if (mod10 >= 2 && mod10 <= 4 && (mod100 < 12 || mod100 > 14)) return few;
    return many;
  },

  // Lists & Records
  count: (arr: any[]) => (Array.isArray(arr) ? arr.length : 0),
  first: (arr: any[]) => (Array.isArray(arr) ? arr[0] : undefined),
  last: (arr: any[]) => (Array.isArray(arr) && arr.length > 0 ? arr[arr.length - 1] : undefined),
  join: (arr: any[], sep = ', ') => (Array.isArray(arr) ? arr.join(sep) : ''),
  reverse: (arr: any[]) => (Array.isArray(arr) ? [...arr].reverse() : []),
  take: (arr: any[], n: number) => (Array.isArray(arr) ? arr.slice(0, n) : []),
  skip: (arr: any[], n: number) => (Array.isArray(arr) ? arr.slice(n) : []),
  map: (arr: any[], fieldOrFn: string | ((x: any) => any)) => {
    if (!Array.isArray(arr)) return [];
    if (typeof fieldOrFn === 'function') return arr.map(fieldOrFn);
    return arr.map((item) => (item && typeof item === 'object' ? item[fieldOrFn] : item));
  },
  filter: (arr: any[], fieldOrFn: string | ((x: any) => boolean)) => {
    if (!Array.isArray(arr)) return [];
    if (typeof fieldOrFn === 'function') return arr.filter(fieldOrFn);
    return arr.filter((item) =>
      Boolean(item && typeof item === 'object' ? item[fieldOrFn] : item)
    );
  },
  unique: (arr: any[]) => (Array.isArray(arr) ? Array.from(new Set(arr)) : []),
  range: (start: number, end: number) =>
    Array.from({ length: Math.max(0, end - start) }, (_, i) => start + i),
  keys: (obj: Record<string, any>) => (obj && typeof obj === 'object' ? Object.keys(obj) : []),
  values: (obj: Record<string, any>) => (obj && typeof obj === 'object' ? Object.values(obj) : []),

  // Predicates & Casts
  String: (v: any) => String(v ?? ''),
  Number: (v: any) => Number(v ?? 0),
  Bool: (v: any) => Boolean(v),
  isEmpty: (v: any) =>
    v == null ||
    v === '' ||
    (Array.isArray(v) && v.length === 0) ||
    (typeof v === 'object' && Object.keys(v).length === 0),
  isNull: (v: any) => v === null || v === undefined,
  coalesce: (...args: any[]) => args.find((a) => a !== null && a !== undefined),

  // WCAG Color Functions
  rgb: (r: number, g: number, b: number) => rgbToHex(r, g, b),
  rgba: (r: number, g: number, b: number, a: number) => `rgba(${r}, ${g}, ${b}, ${a})`,
  alpha: (hex: string, a: number) => {
    const [r, g, b] = parseHexToRgb(hex);
    return `rgba(${r}, ${g}, ${b}, ${a})`;
  },
  lighten: (hex: string, amount: number) => {
    const [r, g, b] = parseHexToRgb(hex);
    const factor = amount <= 1 ? amount * 255 : amount;
    return rgbToHex(r + factor, g + factor, b + factor);
  },
  darken: (hex: string, amount: number) => {
    const [r, g, b] = parseHexToRgb(hex);
    const factor = amount <= 1 ? amount * 255 : amount;
    return rgbToHex(r - factor, g - factor, b - factor);
  },
  luminance: (hex: string) => Number(relativeLuminance(hex).toFixed(4)),
  contrast: (hexA: string, hexB: string) => {
    const l1 = relativeLuminance(hexA);
    const l2 = relativeLuminance(hexB);
    const brightest = Math.max(l1, l2);
    const darkest = Math.min(l1, l2);
    return Number(((brightest + 0.05) / (darkest + 0.05)).toFixed(2));
  },
};

/**
 * Transforms Forma pipe syntax `a |> fn(b, c) |> fn2()` into standard nested calls `fn2(fn(a, b, c))`.
 */
export function transformFormaPipes(expr: string): string {
  if (!expr.includes('|>')) return expr;
  const stages = expr.split('|>').map((s) => s.trim());
  let acc = stages[0];
  for (let i = 1; i < stages.length; i++) {
    const stage = stages[i];
    const openParen = stage.indexOf('(');
    if (openParen !== -1 && stage.endsWith(')')) {
      const fnName = stage.slice(0, openParen).trim();
      const argsInner = stage.slice(openParen + 1, -1).trim();
      acc = argsInner.length > 0 ? `${fnName}(${acc}, ${argsInner})` : `${fnName}(${acc})`;
    } else {
      acc = `${stage}(${acc})`;
    }
  }
  return acc;
}
