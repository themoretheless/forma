// Writes only existing model fields. Repeated elements retain their actual item
// object in their lexical environment, so a sort cannot redirect an input edit.
export function writePreviewBinding(state, node, path, value, {createMissing = false} = {}) {
  const [root, ...parts] = path.split('.');
  let target = root === 'state' ? state : node.environment?.locals?.[root];
  if (!target || !parts.length) throw Error(`Недоступная привязка ${path}`);
  const field = parts.pop();
  for (const part of parts) {
    if (!Object.hasOwn(target, part) || target[part] == null) throw Error(`Нет значения ${path}`);
    target = target[part];
  }
  if (!Object.hasOwn(target, field) && !createMissing) throw Error(`Нет значения ${path}`);
  if (Object.is(target[field], value)) return false;
  target[field] = value;
  return true;
}

function resolveTarget(state, path) {
  const parts = String(path ?? '').replace(/^state\./, '').split('.').filter(Boolean);
  if (!parts.length) return null;
  const field = parts.pop();
  let target = state;
  for (const part of parts) {
    if (target[part] == null || typeof target[part] !== 'object') target[part] = {};
    target = target[part];
  }
  return { target, field, fullPath: ['state', ...parts, field].join('.') };
}

export function executePreviewStateAction(state, action, args = [], stateDefaults = {}) {
  const norm = action.startsWith('actions.') ? 'state.' + action.slice(8) : action;
  if (!norm.startsWith('state.')) return { handled: false };
  const cmd = norm.slice(6);
  if (cmd === 'reset') {
    for (const k of Object.keys(state)) delete state[k];
    Object.assign(state, structuredClone(stateDefaults));
    return { handled: true, log: 'state сброшен к значениям по умолчанию' };
  }
  if (['toggle', 'set', 'increment', 'decrement', 'push', 'remove', 'clear'].includes(cmd) && typeof args[0] === 'string') {
    const loc = resolveTarget(state, args[0]);
    if (!loc) return { handled: false };
    const { target, field, fullPath } = loc;
    if (cmd === 'toggle') target[field] = !target[field];
    else if (cmd === 'set') target[field] = args[1];
    else if (cmd === 'increment') target[field] = (Number(target[field]) || 0) + (args[1] !== undefined ? Number(args[1]) : 1);
    else if (cmd === 'decrement') target[field] = (Number(target[field]) || 0) - (args[1] !== undefined ? Number(args[1]) : 1);
    else if (cmd === 'push') {
      const list = Array.isArray(target[field]) ? [...target[field]] : [];
      list.push(args[1]);
      target[field] = list;
    } else if (cmd === 'remove') {
      const list = Array.isArray(target[field]) ? [...target[field]] : [];
      const item = args[1];
      target[field] = typeof item === 'number' && item >= 0 && item < list.length ? list.filter((_, i) => i !== item) : list.filter(x => x !== item);
    } else if (cmd === 'clear') {
      target[field] = Array.isArray(target[field]) ? [] : typeof target[field] === 'object' && target[field] !== null ? {} : '';
    }
    return { handled: true, log: `${fullPath} = ${JSON.stringify(target[field])}` };
  }
  if (cmd.length > 0) {
    const loc = resolveTarget(state, cmd);
    if (!loc) return { handled: false };
    const { target, field, fullPath } = loc;
    target[field] = args.length ? args[0] : !target[field];
    return { handled: true, log: `${fullPath} = ${JSON.stringify(target[field])}` };
  }
  return { handled: false };
}

const KEY_ALIASES = {
  esc: 'escape',
  return: 'enter',
  space: ' ',
  spacebar: ' ',
  del: 'delete',
  ins: 'insert',
  up: 'arrowup',
  down: 'arrowdown',
  left: 'arrowleft',
  right: 'arrowright',
  pgup: 'pageup',
  pgdn: 'pagedown',
};

export function matchPreviewShortcut(event, combo) {
  const parts = String(combo ?? '').split('+').map(p => p.trim().toLowerCase()).filter(Boolean);
  if (!parts.length) return false;
  const rawKey = parts.at(-1);
  const keyPart = KEY_ALIASES[rawKey] ?? rawKey;
  const mods = new Set(parts.slice(0, -1));
  const wantCmdOrCtrl = mods.has('cmdorctrl') || mods.has('mod');
  const wantCtrl = mods.has('ctrl') || mods.has('control');
  const wantMeta = mods.has('cmd') || mods.has('meta') || mods.has('command');
  const wantShift = mods.has('shift');
  const wantAlt = mods.has('alt') || mods.has('option') || mods.has('opt');
  if (wantCmdOrCtrl && !(event.metaKey || event.ctrlKey)) return false;
  if (!wantCmdOrCtrl && wantCtrl !== !!event.ctrlKey) return false;
  if (!wantCmdOrCtrl && wantMeta !== !!event.metaKey) return false;
  if (wantShift !== !!event.shiftKey) return false;
  if (wantAlt !== !!event.altKey) return false;
  const actualKey = (event.key || '').toLowerCase();
  return actualKey === keyPart || (KEY_ALIASES[actualKey] ?? actualKey) === keyPart;
}


