import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import {
  copyFileSync,
  existsSync,
  mkdirSync,
  readFileSync,
  readlinkSync,
  renameSync,
  rmSync,
  symlinkSync,
  writeFileSync,
} from 'node:fs';
import { dirname, relative, resolve } from 'node:path';
import { root } from './versions.mjs';

// Rust takes tokenizer-forma from an exact revision: a path dependency's lockfile is not inherited
// by exported applications. The workspace Cargo.lock records the same pin. npm installs the JS
// adapters and language WASM from a verified vendored tarball matching that revision
// (package.json: file:vendor/themoretheless-tokenizer-forma-<version>.tgz), so clean checkouts
// do not need a sibling ../tokenizer repository.
export const tokenizerDir = resolve(root, process.env.FORMA_TOKENIZER_DIR ?? '../tokenizer');
const PACKAGE = resolve(tokenizerDir, 'packages/forma');
const SOURCES = ['crates/tokenizer-forma', 'packages/forma'];
export const VENDOR_MANIFEST = 'vendor/tokenizer-forma.json';

export function lockedTokenizer(lock = readFileSync(resolve(root, 'Cargo.lock'), 'utf8')) {
  const block = lock
    .split('[[package]]')
    .find(entry => /^name = "themoretheless-tokenizer-forma"$/m.test(entry));
  const source = block?.match(/^source = "git\+([^?"]+)\?rev=([0-9a-f]{40})#([0-9a-f]{40})"$/m);
  const version = block?.match(/^version = "([^"]+)"$/m)?.[1];
  if (!source || source[2] !== source[3] || !version)
    throw Error(
      'Cargo.lock must pin themoretheless-tokenizer-forma to an exact tokenizer revision',
    );
  return { url: source[1], rev: source[2], version };
}

function run(command, args, options = {}) {
  const result = spawnSync(command, args, { encoding: 'utf8', stdio: 'pipe', ...options });
  if (result.error || result.status !== 0)
    throw Error(
      `${command} ${args.join(' ')} failed: ${result.error?.message ?? (result.stderr?.trim() || result.status)}`,
    );
  return result.stdout?.trim() ?? '';
}

function sha256File(path) {
  return createHash('sha256').update(readFileSync(path)).digest('hex');
}

export function checkTokenizerCheckout(locked = lockedTokenizer(), dir = tokenizerDir) {
  if (!existsSync(resolve(dir, '.git')))
    throw Error(`No tokenizer checkout at ${dir}; run node scripts/tokenizer.mjs checkout`);
  if (spawnSync('git', ['cat-file', '-e', `${locked.rev}^{commit}`], { cwd: dir }).status !== 0)
    throw Error(
      `${dir} does not have the locked revision ${locked.rev.slice(0, 8)}; fetch it in that checkout`,
    );
  const changed = run(
    'git',
    ['status', '--porcelain', '--untracked-files=normal', '--', ...SOURCES],
    {
      cwd: dir,
    },
  );
  const differs =
    spawnSync('git', ['diff', '--quiet', locked.rev, '--', ...SOURCES], { cwd: dir }).status !== 0;
  if (changed || differs)
    throw Error(
      `${dir}: ${SOURCES.join(' and ')} differ from the locked revision ${locked.rev.slice(0, 8)}; check it out, or update rev in crates/forma/Cargo.toml and refresh the lockfiles`,
    );
  const version = JSON.parse(
    readFileSync(resolve(dir, 'packages/forma/package.json'), 'utf8'),
  ).version;
  if (version !== locked.version)
    throw Error(`packages/forma is ${version}, Cargo.lock pins ${locked.version}`);
  return locked;
}

export function checkTokenizer(locked = lockedTokenizer(), repoRoot = root) {
  const manifestPath = resolve(repoRoot, VENDOR_MANIFEST);
  if (!existsSync(manifestPath))
    throw Error(
      `Missing ${VENDOR_MANIFEST}; run node scripts/tokenizer.mjs sync from a matching tokenizer checkout`,
    );
  const manifest = JSON.parse(readFileSync(manifestPath, 'utf8'));
  const expectedTarball = `vendor/themoretheless-tokenizer-forma-${locked.version}.tgz`;
  if (manifest.name !== '@themoretheless/tokenizer-forma')
    throw Error(`${VENDOR_MANIFEST} has unexpected package name ${manifest.name}`);
  if (
    manifest.url !== locked.url ||
    manifest.rev !== locked.rev ||
    manifest.version !== locked.version
  )
    throw Error(
      `${VENDOR_MANIFEST} (${manifest.version} @ ${String(manifest.rev).slice(0, 8)}) differs from Cargo.lock (${locked.version} @ ${locked.rev.slice(0, 8)})`,
    );
  if (manifest.tarball !== expectedTarball)
    throw Error(`${VENDOR_MANIFEST} must reference ${expectedTarball}, found ${manifest.tarball}`);

  const tarballPath = resolve(repoRoot, manifest.tarball);
  if (!existsSync(tarballPath))
    throw Error(`Missing vendored tokenizer tarball ${manifest.tarball}`);
  const actualSha = sha256File(tarballPath);
  if (actualSha !== manifest.sha256)
    throw Error(
      `${manifest.tarball} sha256 mismatch: expected ${manifest.sha256}, got ${actualSha}`,
    );

  const expectedSpec = `file:${manifest.tarball}`;
  const pkg = JSON.parse(readFileSync(resolve(repoRoot, 'package.json'), 'utf8'));
  if (pkg.dependencies?.['@themoretheless/tokenizer-forma'] !== expectedSpec)
    throw Error(`package.json must depend on "@themoretheless/tokenizer-forma": "${expectedSpec}"`);

  const lock = JSON.parse(readFileSync(resolve(repoRoot, 'package-lock.json'), 'utf8'));
  if (lock.packages?.['']?.dependencies?.['@themoretheless/tokenizer-forma'] !== expectedSpec)
    throw Error(`package-lock.json root dependency must be "${expectedSpec}"`);
  const installedLock = lock.packages?.['node_modules/@themoretheless/tokenizer-forma'];
  if (
    !installedLock ||
    installedLock.resolved !== expectedSpec ||
    installedLock.version !== locked.version ||
    !installedLock.integrity
  )
    throw Error(
      'package-lock.json must lock node_modules/@themoretheless/tokenizer-forma from the vendored tarball with integrity',
    );

  const installedPkg = resolve(
    repoRoot,
    'node_modules/@themoretheless/tokenizer-forma/package.json',
  );
  if (existsSync(installedPkg)) {
    const installedVersion = JSON.parse(readFileSync(installedPkg, 'utf8')).version;
    if (installedVersion !== locked.version)
      throw Error(
        `Installed @themoretheless/tokenizer-forma is ${installedVersion}, expected ${locked.version}`,
      );
  }

  if (process.env.FORMA_TOKENIZER_DIR) {
    checkTokenizerCheckout(locked, resolve(repoRoot, process.env.FORMA_TOKENIZER_DIR));
  }

  return locked;
}

