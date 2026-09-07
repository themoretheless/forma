// Layout policy for explicit containers. Legacy Frame remains compatible;
// Row/Column/Grid/Stack state their arrangement directly in the source.
export const containerTypes = new Set(['Frame', 'Row', 'Column', 'Grid', 'Stack']);
const raw = value => value?.expr ?? value;
const own = (object, key) => Object.hasOwn(object, key);
export const intrinsicLength = value => value === undefined || ['auto', 'content', '-'].includes(raw(value));

export function length(value, available, natural, label = 'размер') {
  const v = raw(value);
  let result;
  if (intrinsicLength(value)) result = natural;
  else if (typeof v === 'number') result = v;
  else if (typeof v === 'string' && /^\d+(?:\.\d+)?px$/.test(v)) result = Number(v.slice(0, -2));
  else if (typeof v === 'string' && /^\d+(?:\.\d+)?%$/.test(v)) result = available * Number(v.slice(0, -1)) / 100;
  else if (typeof v === 'string' && /^(?:\d+(?:\.\d+)?)?\*$/.test(v)) result = available;
  else throw Error(`${label}: неверный размер ${String(v)}`);
  if (!Number.isFinite(result) || result < 0 || result > 100000) throw Error(`${label}: размер вне диапазона 0…100000`);
  return result;
}

export function constrained(value, props, axis, available) {
  const suffix = axis === 0 ? 'Width' : 'Height';
  const min = props['min' + suffix] === undefined ? 0 : length(props['min' + suffix], available, 0, 'min' + suffix);
  const max = props['max' + suffix] === undefined ? Infinity : length(props['max' + suffix], available, 0, 'max' + suffix);
  if (min > max) throw Error(`min${suffix} превышает max${suffix}`);
  return Math.min(max, Math.max(min, value));
}

export function padding(value = 0) {
  const items = Array.isArray(value) ? value : [value];
  if (!items.length || items.length > 4) throw Error('padding: от 1 до 4 размеров');
  const a = items.map(item => length(item, 0, 0, 'padding'));
  return a.length === 1 ? [a[0], a[0], a[0], a[0]] : a.length === 2 ? [a[0], a[1], a[0], a[1]] : a.length === 3 ? [a[0], a[1], a[2], a[1]] : a;
}

export function gaps(value = 0) {
  const items = Array.isArray(value) ? value : [value, value];
  if (items.length !== 2) throw Error('gap: один или два размера');
  return items.map(item => length(item, 0, 0, 'gap'));
}

function weight(value) {
  const v = raw(value);
  if (typeof v !== 'string' || !/^(?:\d+(?:\.\d+)?)?\*$/.test(v)) return 0;
  const result = v === '*' ? 1 : Number(v.slice(0, -1));
  if (!Number.isFinite(result) || result <= 0) throw Error('Вес * должен быть положительным конечным числом');
  return result;
}

export function gridPlacement(node) {
  const p = node.props;
  const columns = p.columns === undefined ? [{expr: '*'}] : Array.isArray(p.columns) ? p.columns : [p.columns];
  const rows = p.rows === undefined ? [] : Array.isArray(p.rows) ? [...p.rows] : [p.rows];
  if (!columns.length || (p.rows !== undefined && !rows.length)) throw Error('Список треков не должен быть пустым');
  const occupied = new Set();
  const placements = [];
  const positive = (value, name) => {
    if (!Number.isInteger(value) || value < 1 || value > 4096) throw Error(`${name}: ожидается целое число от 1 до 4096`);
    return value;
  };
  const requested = node.children.map(child => {
    const c = child.props;
    if (c.cell !== undefined && (!Array.isArray(c.cell) || c.cell.length !== 2 || own(c, 'row') || own(c, 'column'))) throw Error('cell: ожидаются row column без отдельных row/column');
    let row = c.cell?.[0] ?? c.row, column = c.cell?.[1] ?? c.column;
    if (row !== undefined) row = positive(row, 'row') - 1;
    if (column !== undefined) column = positive(column, 'column') - 1;
    const rs = positive(c['row.span'] ?? 1, 'row.span'), cs = positive(c['column.span'] ?? 1, 'column.span');
    if (cs > columns.length || column !== undefined && column + cs > columns.length) throw Error('Ячейка за пределами columns');
    if (row !== undefined && row + rs > 4096) throw Error('Grid: слишком много строк');
    return {row, column, rs, cs};
  });
  const occupy = (row, column, rs, cs) => {
    for (let y = row; y < row + rs; y++) for (let x = column; x < column + cs; x++) occupied.add(`${y}:${x}`);
  };
  // Explicit cells are reserved independently of source order. Automatically
  // placed siblings must not cover a later explicitly positioned child.
  for (const {row, column, rs, cs} of requested) if (row !== undefined && column !== undefined) occupy(row, column, rs, cs);
  for (const request of requested) {
    let {row, column, rs, cs} = request;
    const available = (r, col) => {
      for (let y = r; y < r + rs; y++) for (let x = col; x < col + cs; x++) if (occupied.has(`${y}:${x}`)) return false;
      return true;
    };
    if (row === undefined || column === undefined) {
      let found = false;
      for (let r = row ?? 0; r < (row === undefined ? 4096 - rs + 1 : row + 1) && !found; r++) {
        for (let col = column ?? 0; col < (column === undefined ? columns.length - cs + 1 : column + 1); col++) {
          if (available(r, col)) { row = r; column = col; found = true; break; }
        }
      }
      if (!found) throw Error('Grid: нет свободной ячейки');
    }
    while (rows.length < row + rs) rows.push({expr: 'auto'});
    occupy(row, column, rs, cs);
    placements.push({row, column, rowSpan: rs, columnSpan: cs});
  }
  return {columns, rows: rows.length ? rows : [{expr: 'auto'}], placements};
}

