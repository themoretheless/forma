// @ts-check
import { execFile } from 'node:child_process';
import { existsSync } from 'node:fs';
import { mkdtemp, open, realpath, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { promisify } from 'node:util';

const exec = promisify(execFile);

const FORBIDDEN_KEYWORDS = new Set(['unsafe', 'extern', 'async', 'macro_rules']);
const ALLOWED_OUTER_ATTRS = new Set(['derive', 'allow']);
const FORBIDDEN_MODULES = new Set([
  'process',
  'fs',
  'net',
  'env',
  'os',
  'ffi',
  'ptr',
  'mem',
  'intrinsics',
  'arch',
  'thread',
  'unix',
  'windows',
  'Command',
]);
const SAFE_PURE_MACROS = new Set([
  'format',
  'vec',
  'concat',
  'stringify',
  'matches',
  'cfg',
  'panic',
  'unreachable',
  'todo',
  'unimplemented',
  'assert',
  'assert_eq',
  'assert_ne',
  'write',
  'writeln',
]);

const RE_RAW_STRING_START = /(?:[bc])?r(#*)"/uy;
const RE_STRING_PREFIX = /(?:[bc])?"/uy;
const RE_BYTE_CHAR = /b'(?:\\(?:x[0-9a-fA-F]{2}|u\{[0-9a-fA-F_]+\}|[^'\n\r])|[^'\\\n\r])'/uy;
const RE_CHAR_LITERAL = /'(?:\\(?:x[0-9a-fA-F]{2}|u\{[0-9a-fA-F_]+\}|[^'\n\r])|[^'\\\n\r])'/uy;
const RE_LIFETIME = /'[A-Za-z_\p{ID_Start}][A-Za-z0-9_\p{ID_Continue}]*/uy;
const RE_RAW_IDENT = /r#([A-Za-z_\p{ID_Start}][A-Za-z0-9_\p{ID_Continue}]*)/uy;
const RE_IDENT = /[A-Za-z_\p{ID_Start}][A-Za-z0-9_\p{ID_Continue}]*/uy;

function matchAt(pattern, source, index) {
  pattern.lastIndex = index;
  return pattern.exec(source);
}

function rejectDesignConstruct(detail) {
  throw Error(`src/design.rs содержит запрещённые конструкции (${detail})`);
}

function tokenizeRustSource(source) {
  /** @type {Array<{ kind: string, value: string }>} */
  const tokens = [];
  let i = 0;
  while (i < source.length) {
    const ch = source[i];
    if (/\s/.test(ch)) {
      i++;
      continue;
    }
    if (ch === '/' && source[i + 1] === '/') {
      const nl = source.indexOf('\n', i + 2);
      i = nl < 0 ? source.length : nl + 1;
      continue;
    }
    if (ch === '/' && source[i + 1] === '*') {
      let depth = 1;
      i += 2;
      while (i < source.length && depth > 0) {
        if (source[i] === '/' && source[i + 1] === '*') {
          depth++;
          i += 2;
        } else if (source[i] === '*' && source[i + 1] === '/') {
          depth--;
          i += 2;
        } else {
          i++;
        }
      }
      if (depth > 0) rejectDesignConstruct('незакрытый комментарий');
      continue;
    }

    const rawStr = matchAt(RE_RAW_STRING_START, source, i);
    if (rawStr) {
      const closing = '"' + '#'.repeat(rawStr[1].length);
      const end = source.indexOf(closing, i + rawStr[0].length);
      if (end < 0) rejectDesignConstruct('незакрытая строка');
      i = end + closing.length;
      tokens.push({ kind: 'str', value: '' });
      continue;
    }

    const strStart = matchAt(RE_STRING_PREFIX, source, i);
    if (strStart) {
      i += strStart[0].length;
      let closed = false;
      while (i < source.length) {
        if (source[i] === '\\') {
          i += 2;
        } else if (source[i] === '"') {
          i++;
          closed = true;
          break;
        } else {
          i++;
        }
      }
      if (!closed) rejectDesignConstruct('незакрытая строка');
      tokens.push({ kind: 'str', value: '' });
      continue;
    }

    if (ch === 'b' && source[i + 1] === "'") {
      const byteChar = matchAt(RE_BYTE_CHAR, source, i);
      if (byteChar) {
        i += byteChar[0].length;
        tokens.push({ kind: 'char', value: '' });
        continue;
      }
    }

    if (ch === "'") {
      const charLit = matchAt(RE_CHAR_LITERAL, source, i);
      if (charLit) {
        i += charLit[0].length;
        tokens.push({ kind: 'char', value: '' });
        continue;
      }
      const lifetime = matchAt(RE_LIFETIME, source, i);
      if (lifetime) {
        i += lifetime[0].length;
        tokens.push({ kind: 'lifetime', value: lifetime[0] });
        continue;
      }
      i++;
      tokens.push({ kind: 'punct', value: "'" });
      continue;
    }

    const rawIdent = matchAt(RE_RAW_IDENT, source, i);
    if (rawIdent) {
      i += rawIdent[0].length;
      tokens.push({ kind: 'ident', value: rawIdent[1] });
      continue;
    }

    const ident = matchAt(RE_IDENT, source, i);
    if (ident) {
      i += ident[0].length;
      tokens.push({ kind: 'ident', value: ident[0] });
      continue;
    }

    if (ch === ':' && source[i + 1] === ':') {
      tokens.push({ kind: 'punct', value: '::' });
      i += 2;
      continue;
    }
    if (ch === '!' && source[i + 1] === '=') {
      tokens.push({ kind: 'punct', value: '!=' });
      i += 2;
      continue;
    }
    tokens.push({ kind: 'punct', value: ch });
    i++;
  }
  return tokens;
}

/** Validates user design-data Rust source with a Rust lexical scanner before compilation. */
export function validateDesignRustSource(source) {
  if (typeof source !== 'string') throw Error('Добавьте src/design.rs с pub const или pub fn');
  if (source.length > 256 * 1024) throw Error('src/design.rs превышает лимит 256 КиБ');
  const tokens = tokenizeRustSource(source);

  for (let i = 0; i < tokens.length; i++) {
    const token = tokens[i];
    if (token.kind === 'ident') {
      if (FORBIDDEN_KEYWORDS.has(token.value)) rejectDesignConstruct(token.value);
      if (
        FORBIDDEN_MODULES.has(token.value) &&
        (tokens[i - 1]?.value === '::' || tokens[i + 1]?.value === '::')
      ) {
        rejectDesignConstruct(token.value);
      }
      if (tokens[i + 1]?.kind === 'punct' && tokens[i + 1].value === '!') {
        if (tokens[i - 1]?.value === '::' || !SAFE_PURE_MACROS.has(token.value)) {
          rejectDesignConstruct(`${token.value}!`);
        }
      }
      if (token.value === 'use' && tokens[i - 1]?.value !== '::') {
        let braceDepth = 0;
        for (let j = i + 1; j < tokens.length; j++) {
          const inner = tokens[j];
          if (inner.kind === 'punct' && inner.value === '{') braceDepth++;
          else if (inner.kind === 'punct' && inner.value === '}') braceDepth--;
          else if (inner.kind === 'punct' && inner.value === ';' && braceDepth <= 0) break;
          else if (inner.kind === 'ident' && FORBIDDEN_MODULES.has(inner.value)) {
            rejectDesignConstruct(inner.value);
          }
        }
      }
    } else if (token.kind === 'punct' && token.value === '#') {
      if (tokens[i + 1]?.value === '!' && tokens[i + 2]?.value === '[') {
        rejectDesignConstruct('#![...]');
      }
      if (tokens[i + 1]?.value === '[') {
        const attrName = tokens[i + 2];
        const afterName = tokens[i + 3];
        if (
          attrName?.kind !== 'ident' ||
          !ALLOWED_OUTER_ATTRS.has(attrName.value) ||
          (afterName?.value !== '(' && afterName?.value !== ']')
        ) {
          rejectDesignConstruct(`#[${attrName?.value ?? ''}]`);
        }
        let depth = 1;
        let closed = false;
        for (let j = i + 2; j < tokens.length; j++) {
          if (tokens[j].value === '[') depth++;
          else if (tokens[j].value === ']') {
            depth--;
            if (depth === 0) {
              closed = true;
              break;
            }
          }
        }
        if (!closed) rejectDesignConstruct('незакрытый атрибут');
      }
    }
  }
}

function escapeSandboxString(value) {
  return value.replace(/\\/g, '\\\\').replace(/"/g, '\\"');
}

/** Runs a compiled design-data helper binary with stripped environment and OS write/network confinement. */
export async function runSandboxedDesignBinary(binPath, dir, options = {}) {
  const realDir = await realpath(dir);
  const env = {
    HOME: realDir,
    TMPDIR: realDir,
    TMP: realDir,
    TEMP: realDir,
    PATH: '',
  };
  const execOpts = {
    cwd: realDir,
    env,
    timeout: options.timeout ?? 60000,
    maxBuffer: options.maxBuffer ?? 1024 * 1024,
  };
  if (options.sandbox !== false) {
    if (process.platform === 'darwin' && existsSync('/usr/bin/sandbox-exec')) {
      const profile = `(version 1) (allow default) (deny network*) (deny process-fork) (deny file-write*) (allow file-write* (subpath "${escapeSandboxString(realDir)}") (literal "/dev/null"))`;
      return exec('/usr/bin/sandbox-exec', ['-p', profile, binPath], execOpts);
    }
    const bwrap = existsSync('/usr/bin/bwrap')
      ? '/usr/bin/bwrap'
      : existsSync('/bin/bwrap')
        ? '/bin/bwrap'
        : null;
    if (process.platform === 'linux' && bwrap) {
      return exec(
        bwrap,
        [
          '--unshare-net',
          '--ro-bind',
          '/usr',
          '/usr',
          '--ro-bind',
          '/lib',
          '/lib',
          ...(existsSync('/lib64') ? ['--ro-bind', '/lib64', '/lib64'] : []),
          '--proc',
          '/proc',
          '--dev',
          '/dev',
          '--bind',
          realDir,
          realDir,
          '--chdir',
          realDir,
          binPath,
        ],
        execOpts,
      );
    }
  }
  return exec(binPath, [], execOpts);
}

export async function evaluateDesignData(
  files,
  refs,
  { tempRoot = tmpdir(), sandbox = true } = {},
) {
  if (
    !Array.isArray(refs) ||
    refs.length > 100 ||
    refs.some(r => !/^design\.[A-Za-z_][A-Za-z0-9_]*(\(\))?$/.test(r))
  )
    throw Error('Поддерживаются design.NAME и design.method() без аргументов');
  const designSource = files?.['src/design.rs'];
  validateDesignRustSource(designSource);
  const dir = await mkdtemp(path.join(tempRoot, 'forma-design-'));
  try {
    await writeFile(path.join(dir, 'design.rs'), designSource);
    const source =
      'mod design; fn main() {\n' +
      refs
        .map(
          (r, i) =>
            `std::fs::write("value-${i}", format!("{}", design::${r.slice('design.'.length)})).unwrap();`,
        )
        .join('\n') +
      '\n}';
    await writeFile(path.join(dir, 'main.rs'), source);
    const executable = process.platform === 'win32' ? 'design-data.exe' : 'design-data';
    const compileEnv = { ...process.env, TMPDIR: dir, TMP: dir, TEMP: dir };
    await exec('rustc', ['--edition=2021', 'main.rs', '-o', executable], {
      cwd: dir,
      env: compileEnv,
      timeout: 60000,
      maxBuffer: 1024 * 1024,
    });
    const binPath = await realpath(path.join(dir, executable));
    await runSandboxedDesignBinary(binPath, dir, { sandbox });
    return await readDesignValues(dir, refs);
    // Windows can keep the just-exited executable locked briefly (EBUSY/EPERM); rm retries those.
  } finally {
    await rm(dir, { recursive: true, force: true, maxRetries: 10, retryDelay: 100 });
  }
}

// One bounded scratch buffer; never read arbitrary-size result files in parallel.
export async function readDesignValues(dir, refs) {
  const limit = 1024 * 1024,
    buffer = Buffer.allocUnsafe(limit + 1),
    values = {};
  let total = 0;
  for (let i = 0; i < refs.length; i++) {
    const file = await open(path.join(dir, `value-${i}`), 'r');
    try {
      if (!(await file.stat()).isFile()) throw Error('Результат design должен быть обычным файлом');
      let used = 0;
      while (used <= limit - total) {
        const { bytesRead } = await file.read(buffer, used, limit - total + 1 - used, null);
        if (!bytesRead) break;
        used += bytesRead;
      }
      if (used > limit - total) throw Error('Результаты design превышают 1 МиБ');
      total += used;
      values[refs[i]] = buffer.toString('utf8', 0, used);
    } finally {
      await file.close();
    }
  }
  return values;
}
