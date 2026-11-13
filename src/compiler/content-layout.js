// Measuring and flattening the content a control draws: text, images, rectangles and the
// containers that arrange them. Part of the component compiler.
import {
  arrange,
  constrained,
  containerTypes,
  intrinsicLength,
  length,
  naturalContainer,
} from '../component-layout.js';
import { literal } from '../component-template.js';
import { own } from '../shared/object.js';
import { contentProps, layoutProps, visualTypes } from './vocabulary.js';

export const num = (v, label) => {
  if (typeof v === 'object' && v?.expr?.endsWith('px')) v = Number(v.expr.slice(0, -2));
  if (typeof v !== 'number' || !Number.isFinite(v) || v < 0 || v > 4096)
    throw Error(`${label}: ожидается размер 0…4096`);
  return v;
};
export const color = v => v?.expr ?? v;
export function insets(v = 0) {
  const a = Array.isArray(v) ? v : [v];
  if (a.length < 1 || a.length > 4) throw Error('padding: от 1 до 4 размеров');
  const x = a.map(v => num(v, 'padding'));
  return x.length === 1
    ? [x[0], x[0], x[0], x[0]]
    : x.length === 2
      ? [x[0], x[1], x[0], x[1]]
      : x.length === 3
        ? [x[0], x[1], x[2], x[1]]
        : x;
}
export function tracks(v) {
  return v === undefined ? [{ expr: '*' }] : Array.isArray(v) ? v : [v];
}
export function cell(n, axis, count) {
  const c = n.props.cell;
  if (c && (!Array.isArray(c) || c.length !== 2 || own(n.props, 'row') || own(n.props, 'column')))
    throw Error('cell: ожидаются row column без отдельных row/column');
  const v = c?.[axis === 'rows' ? 0 : 1] ?? n.props[axis === 'rows' ? 'row' : 'column'];
  if (v === undefined) return null;
  if (!Number.isInteger(v) || v < 1 || v > count) throw Error(`Ячейка за пределами ${axis}`);
  return v - 1;
}
export function gap(p) {
  const a = Array.isArray(p.gap) ? p.gap : [p.gap ?? 0, p.gap ?? 0];
  if (a.length !== 2) throw Error('gap: один или два размера');
  return a.map(v => num(v, 'gap'));
}
// Primary face of a passive Text: 'sans' (default) or 'mono'.
export function fontFamily(p) {
  const family = p.fontFamily ?? p['font.family'] ?? 'sans';
  if (family !== 'sans' && family !== 'mono')
    throw Error("Text.fontFamily ожидает 'sans' или 'mono'");
  return family;
}
// The default family stays implicit so existing scenes compile to the same text.
export const familyField = family => (family === 'sans' ? '' : ` fontFamily: ${family};`);
export function wrapLines(text, fontSize, maxWidth, metrics, family = 'sans') {
  const raw = String(text ?? '').split('\n');
  if (!Number.isFinite(maxWidth) || maxWidth <= 0) return raw;
  const out = [];
  for (const paragraph of raw) {
    const words = paragraph.split(' ');
    let current = words[0] ?? '';
    for (let i = 1; i < words.length; i++) {
      const word = words[i],
        candidate = current ? `${current} ${word}` : word;
      if (metrics.measureText(candidate, fontSize, family)[0] <= maxWidth) current = candidate;
      else {
        out.push(current);
        current = word;
      }
    }
    out.push(current);
  }
  return out;
}
export function natural(n, axis, metrics) {
  const key = axis === 0 ? 'width' : 'height',
    p = n.props;
  if (p[key] !== undefined && !intrinsicLength(p[key]) && !/[*%]$/.test(p[key]?.expr ?? ''))
    return constrained(num(p[key], key), p, axis, 4096);
  if (containerTypes.has(n.type) && n.type !== 'Frame')
    return naturalContainer(n, axis, (child, dimension) => natural(child, dimension, metrics));
  if (n.type === 'Text') {
    if (p.wrap !== undefined && typeof p.wrap !== 'boolean')
      throw Error('Text.wrap ожидает boolean');
    const fs = num(p.fontSize ?? p['font.size'] ?? 16, 'fontSize'),
      family = fontFamily(p);
    if (!metrics?.measureText) throw Error('Размер текста по контенту требует метрик Rust runtime');
    const wrapW =
      p.wrap && axis === 1 && typeof p.width === 'number' ? num(p.width, 'width') : null;
    const lines =
      wrapW !== null
        ? wrapLines(p.text, fs, wrapW, metrics, family)
        : String(p.text ?? '').split('\n');
    const size =
      lines.length === 1
        ? metrics.measureText(lines[0], fs, family)[axis]
        : axis === 1
          ? lines.length * fs * 1.5
          : Math.max(...lines.map(line => metrics.measureText(line, fs, family)[0]));
    if (!Number.isFinite(size) || size < 0) throw Error('Некорректные метрики текста');
    return size;
  }
  if (n.type === 'TextInput') return axis === 0 ? 80 : Number(p.fontSize ?? 15) * 1.5;
  if (n.type === 'Image') return 20;
  if (n.type === 'Rectangle')
    return Math.max(
      0,
      ...n.children.filter(c => visualTypes.has(c.type)).map(c => natural(c, axis, metrics)),
    );
  if (n.type === 'Frame') {
    const ts = tracks(p[axis === 0 ? 'columns' : 'rows']),
      g = gap(p)[axis === 0 ? 1 : 0],
      pad = insets(p.padding);
    return (
      ts.reduce(
        (s, t, i) =>
          s +
          (typeof t === 'number'
            ? num(t, key)
            : Math.max(
                0,
                ...n.children
                  .filter(
                    c =>
                      cell(c, axis === 0 ? 'columns' : 'rows', ts.length) === i || ts.length === 1,
                  )
                  .map(c => natural(c, axis, metrics)),
              )),
        0,
      ) +
      g * (ts.length - 1) +
      pad[axis === 0 ? 1 : 0] +
      pad[axis === 0 ? 3 : 2]
    );
  }
  throw Error(`Контент ${n.type} пока не поддерживается`);
}
export function sizes(n, axis, available, metrics) {
  const name = axis === 0 ? 'columns' : 'rows',
    ts = tracks(n.props[name]),
    g = gap(n.props)[axis === 0 ? 1 : 0];
  let weight = 0;
  const result = ts.map((t, i) => {
    const x = t?.expr ?? t;
    if (typeof x === 'number') return num(x, name);
    if (x === '-' || x === 'content' || x === 'auto')
      return Math.max(
        0,
        ...n.children
          .filter(c => cell(c, name, ts.length) === i || ts.length === 1)
          .map(c => natural(c, axis, metrics)),
      );
    if (typeof x === 'string' && /^(?:\d+(?:\.\d+)?)?\*$/.test(x)) {
      const w = x === '*' ? 1 : Number(x.slice(0, -1));
      if (w <= 0) throw Error('Вес * должен быть положительным');
      weight += w;
      return { w };
    }
    if (typeof x === 'string' && /^\d+(?:\.\d+)?%$/.test(x))
      return (available * Number(x.slice(0, -1))) / 100;
    throw Error(`Неподдерживаемый размер ${x}`);
  });
  const remaining = Math.max(
    0,
    available -
      g * (ts.length - 1) -
      result.reduce((s, v) => s + (typeof v === 'number' ? v : 0), 0),
  );
  return result.map(v => (typeof v === 'number' ? v : (remaining * v.w) / weight));
}
export function flatten(n, box, files, output, metrics) {
  // Keys belong to the inspector snapshot. Lowering must not mutate that tree.
  const p = own(n.props, 'key')
      ? Object.fromEntries(Object.entries(n.props).filter(([k]) => k !== 'key'))
      : n.props,
    allowed = contentProps.get(n.type) ?? layoutProps;
  for (const k of Object.keys(p))
    if (!allowed.has(k)) throw Error(`${n.type}.${k} пока не поддерживается в контенте`);
  if (Object.keys(n.events ?? {}).length || Object.keys(n.bindings ?? {}).length)
    throw Error('Контент кнопки пока не содержит отдельных интерактивных элементов');
  let [x, y, w, h] = box;
  const nw = constrained(
      p.width === undefined
        ? w
        : length(
            p.width,
            w,
            intrinsicLength(p.width)
              ? natural({ ...n, props: { ...p, width: undefined } }, 0, metrics)
              : 0,
            'width',
          ),
      p,
      0,
      w,
    ),
    nh = constrained(
      p.height === undefined
        ? h
        : length(
            p.height,
            h,
            intrinsicLength(p.height)
              ? natural({ ...n, props: { ...p, width: nw, height: undefined } }, 1, metrics)
              : 0,
            'height',
          ),
      p,
      1,
      h,
    );
  x += (w - nw) / 2;
  y += (h - nh) / 2;
  w = nw;
  h = nh;
  const visual = metrics.visuals
    ? {
        id: metrics.visuals.length,
        parent: metrics.parentVisual ?? null,
        type: n.type,
        source: n.source,
        propertySources: n.propertySources,
        propertyOrigins: n.propertyOrigins,
        props: p,
        bounds: [x, y, w, h],
      }
    : null;
  if (visual) metrics.visuals.push(visual);
  const childMetrics = visual ? { ...metrics, parentVisual: visual.id } : metrics;
  if (n.type === 'Rectangle') {
    const radius = num(p.radius ?? 0, 'radius');
    if (
      p.background !== undefined &&
      !(
        typeof color(p.background) === 'string' &&
        /^#(?:[\da-f]{3}|[\da-f]{6}|[\da-f]{8})$/i.test(color(p.background))
      )
    )
      throw Error(
        'Rectangle.background в контенте принимает только hex; Brush и его состояния пока не поддерживаются',
      );
    // A native clip rounds an ordinary polygon without a new Rust primitive.
    // Children share this bounded surface; use a Frame child for grid layout.
    output.push(`ContentClip { x: ${x}; y: ${y}; width: ${w}; height: ${h}; radius: ${radius}; }`);
    if (p.background !== undefined && (metrics.preserveEmptyGeometry || (w > 0 && h > 0)))
      output.push(
        `ContentShape { points: '${x} ${y} ${x + w} ${y} ${x + w} ${y + h} ${x} ${y + h}'; color: ${color(p.background)}; }`,
      );
    for (const child of n.children) flatten(child, [x, y, w, h], files, output, childMetrics);
    output.push('ContentClipEnd {}');
    return;
  }
  if (containerTypes.has(n.type) && n.type !== 'Frame') {
    if (p.clip !== undefined && typeof p.clip !== 'boolean')
      throw Error(`${n.type}.clip ожидает boolean`);
    if (p.clip)
      output.push(
        `ContentClip { x: ${x}; y: ${y}; width: ${w}; height: ${h}; radius: ${num(p.radius ?? 0, 'radius')}; }`,
      );
    const layout = arrange(n, [x, y, w, h], (child, axis) => natural(child, axis, metrics));
    if (visual && layout.grid) visual.grid = layout.grid;
    n.children.forEach((child, index) =>
      flatten(
        {
          ...child,
          props: { ...child.props, width: layout.boxes[index][2], height: layout.boxes[index][3] },
        },
        layout.boxes[index],
        files,
        output,
        childMetrics,
      ),
    );
    if (p.clip) output.push('ContentClipEnd {}');
    return;
  }
  if (n.type === 'Frame') {
    if (p.clip !== undefined && typeof p.clip !== 'boolean')
      throw Error('Frame.clip ожидает boolean');
    const radius = num(p.radius ?? 0, 'radius');
    if (p.clip)
      output.push(
        `ContentClip { x: ${x}; y: ${y}; width: ${w}; height: ${h}; radius: ${radius}; }`,
      );
    const pad = insets(p.padding);
    x += pad[3];
    y += pad[0];
    w = Math.max(0, w - pad[1] - pad[3]);
    h = Math.max(0, h - pad[0] - pad[2]);
    const cols = sizes(n, 0, w, metrics),
      rows = sizes(n, 1, h, metrics),
      g = gap(p);
    if (visual) visual.grid = { bounds: [x, y, w, h], columns: cols, rows, gap: g };
    for (const c of n.children) {
      const ci = cell(c, 'columns', cols.length),
        ri = cell(c, 'rows', rows.length);
      flatten(
        c,
        [
          ci === null ? x : x + cols.slice(0, ci).reduce((a, b) => a + b, 0) + ci * g[1],
          ri === null ? y : y + rows.slice(0, ri).reduce((a, b) => a + b, 0) + ri * g[0],
          ci === null ? w : cols[ci],
          ri === null ? h : rows[ri],
        ],
        files,
        output,
        childMetrics,
      );
    }
    if (p.clip) output.push('ContentClipEnd {}');
    return;
  }
  if (n.children?.length) throw Error(`${n.type} не принимает дочерние элементы`);
  if (n.type === 'TextInput') {
    output.push(
      `ContentInput { x: ${x}; y: ${y}; width: ${w}; height: ${h}; value: ${literal(String(p.value ?? ''))}; placeholder: ${literal(String(p.placeholder ?? ''))}; color: ${literal(p.color ?? { expr: '#ffffff' })}; placeholderColor: ${literal(p.placeholderColor ?? { expr: '#999999' })}; fontSize: ${num(p.fontSize ?? 15, 'fontSize')}; multiline: ${Boolean(p.multiline)}; }`,
    );
    return;
  }
  if (n.type === 'Text') {
    if (p.wrap !== undefined && typeof p.wrap !== 'boolean')
      throw Error('Text.wrap ожидает boolean');
    const text = String(p.text ?? ''),
      fontSize = num(p.fontSize ?? p['font.size'] ?? 16, 'fontSize'),
      family = fontFamily(p);
    if (p.wrap && !metrics?.measureText) throw Error('Перенос текста требует метрик Rust runtime');
    const lines = p.wrap ? wrapLines(text, fontSize, w, metrics, family) : text.split('\n');
    if (lines.length === 1)
      output.push(
        `ContentText { x: ${x}; y: ${y}; width: ${w}; height: ${h}; text: ${literal(lines[0])}; color: ${literal(p.color ?? { expr: '#ffffff' })}; fontSize: ${fontSize};${familyField(family)} }`,
      );
    else {
      if (!metrics?.measureText) throw Error('Многострочный текст требует метрик Rust runtime');
      output.push(`ContentClip { x: ${x}; y: ${y}; width: ${w}; height: ${h}; radius: 0; }`);
      lines.forEach((line, i) =>
        output.push(
          `ContentText { x: ${x}; y: ${y + i * fontSize * 1.5}; width: ${metrics.measureText(line, fontSize, family)[0]}; height: ${fontSize * 1.5}; text: ${literal(line)}; color: ${literal(p.color ?? { expr: '#ffffff' })}; fontSize: ${fontSize};${familyField(family)} }`,
        ),
      );
      output.push('ContentClipEnd {}');
    }
    return;
  }
  if (n.type === 'Image') {
    if (typeof p.source !== 'string' || !own(files, p.source))
      throw Error(`SVG-файл не найден в проекте: ${p.source}`);
    const content = metrics.imageContent(
      files[p.source],
      [x, y, w, h],
      color(p.color ?? '#ffffff'),
    );
    if (content) output.push(content);
    return;
  }
  throw Error(`Неподдерживаемый контент ${n.type}`);
}