// Intrinsic children contribute only to content-sized tracks. A spanning child
// contributes its missing size across those tracks after single-cell content;
// fixed tracks are never measured and are counted only once.
function measuredTracks(node, grid, axis, natural, available, spacing, intrinsicFlex = false) {
  const tracks = axis === 0 ? grid.columns : grid.rows;
  const measurable = tracks.map(track => intrinsicLength(track) || intrinsicFlex && weight(track) > 0);
  const values = tracks.map((track, index) => measurable[index] || weight(track) ? 0 : length(track, available, 0));
  const ranges = node.children.map((child, index) => {
    const place = grid.placements[index];
    return {child, start: axis === 0 ? place.column : place.row, span: axis === 0 ? place.columnSpan : place.rowSpan};
  }).sort((a, b) => a.span - b.span);
  for (const {child, start, span} of ranges) {
    const flexible = [];
    for (let index = start; index < start + span; index++) if (measurable[index]) flexible.push(index);
    if (!flexible.length) continue;
    const required = natural(child, axis);
    const occupied = values.slice(start, start + span).reduce((sum, value) => sum + value, 0) + spacing * (span - 1);
    const addition = Math.max(0, required - occupied) / flexible.length;
    for (const index of flexible) values[index] += addition;
  }
  return values;
}

export function naturalContainer(node, axis, natural) {
  const p = node.props, pad = padding(p.padding), gap = gaps(p.gap), type = node.layoutKind ?? node.type;
  let size;
  if (type === 'Grid') {
    const grid = gridPlacement(node), tracks = axis === 0 ? grid.columns : grid.rows;
    size = measuredTracks(node, grid, axis, natural, 0, gap[axis === 0 ? 1 : 0], true).reduce((sum, value) => sum + value, 0) + gap[axis === 0 ? 1 : 0] * Math.max(0, tracks.length - 1);
  } else {
    const children = node.children.map(child => natural(child, axis));
    size = type === (axis === 0 ? 'Row' : 'Column') ? children.reduce((a, b) => a + b, 0) + gap[axis === 0 ? 1 : 0] * Math.max(0, children.length - 1) : Math.max(0, ...children);
  }
  return constrained(size + pad[axis === 0 ? 1 : 0] + pad[axis === 0 ? 3 : 2], p, axis, size);
}

