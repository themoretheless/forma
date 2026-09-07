// Semantic values adapted from the supplied Unified Style Guide, 2026-09-06.
// Decorative accent keeps the reference hue; action fills have their own contrast budget.
export const themes = Object.freeze({
  light: Object.freeze({
    canvas: '#F4F1EA', surface: '#FBF9F4', surfaceRaised: '#FFFFFF',
    surfaceHover: '#EEE9DF', surfacePressed: '#EBE5DC', border: '#DED7CA', borderStrong: '#C8BFB0',
    text: '#2B2A26', textMuted: '#6B665C', textDisabled: '#A39C90', focus: '#A6470C',
    accent: '#DE6A19', accentWash: '#F5E5D5', accentInk: '#93420E', reveal: '#DE6A19F2',
    primary: '#B9510F', primaryHover: '#A6470C', primaryPressed: '#913F0B', primaryText: '#FFFFFF',
    danger: '#C23625', dangerHover: '#AA2E20', dangerPressed: '#92261B', dangerText: '#FFFFFF',
    dangerInk: '#A42E22', dangerWash: '#F9E4DF',
    success: '#306448', successWash: '#E7F0E8', warning: '#825219', warningWash: '#F5E8CE',
    track: '#E3DCCE',
  }),
  dark: Object.freeze({
    canvas: '#1F1E1B', surface: '#2A2825', surfaceRaised: '#33302C',
    surfaceHover: '#3A3733', surfacePressed: '#403C35', border: '#54504A', borderStrong: '#6E675F',
    text: '#F1E6C4', textMuted: '#B8AA8F', textDisabled: '#7A736A', focus: '#FFAC72',
    accent: '#F2853A', accentWash: '#443122', accentInk: '#FFAC72', reveal: '#FFA45CF2',
    primary: '#F2853A', primaryHover: '#FF9A52', primaryPressed: '#DF752F', primaryText: '#1F1E1B',
    danger: '#C23625', dangerHover: '#AA2E20', dangerPressed: '#92261B', dangerText: '#FFFFFF',
    dangerInk: '#FFAEA1', dangerWash: '#492C27',
    success: '#83BF9C', successWash: '#293B30', warning: '#E6B96D', warningWash: '#423725',
    track: '#47423B',
  }),
});

// The 18/22px specimens in the reference are too small for our default controls.
export const sizes = Object.freeze({
  compact: Object.freeze({ height: 28, fontSize: 13, horizontalPadding: 10, radius: 6, iconSize: 14 }),
  regular: Object.freeze({ height: 32, fontSize: 14, horizontalPadding: 12, radius: 8, iconSize: 16 }),
  comfortable: Object.freeze({ height: 40, fontSize: 15, horizontalPadding: 16, radius: 9, iconSize: 18 }),
  touch: Object.freeze({ height: 44, fontSize: 16, horizontalPadding: 16, radius: 10, iconSize: 20 }),
});

export function themeState(name = 'light', { reducedMotion = false } = {}) {
  const theme = themes[name];
  if (!Object.hasOwn(themes, name)) throw new Error(`Unknown Forma theme: ${name}`);
  return {
    theme: Object.fromEntries(Object.entries(theme).map(([key, value]) => [key, { expr: value }])),
    motion: {
      control: { expr: reducedMotion ? '0ms' : '180ms' },
      reveal: { expr: reducedMotion ? '0ms' : '280ms' },
    },
  };
}

// Produce standalone .ui source files for Studio/native compilation. This only
// substitutes named theme constants in our library; it is not a bindings engine.
function materializeSource(source, state) {
  let result = '', cursor = 0;
  while (cursor < source.length) {
    const start = cursor, char = source[cursor];
    if (char === '\'' || char === '"') {
      cursor++;
      while (cursor < source.length) {
        if (source[cursor] === '\\') cursor = Math.min(source.length, cursor + 2);
        else if (source[cursor++] === char) break;
      }
    } else if (source.startsWith('//', cursor)) {
      const end = source.indexOf('\n', cursor + 2);
      cursor = end < 0 ? source.length : end;
    } else if (source.startsWith('/*', cursor)) {
      let depth = 1;
      cursor += 2;
      while (cursor < source.length && depth) {
        if (source.startsWith('/*', cursor)) { depth++; cursor += 2; }
        else if (source.startsWith('*/', cursor)) { depth--; cursor += 2; }
        else cursor++;
      }
    } else if (/[A-Za-z_]/.test(char)) {
      cursor++;
      while (cursor < source.length && /[\w.-]/.test(source[cursor])) cursor++;
    } else cursor++;
    const token = source.slice(start, cursor);
    const constant = /^state\.(theme|motion)\.(.+)$/.exec(token);
    if (constant) {
      const [, scope, key] = constant;
      if (!Object.hasOwn(state[scope], key)) throw new Error(`Unknown theme constant: ${token}`);
      result += state[scope][key].expr;
    } else result += token;
  }
  return result;
}

export function materializeTheme(files, name = 'light', options = {}) {
  const state = themeState(name, options);
  return Object.fromEntries(Object.entries(files).map(([path, source]) => [path,
    path.endsWith('.ui') ? materializeSource(source, state) : source,
  ]));
}

export function contrastRatio(foreground, background) {
  const luminance = hex => {
    if (!/^#[0-9a-f]{6}$/i.test(hex)) throw new Error('Contrast expects opaque #RRGGBB colors');
    const rgb = [1, 3, 5].map(i => parseInt(hex.slice(i, i + 2), 16) / 255)
      .map(c => c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4);
    return rgb[0] * 0.2126 + rgb[1] * 0.7152 + rgb[2] * 0.0722;
  };
  const a = luminance(foreground), b = luminance(background);
  return (Math.max(a, b) + 0.05) / (Math.min(a, b) + 0.05);
}