export function checkoutTokenizer(locked = lockedTokenizer()) {
  if (!existsSync(tokenizerDir)) {
    run(
      'git',
      ['clone', '--quiet', '--no-checkout', '--filter=blob:none', locked.url, tokenizerDir],
      { cwd: root },
    );
    run('git', ['checkout', '--quiet', '--detach', locked.rev], { cwd: tokenizerDir });
  }
  return checkTokenizerCheckout(locked);
}

// Kept for tests and explicit local override workflows; normal installs use the vendored tarball.
export const PACKAGE_LINK = resolve(root, 'node_modules/@themoretheless/tokenizer-forma');

export function linkTokenizerPackage(link = PACKAGE_LINK, target = PACKAGE) {
  let previous;
  try {
    previous = readlinkSync(link);
  } catch {
    return () => {};
  }
  if (resolve(dirname(link), previous) === resolve(target)) return () => {};
  const point = to => {
    const staged = `${link}.swap`;
    rmSync(staged, { force: true });
    symlinkSync(to, staged, 'junction');
    renameSync(staged, link);
  };
  point(process.platform === 'win32' ? resolve(target) : relative(dirname(link), target));
  let restored = false;
  return () => {
    if (restored) return;
    restored = true;
    point(previous);
  };
}

export function installTokenizer() {
  run(
    process.platform === 'win32' ? 'npm.cmd' : 'npm',
    ['ci', '--ignore-scripts', '--no-audit', '--no-fund'],
    { cwd: PACKAGE, shell: process.platform === 'win32' },
  );
}

// The tokenizer script reads its wasm from wasm/target, so a CARGO_TARGET_DIR set for forma must
// not leak into it, and it has no toolchain file of its own: build with the one forma pins.
export function buildTokenizerWasm() {
  const channel = readFileSync(resolve(root, 'rust-toolchain.toml'), 'utf8').match(
    /^channel = "([^"]+)"$/m,
  )?.[1];
  if (!channel) throw Error('rust-toolchain.toml has no channel');
  const env = { ...process.env, RUSTUP_TOOLCHAIN: channel };
  delete env.CARGO_TARGET_DIR;
  run(process.execPath, ['build-wasm.mjs'], { cwd: PACKAGE, env, stdio: 'inherit' });
}

export function syncTokenizer(locked = lockedTokenizer()) {
  checkoutTokenizer(locked);
  buildTokenizerWasm();
  const npm = process.platform === 'win32' ? 'npm.cmd' : 'npm';
  const packed = run(npm, ['pack', '--silent', '--ignore-scripts'], {
    cwd: PACKAGE,
    shell: process.platform === 'win32',
  });
  const tgzName = packed.split('\n').at(-1).trim();
  const tarballRel = `vendor/themoretheless-tokenizer-forma-${locked.version}.tgz`;
  mkdirSync(resolve(root, 'vendor'), { recursive: true });
  copyFileSync(resolve(PACKAGE, tgzName), resolve(root, tarballRel));
  rmSync(resolve(PACKAGE, tgzName), { force: true });
  const manifest = {
    name: '@themoretheless/tokenizer-forma',
    version: locked.version,
    url: locked.url,
    rev: locked.rev,
    tarball: tarballRel,
    sha256: sha256File(resolve(root, tarballRel)),
  };
  writeFileSync(resolve(root, VENDOR_MANIFEST), JSON.stringify(manifest, null, 2) + '\n');
  return manifest;
}

const invoked =
  process.argv[1] && import.meta.url === new URL(`file://${resolve(process.argv[1])}`).href;
if (invoked) {
  const commands = {
    check: () => checkTokenizer(),
    checkout: () => checkoutTokenizer(),
    install: () => installTokenizer(),
    build: () => buildTokenizerWasm(),
    sync: () => syncTokenizer(),
  };
  const names = process.argv.slice(2);
  if (!names.length || names.some(name => !commands[name])) {
    console.error(`usage: node scripts/tokenizer.mjs ${Object.keys(commands).join('|')} ...`);
    process.exit(2);
  }
  try {
    for (const name of names) commands[name]();
    const locked = lockedTokenizer();
    console.log(
      `tokenizer ${locked.version} @ ${locked.rev.slice(0, 12)} (pinned): ${names.join(', ')} ok`,
    );
  } catch (error) {
    console.error(error.message);
    process.exit(1);
  }
}