export function arrange(node, box, natural, {scene = false} = {}) {
  const p = node.props, type = node.layoutKind ?? node.type, pad = padding(p.padding), gap = gaps(p.gap);
  const [left, top, width, height] = box;
  const inner = [left + pad[3], top + pad[0], Math.max(0, width - pad[1] - pad[3]), Math.max(0, height - pad[0] - pad[2])];
  const boxes = [];
  const childSize = (child, axis, available, stretch) => {
    const value = child.props[axis === 0 ? 'width' : 'height'];
    return constrained(value === undefined && stretch ? available : length(value, available, intrinsicLength(value) ? natural(child, axis) : 0), child.props, axis, available);
  };
  if (type === 'Row' || type === 'Column' || scene && type === 'Frame' && p.columns === undefined && p.rows === undefined) {
    const axis = type === 'Row' ? 0 : 1, available = inner[axis + 2], spacing = gap[axis === 0 ? 1 : 0];
    const sizes = node.children.map(child => weight(child.props[axis === 0 ? 'width' : 'height']) ? 0 : childSize(child, axis, available, false));
    let remaining = Math.max(0, available - spacing * Math.max(0, sizes.length - 1) - sizes.reduce((a, b) => a + b, 0));
    const weighted = node.children.map((child, index) => ({child, index, weight: weight(child.props[axis === 0 ? 'width' : 'height'])})).filter(item => item.weight);
    let unresolved = [...weighted];
    while (unresolved.length) {
      const scale = Math.max(...unresolved.map(item => item.weight));
      const total = unresolved.reduce((sum, item) => sum + item.weight / scale, 0);
      const candidates = unresolved.map(item => {
        const proposed = remaining * (item.weight / scale) / total;
        return {...item, proposed, actual: constrained(proposed, item.child.props, axis, available)};
      });
      const violation = candidates.reduce((sum, item) => sum + item.actual - item.proposed, 0);
      if (Math.abs(violation) < 1e-9) { for (const item of candidates) sizes[item.index] = item.actual; break; }
      // Freeze the bounds responsible for the total violation. Freezing a max
      // together with a min can force overflow even when a feasible fit exists.
      const fixed = candidates.filter(item => violation > 0 ? item.actual > item.proposed : item.actual < item.proposed);
      if (!fixed.length) { for (const item of candidates) sizes[item.index] = item.actual; break; }
      for (const item of fixed) { sizes[item.index] = item.actual; remaining = Math.max(0, remaining - item.actual); }
      const fixedIndexes = new Set(fixed.map(item => item.index));
      unresolved = unresolved.filter(item => !fixedIndexes.has(item.index));
    }
    let cursor = inner[axis];
    node.children.forEach((child, index) => {
      const cross = childSize(child, 1 - axis, inner[3 - axis], true);
      const childBox = axis === 0 ? [cursor, inner[1] + (inner[3] - cross) / 2, sizes[index], cross] : [inner[0] + (inner[2] - cross) / 2, cursor, cross, sizes[index]];
      if (scene) {
        if (child.props.x !== undefined) childBox[0] = left + length(child.props.x, width, 0, 'x');
        if (child.props.y !== undefined) childBox[1] = top + length(child.props.y, height, 0, 'y');
      }
      boxes.push(childBox); cursor += sizes[index] + spacing;
    });
    return {boxes, inner};
  }
  if (type === 'Grid') {
    const grid = gridPlacement(node);
    const compute = axis => {
      const tracks = axis === 0 ? grid.columns : grid.rows, spacing = gap[axis === 0 ? 1 : 0], available = inner[axis + 2];
      const values = measuredTracks(node, grid, axis, natural, available, spacing);
      const total = tracks.reduce((sum, track) => sum + weight(track), 0), remaining = Math.max(0, available - spacing * Math.max(0, tracks.length - 1) - values.reduce((a, b) => a + b, 0));
      return values.map((value, index) => weight(tracks[index]) ? remaining * weight(tracks[index]) / total : value);
    };
    const columns = compute(0), rows = compute(1);
    for (const [index, child] of node.children.entries()) {
      const place = grid.placements[index];
      const x = inner[0] + columns.slice(0, place.column).reduce((a, b) => a + b, 0) + place.column * gap[1];
      const y = inner[1] + rows.slice(0, place.row).reduce((a, b) => a + b, 0) + place.row * gap[0];
      const w = columns.slice(place.column, place.column + place.columnSpan).reduce((a, b) => a + b, 0) + (place.columnSpan - 1) * gap[1];
      const h = rows.slice(place.row, place.row + place.rowSpan).reduce((a, b) => a + b, 0) + (place.rowSpan - 1) * gap[0];
      const cw = childSize(child, 0, w, true), ch = childSize(child, 1, h, true);
      boxes.push([x + (w - cw) / 2, y + (h - ch) / 2, cw, ch]);
    }
    return {boxes, inner, grid: {bounds: inner, columns, rows, gap}};
  }
  for (const child of node.children) {
    const w = childSize(child, 0, inner[2], true), h = childSize(child, 1, inner[3], true);
    boxes.push([inner[0] + (inner[2] - w) / 2, inner[1] + (inner[3] - h) / 2, w, h]);
  }
  return {boxes, inner};
}
