import { readFileSync, writeFileSync, realpathSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { resolve } from 'node:path';

export const root = fileURLToPath(new URL('../', import.meta.url));
// An entry point reached through a symlink (/var -> /private/var in a macOS temp
// directory) has two absolute spellings for one file, so the CLI guard compares
// canonical paths. Otherwise `check` would print nothing and still exit 0.
const canonical = path => {
  try {
    return realpathSync(path);
  } catch {
    return path;
  }
};
const read = path => readFileSync(resolve(root, path), 'utf8');
export function validVersion(value) {
  if (typeof value !== 'string') return false;
  const match =
    /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?$/.exec(
      value,
    );
  return (
    !!match &&
    (!match[4] ||
      match[4].split('.').every(id => !/^\d+$/.test(id) || id === '0' || !id.startsWith('0')))
  );
}
function cargoVersion(text, name) {
  const section = text
    .split(/(?=^\[\[?[^\n]+\]\]?\s*$)/m)
    .find(s => new RegExp(`^name = "${name}"$`, 'm').test(s));
  const version = section?.match(/^version = "([^"]+)"$/m)?.[1];
  if (!version) throw Error(`Missing version for ${name}`);
  return version;
}
const SVET_PACKAGE = 'packages/svet/package.json',
  SVET_LOCK = 'packages/svet/package-lock.json',
  SVET_WASM = 'packages/svet/crates/svet-wasm/Cargo.toml',
  SVET_WASM_LOCK = 'packages/svet/crates/svet-wasm/Cargo.lock',
  SVET_WORKSPACE_CRATES = [
    ['svet-graph', 'crates/svet-graph/Cargo.toml'],
    ['svet-reconcile', 'crates/svet-reconcile/Cargo.toml'],
    ['svet-gpu-arena', 'crates/svet-gpu-arena/Cargo.toml'],
  ];
