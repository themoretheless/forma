// Primitive template literal escaping, serialization, and byte-length framing.
const templateEncoder = new TextEncoder();

// Templates are framed by their UTF-8 byte length. Encoding a ~7 KB icon template
// only to read that length allocated the bytes again for every instance, and
// generated templates are ASCII, where the JS length already is the byte length.
const asciiTemplate = /^[\0-\x7f]*$/;

export const templateByteLength = text =>
  asciiTemplate.test(text) ? text.length : templateEncoder.encode(text).length;

export function frameTemplates(controls) {
  return controls.length === 1
    ? controls[0].template
    : 'FORMA-TEMPLATES-1\n' +
        controls.map(c => `${templateByteLength(c.template)}\n${c.template}`).join('');
}

export function literal(v) {
  if (v?.expr) return v.expr;
  if (v?.type)
    return `${v.type} { ${Object.entries(v.props)
      .map(([k, x]) => `${k}: ${literal(x)};`)
      .join(' ')} ${(v.children ?? []).map(serialize).join(' ')} }`;
  if (Array.isArray(v)) return v.map(literal).join(' ');
  if (typeof v === 'string')
    return `'${v
      .replace(/\\/g, '\\\\')
      .replace(/'/g, "\\'")
      .replace(/\n/g, '\\n')
      .replace(/\r/g, '\\r')
      .replace(/\t/g, '\\t')
      .replace(/\$\{/g, '\\${')}'`;
  if (typeof v === 'number' && !Number.isFinite(v)) throw Error('Нечисловой размер');
  if (typeof v === 'number' || typeof v === 'boolean') return String(v);
  throw Error('Значение не удалось вычислить');
}

export function serialize(n) {
  return serializeNode(n, false);
}

export function serializeNode(n, omitKeys) {
  if (Object.keys(n.bindings ?? {}).length || Object.keys(n.slots ?? {}).length)
    throw Error(`Необработанная привязка или override в ${n.type}`);
  const keepTransition = n.type === 'Brush' || n.type === 'Reveal';
  return `${n.type} { ${Object.entries(n.props)
    .filter(([k]) => !(omitKeys && k === 'key') && (keepTransition || k !== 'transition'))
    .map(([k, v]) => `${k}: ${literal(v)};`)
    .join(' ')} ${Object.entries(n.events ?? {})
    .map(([k, v]) => `${k} -> ${v}();`)
    .join(' ')} ${(n.children ?? []).map(child => serializeNode(child, omitKeys)).join(' ')} }`;
}
