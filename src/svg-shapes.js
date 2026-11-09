// Deliberately bounded SVG subset. Never executes SVG, fetches URLs or embeds HTML.
// Unsupported SVG features are diagnostics, not silently missing artwork.
// Parsing is box-independent: a repeated icon tessellates once and is then placed
// per control, so callers must not re-parse the same source for every surface.
const UNIT_CIRCLE = Array.from({ length: 64 }, (_, i) => [
  Math.cos((i * Math.PI) / 32),
  Math.sin((i * Math.PI) / 32),
]);
const HEX_COLOR_RE = /^#(?:[\da-f]{3}|[\da-f]{4}|[\da-f]{6}|[\da-f]{8})$/i;
const ATTR_RE = /\s+([\w:-]+)\s*=\s*(['"])(.*?)\2/gy;
const ALLOWED_TAGS = new Set([
  'svg',
  'rect',
  'circle',
  'ellipse',
  'line',
  'polyline',
  'polygon',
  'path',
]);
const ALLOWED_ATTRS = {
  svg: new Set(['xmlns', 'viewBox', 'width', 'height']),
  rect: new Set(['fill', 'stroke', 'stroke-width', 'x', 'y', 'width', 'height']),
  circle: new Set(['fill', 'stroke', 'stroke-width', 'cx', 'cy', 'r']),
  ellipse: new Set(['fill', 'stroke', 'stroke-width', 'cx', 'cy', 'rx', 'ry']),
  line: new Set(['fill', 'stroke', 'stroke-width', 'x1', 'y1', 'x2', 'y2']),
  polyline: new Set(['fill', 'stroke', 'stroke-width', 'points']),
  polygon: new Set(['fill', 'stroke', 'stroke-width', 'points']),
  path: new Set(['fill', 'stroke', 'stroke-width', 'd']),
};
const MAX_PATH_POINTS = 4096,
  MAX_CURVE_STEPS = 128;
const PATH_TOKEN_RE = /[\s,]*(?:([a-zA-Z])|([-+]?(?:\d+\.?\d*|\.\d+)(?:[eE][-+]?\d+)?))/y;
const steps = (deviation, factor, tolerance) =>
  Math.min(MAX_CURVE_STEPS, Math.max(1, Math.ceil(Math.sqrt((factor * deviation) / tolerance))));
const secondDifference = (a, b, c) => Math.hypot(a[0] - 2 * b[0] + c[0], a[1] - 2 * b[1] + c[1]);
const reflect = (control, at) => (control ? [2 * at[0] - control[0], 2 * at[1] - control[1]] : at);
// Endpoint arc (SVG 1.1 F.6.5) as chords; a zero radius is a straight line.
function arc(out, from, rx, ry, rotation, large, sweep, to, tolerance) {
  rx = Math.abs(rx);
  ry = Math.abs(ry);
  if (from[0] === to[0] && from[1] === to[1]) return;
  if (!rx || !ry) {
    out.push(to);
    return;
  }
  const angle = (rotation * Math.PI) / 180,
    sin = Math.sin(angle),
    cos = Math.cos(angle),
    dx = (from[0] - to[0]) / 2,
    dy = (from[1] - to[1]) / 2;
  const x1 = cos * dx + sin * dy,
    y1 = -sin * dx + cos * dy,
    scale = (x1 * x1) / (rx * rx) + (y1 * y1) / (ry * ry);
  if (scale > 1) {
    rx *= Math.sqrt(scale);
    ry *= Math.sqrt(scale);
  }
  let factor = Math.sqrt(
    Math.max(
      0,
      (rx * rx * ry * ry - rx * rx * y1 * y1 - ry * ry * x1 * x1) /
        (rx * rx * y1 * y1 + ry * ry * x1 * x1),
    ),
  );
  if (large === sweep) factor = -factor;
  const cx1 = (factor * rx * y1) / ry,
    cy1 = (-factor * ry * x1) / rx,
    cx = cos * cx1 - sin * cy1 + (from[0] + to[0]) / 2,
    cy = sin * cx1 + cos * cy1 + (from[1] + to[1]) / 2;
  const start = Math.atan2((y1 - cy1) / ry, (x1 - cx1) / rx);
  let delta = Math.atan2((-y1 - cy1) / ry, (-x1 - cx1) / rx) - start;
  if (sweep && delta < 0) delta += 2 * Math.PI;
  else if (!sweep && delta > 0) delta -= 2 * Math.PI;
  const step = 2 * Math.acos(1 - Math.min(1, tolerance / Math.max(rx, ry))),
    n =
      step > 0
        ? Math.min(MAX_CURVE_STEPS, Math.max(1, Math.ceil(Math.abs(delta) / step)))
        : MAX_CURVE_STEPS;
  for (let i = 1; i < n; i++) {
    const a = start + (delta * i) / n,
      s = Math.sin(a),
      c = Math.cos(a);
    out.push([cx + cos * rx * c - sin * ry * s, cy + sin * rx * c + cos * ry * s]);
  }
  out.push(to);
}
// SVG path data flattened to polylines; curves and arcs are subdivided so the
// chord error stays under `tolerance` (viewBox units). The Rust twin is
// `flatten_svg_path` in vector-ui/src/form_document/svg_path.rs; keep the two in step.
export function flattenSvgPath(d, tolerance) {
  const fail = message => {
    throw Error(`SVG path: ${message}`);
  };
  if (typeof d !== 'string') fail('нет данных d');
  let at = 0,
    command = '',
    started = false,
    current = [0, 0],
    start = [0, 0],
    cubicControl = null,
    quadControl = null,
    points = [],
    total = 0;
  const subpaths = [];
  const peek = () => {
    PATH_TOKEN_RE.lastIndex = at;
    return PATH_TOKEN_RE.exec(d);
  };
  const number = () => {
    const m = peek();
    if (!m || m[2] === undefined) fail('ожидалось число');
    at = PATH_TOKEN_RE.lastIndex;
    const n = Number(m[2]);
    if (!Number.isFinite(n) || Math.abs(n) > 100_000) fail('неверная координата');
    return n;
  };
  // Arc flags are single digits and may be written without separators.
  const flag = () => {
    const m = /[\s,]*([01])/y;
    m.lastIndex = at;
    const f = m.exec(d);
    if (!f) fail('неверный флаг дуги');
    at = m.lastIndex;
    return f[1] === '1';
  };
  const flush = closed => {
    if (points.length > 1) subpaths.push({ points, closed });
    points = [];
  };
  for (;;) {
    const token = peek();
    if (!token) {
      if (d.slice(at).replace(/[\s,]/g, '')) fail('некорректные данные');
      break;
    }
    if (token[1] !== undefined) {
      command = token[1];
      at = PATH_TOKEN_RE.lastIndex;
    } else if (!command || command === 'z' || command === 'Z') fail('должен начинаться с M');
    const relative = command === command.toLowerCase(),
      base = relative ? current : [0, 0],
      kind = command.toUpperCase();
    const point = () => {
      const x = number(),
        y = number();
      return [base[0] + x, base[1] + y];
    };
    let nextCubic = null,
      nextQuad = null;
    if (kind === 'M') {
      flush(false);
      started = true;
      current = point();
      start = current;
      points.push(current);
      command = relative ? 'l' : 'L';
    } else if (!started) fail('должен начинаться с M');
    else if (kind === 'Z') {
      flush(true);
      current = start;
    } else {
      if (!points.length) points.push(current);
      if (kind === 'L') {
        current = point();
        points.push(current);
      } else if (kind === 'H') {
        current = [base[0] + number(), current[1]];
        points.push(current);
      } else if (kind === 'V') {
        current = [current[0], base[1] + number()];
        points.push(current);
      } else if (kind === 'C' || kind === 'S') {
        const p0 = current,
          p1 = kind === 'C' ? point() : reflect(cubicControl, current),
          p2 = point(),
          p3 = point(),
          n = steps(
            Math.max(secondDifference(p0, p1, p2), secondDifference(p1, p2, p3)),
            0.75,
            tolerance,
          );
        for (let i = 1; i <= n; i++) {
          const t = i / n,
            u = 1 - t,
            a = u * u * u,
            b = 3 * u * u * t,
            c = 3 * u * t * t,
            e = t * t * t;
          points.push([
            a * p0[0] + b * p1[0] + c * p2[0] + e * p3[0],
            a * p0[1] + b * p1[1] + c * p2[1] + e * p3[1],
          ]);
        }
        nextCubic = p2;
        current = p3;
      } else if (kind === 'Q' || kind === 'T') {
        const p0 = current,
          p1 = kind === 'Q' ? point() : reflect(quadControl, current),
          p2 = point(),
          n = steps(secondDifference(p0, p1, p2), 0.25, tolerance);
        for (let i = 1; i <= n; i++) {
          const t = i / n,
            u = 1 - t,
            a = u * u,
            b = 2 * u * t,
            c = t * t;
          points.push([a * p0[0] + b * p1[0] + c * p2[0], a * p0[1] + b * p1[1] + c * p2[1]]);
        }
        nextQuad = p1;
        current = p2;
      } else if (kind === 'A') {
        const rx = number(),
          ry = number(),
          rotation = number(),
          large = flag(),
          sweep = flag(),
          to = point();
        arc(points, current, rx, ry, rotation, large, sweep, to, tolerance);
        current = to;
      } else fail(`команда ${command} не поддерживается`);
    }
    cubicControl = nextCubic;
    quadControl = nextQuad;
    total = subpaths.reduce((sum, sub) => sum + sub.points.length, points.length);
    if (total > MAX_PATH_POINTS) fail('слишком длинный');
  }
  flush(false);
  if (!subpaths.length) fail('нет отрезков');
  return subpaths;
}
// The rasterizer fills one contour at a time, so a contour inside another
// would paint over it instead of cutting a hole.
function overlappingContours(subpaths) {
  const bounds = subpaths
    .filter(sub => sub.points.length > 2)
    .map(sub =>
      sub.points.reduce(
        (b, p) => [
          Math.min(b[0], p[0]),
          Math.min(b[1], p[1]),
          Math.max(b[2], p[0]),
          Math.max(b[3], p[1]),
        ],
        [Infinity, Infinity, -Infinity, -Infinity],
      ),
    );
  return bounds.some((a, i) =>
    bounds.slice(i + 1).some(b => a[0] < b[2] && b[0] < a[2] && a[1] < b[3] && b[1] < a[3]),
  );
}
export function parseSvgShapes(source, currentColor = '#ffffff') {
  if (typeof source !== 'string' || source.length > 100_000)
    throw Error('SVG должен быть текстом до 100 КБ');
  source = source.replace(/<!--[^]*?-->/g, '').replace(/<\?xml[^]*?\?>/g, '');
  const tags = [...source.matchAll(/<([^>]+)>/g)];
  if (source.replace(/<[^>]+>/g, '').trim())
    throw Error('SVG: текстовые элементы пока не поддерживаются');
  const out = [];
  let view = null,
    root = false,
    closed = false;
  function number(v, fallback = 0) {
    const n = v === undefined ? fallback : Number(v);
    if (!Number.isFinite(n) || Math.abs(n) > 100_000) throw Error('SVG: неверная координата');
    return n;
  }
  function ink(v) {
    if (v === 'none') return null;
    if (v === 'currentColor') v = currentColor;
    if (!HEX_COLOR_RE.test(v)) throw Error('SVG: поддерживаются hex и currentColor');
    return v;
  }
  function emit(points, c) {
    if (!c) return;
    out.push({ color: c, points });
  }
  function segment(a, b, width, c) {
    const dx = b[0] - a[0],
      dy = b[1] - a[1],
      l = Math.hypot(dx, dy);
    if (!l) return;
    const x = ((-dy / l) * width) / 2,
      y = ((dx / l) * width) / 2;
    emit(
      [
        [a[0] + x, a[1] + y],
        [b[0] + x, b[1] + y],
        [b[0] - x, b[1] - y],
        [a[0] - x, a[1] - y],
      ],
      c,
    );
  }
  for (const tag of tags) {
    const raw = tag[1].trim();
    if (raw === '/svg') {
      closed = true;
      continue;
    }
    if (closed) throw Error('SVG: лишние элементы после корня');
    const name = raw.match(/^[a-zA-Z]+/)?.[0];
    if (!ALLOWED_TAGS.has(name))
      throw Error(
        `SVG ${name ?? raw}: пока поддерживаются rect, circle, ellipse, line, polyline, polygon, path`,
      );
    const attrs = {};
    let tail = raw.slice(name.length).replace(/\/$/, '');
    let pos = 0;
    while (pos < tail.length) {
      if (!tail.slice(pos).trim()) break;
      ATTR_RE.lastIndex = pos;
      const m = ATTR_RE.exec(tail);
      if (!m) throw Error('SVG: некорректные атрибуты');
      if (Object.hasOwn(attrs, m[1])) throw Error('SVG: повторный атрибут');
      attrs[m[1]] = m[3];
      pos = ATTR_RE.lastIndex;
    }
    const allowed = ALLOWED_ATTRS[name];
    for (const k of Object.keys(attrs))
      if (!allowed.has(k)) throw Error(`SVG: атрибут ${k} пока не поддерживается`);
    if (name === 'svg') {
      if (root) throw Error('SVG: один корень');
      root = true;
      view = attrs.viewBox
        ?.trim()
        .split(/[\s,]+/)
        .map(v => number(v)) ?? [0, 0, number(attrs.width), number(attrs.height)];
      if (view.length !== 4 || view[2] <= 0 || view[3] <= 0)
        throw Error('SVG требует положительный viewBox');
      continue;
    }
    if (!root || !raw.endsWith('/'))
      throw Error('SVG: примитивы внутри svg должны быть самозакрывающимися');
    const fill = ink(attrs.fill ?? '#000000'),
      stroke = ink(attrs.stroke ?? 'none'),
      sw = number(attrs['stroke-width'], 1);
    if (sw < 0) throw Error('SVG: отрицательная толщина');
    if (name === 'path') {
      const subpaths = flattenSvgPath(attrs.d, Math.max(1e-6, Math.max(view[2], view[3]) / 2048));
      if (fill) {
        if (overlappingContours(subpaths))
          throw Error(
            'SVG path: вложенные и пересекающиеся контуры (отверстия) пока не поддерживаются',
          );
        for (const sub of subpaths) if (sub.points.length > 2) emit(sub.points, fill);
      }
      if (stroke && sw)
        for (const { points: line, closed } of subpaths)
          for (let i = 0; i < line.length - (closed ? 0 : 1); i++)
            segment(line[i], line[(i + 1) % line.length], sw, stroke);
      continue;
    }
    let points = [],
      close = true;
    if (name === 'rect') {
      const x = number(attrs.x),
        y = number(attrs.y),
        w = number(attrs.width),
        h = number(attrs.height);
      if (w < 0 || h < 0) throw Error('SVG: отрицательный размер');
      points = [
        [x, y],
        [x + w, y],
        [x + w, y + h],
        [x, y + h],
      ];
    } else if (name === 'circle' || name === 'ellipse') {
      const x = number(attrs.cx),
        y = number(attrs.cy),
        rx = number(attrs.rx ?? attrs.r),
        ry = number(attrs.ry ?? attrs.r);
      if (rx < 0 || ry < 0) throw Error('SVG: отрицательный радиус');
      points = UNIT_CIRCLE.map(([cx, sy]) => [x + rx * cx, y + ry * sy]);
    } else if (name === 'line') {
      points = [
        [number(attrs.x1), number(attrs.y1)],
        [number(attrs.x2), number(attrs.y2)],
      ];
      close = false;
    } else {
      const coords =
        attrs.points
          ?.trim()
          .split(/[\s,]+/)
          .map(v => number(v)) ?? [];
      if (coords.length < 4 || coords.length % 2 || coords.length > 2048)
        throw Error('SVG: неверный points');
      points = Array.from({ length: coords.length / 2 }, (_, i) => coords.slice(i * 2, i * 2 + 2));
      close = name === 'polygon';
    }
    if (name !== 'line') emit(points, fill);
    if (stroke && sw)
      for (let i = 0; i < points.length - (close ? 0 : 1); i++)
        segment(points[i], points[(i + 1) % points.length], sw, stroke);
  }
  if (!root || !closed) throw Error('SVG: незакрытый корень');
  if (out.length > 2048) throw Error('SVG: слишком много примитивов');
  return { view, shapes: out };
}

// The mapping keeps the original single-pass arithmetic: stroke offsets are
// computed in viewBox units and only then scaled and translated.
// Placed geometry crosses the wire as text, so it is quantized to 1/1000 px.
// Full double precision triples the template size and is far below one device pixel.
const QUANTUM = 1e3;
export function placeSvgShapes({ view, shapes }, box) {
  const [vx, vy, vw, vh] = view,
    [x, y, w, h] = box,
    s = Math.min(w / vw, h / vh),
    ox = x + (w - vw * s) / 2,
    oy = y + (h - vh * s) / 2;
  return shapes.map(({ color, points }) => ({
    color,
    points: points.map(([a, b]) => [
      Math.round((ox + (a - vx) * s) * QUANTUM) / QUANTUM,
      Math.round((oy + (b - vy) * s) * QUANTUM) / QUANTUM,
    ]),
  }));
}

export function svgShapes(source, box, currentColor = '#ffffff') {
  return placeSvgShapes(parseSvgShapes(source, currentColor), box);
}