export function versions() {
  const studio = JSON.parse(read('package.json')).version;
  const lock = JSON.parse(read('package-lock.json'));
  const runtime = cargoVersion(read('crates/forma/Cargo.toml'), 'forma');
  if (!validVersion(runtime) || !validVersion(studio)) throw Error('Invalid component version');
  // One workspace lockfile serves forma and both example apps.
  if (cargoVersion(read('Cargo.lock'), 'forma') !== runtime)
    throw Error('Runtime Cargo.lock version mismatch');
  if (lock.version !== studio || lock.packages[''].version !== studio)
    throw Error('Studio package-lock.json version mismatch');
  for (const path of ['native-app/Cargo.toml', 'native-app/Cargo.lock'])
    if (cargoVersion(read(path), 'forma-native') !== studio)
      throw Error(`Studio host version mismatch: ${path}`);
  // The svet engine ships in the same release: its npm package and its Rust core carry the
  // release version, so a version number never stands still across commits.
  const svet = JSON.parse(read(SVET_PACKAGE)).version;
  if (!validVersion(svet)) throw Error('Invalid svet version');
  const svetLock = JSON.parse(read(SVET_LOCK));
  if (svetLock.version !== svet || svetLock.packages[''].version !== svet)
    throw Error('svet package-lock.json version mismatch');
  for (const path of [SVET_WASM, SVET_WASM_LOCK])
    if (cargoVersion(read(path), 'svet-wasm') !== svet)
      throw Error(`svet-wasm version mismatch: ${path}`);
  for (const [name, manifest] of SVET_WORKSPACE_CRATES)
    for (const path of [manifest, 'Cargo.lock', SVET_WASM_LOCK])
      if (cargoVersion(read(path), name) !== svet) throw Error(`${name} version mismatch: ${path}`);
  const wasmBindgen = read('crates/forma/Cargo.toml').match(/^wasm-bindgen = "=([^"]+)"$/m)?.[1];
  if (!wasmBindgen || cargoVersion(read('Cargo.lock'), 'wasm-bindgen') !== wasmBindgen)
    throw Error('wasm-bindgen must be exactly pinned and match Cargo.lock');
  return { runtime, studio, svet, wasmBindgen };
}
// One tag releases the library, Studio and svet together, so every manifest carries its version.
export function release(tag, current = versions()) {
  const match = /^v(.+)$/.exec(tag ?? '');
  if (!match || !validVersion(match[1])) throw Error('Expected vX.Y.Z (optional prerelease)');
  const version = match[1];
  for (const component of ['runtime', 'studio', 'svet'])
    if (current[component] !== version)
      throw Error(`Tag ${tag} does not match ${component} manifest version ${current[component]}`);
  return { version, tag, prerelease: version.includes('-') };
}
function bumpJson(text, version) {
  const value = JSON.parse(text);
  value.version = version;
  if (value.packages) value.packages[''].version = version;
  return JSON.stringify(value, null, 2) + '\n';
}
function replaceCargo(text, name, version) {
  let found = false;
  const next = text
    .split(/(?=^\[\[?[^\n]+\]\]?\s*$)/m)
    .map(section => {
      if (!new RegExp(`^name = "${name}"$`, 'm').test(section)) return section;
      found = true;
      return section.replace(/^version = "[^"]+"$/m, `version = "${version}"`);
    })
    .join('');
  if (!found) throw Error(`Missing package ${name}`);
  return next;
}
function replaceCargoDepVersion(text, name, version) {
  return text.replace(
    new RegExp(`(^${name}\\s*=\\s*\\{[^\\n]*?\\bversion\\s*=\\s*")[^"]+(")`, 'gm'),
    `$1${version}$2`,
  );
}
export function setVersion(component, version) {
  versions(); // Validate all inputs before preparing any writes.
  if (!['runtime', 'studio', 'svet', 'all'].includes(component) || !validVersion(version))
    throw Error('set all|runtime|studio|svet X.Y.Z[-prerelease]');
  // A release needs every manifest on one version; `all` moves them together.
  if (component === 'all') {
    setVersion('runtime', version);
    setVersion('studio', version);
    return setVersion('svet', version);
  }
  const updates = new Map();
  const current = path => updates.get(path) ?? read(path);
  if (component === 'svet') {
    for (const path of [SVET_PACKAGE, SVET_LOCK]) updates.set(path, bumpJson(read(path), version));
    for (const path of [SVET_WASM, SVET_WASM_LOCK])
      updates.set(path, replaceCargo(current(path), 'svet-wasm', version));
    for (const [name, manifest] of SVET_WORKSPACE_CRATES) {
      for (const path of [manifest, 'Cargo.lock', SVET_WASM_LOCK])
        updates.set(path, replaceCargo(current(path), name, version));
      for (const path of [SVET_WASM, 'crates/forma/Cargo.toml'])
        updates.set(path, replaceCargoDepVersion(current(path), name, version));
    }
    for (const [path, text] of updates) writeFileSync(resolve(root, path), text);
    return versions();
  }
  // forma is a workspace member, so its lock entry lives in the root Cargo.lock;
  // native-app stays outside the workspace and keeps its own.
  const name = component === 'runtime' ? 'forma' : 'forma-native',
    paths =
      component === 'runtime'
        ? ['crates/forma/Cargo.toml', 'Cargo.lock']
        : ['native-app/Cargo.toml', 'native-app/Cargo.lock'];
  for (const path of paths) updates.set(path, replaceCargo(read(path), name, version));
  if (component === 'studio') {
    for (const path of ['package.json', 'package-lock.json'])
      updates.set(path, bumpJson(read(path), version));
  }
  for (const [path, text] of updates) writeFileSync(resolve(root, path), text);
  return versions();
}
if (
  process.argv[1] &&
  canonical(resolve(process.argv[1])) === canonical(fileURLToPath(import.meta.url))
) {
  const [command = 'check', a, b] = process.argv.slice(2);
  try {
    if (command === 'check') console.log(JSON.stringify(a ? release(a) : versions()));
    else if (command === 'wasm-bindgen') console.log(versions().wasmBindgen);
    else if (command === 'set') console.log(JSON.stringify(setVersion(a, b)));
    else
      throw Error(
        'Usage: versions.mjs check [tag] | wasm-bindgen | set all|runtime|studio|svet version',
      );
  } catch (error) {
    console.error(error.message);
    process.exitCode = 1;
  }
}
