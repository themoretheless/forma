import { test } from 'node:test';
import assert from 'node:assert/strict';
import { execFile } from 'node:child_process';
import { existsSync } from 'node:fs';
import { mkdtemp, readdir, readFile, realpath, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { promisify } from 'node:util';
import {
  evaluateDesignData,
  readDesignValues,
  runSandboxedDesignBinary,
  validateDesignRustSource,
} from '../design-data-runner.js';

const exec = promisify(execFile);

test('design evaluation removes temporary files after success and compiler/runtime failures', async () => {
  const parent = await mkdtemp(path.join(tmpdir(), 'forma-design-test-'));
  try {
    assert.deepEqual(
      await evaluateDesignData(
        { 'src/design.rs': 'pub const VALUE: i32 = 42;' },
        ['design.VALUE'],
        { tempRoot: parent },
      ),
      { 'design.VALUE': '42' },
    );
    assert.deepEqual(
      (await readdir(parent)).filter(name => name.startsWith('forma-design-')),
      [],
    );
    await assert.rejects(
      evaluateDesignData({ 'src/design.rs': 'not valid Rust' }, ['design.VALUE'], {
        tempRoot: parent,
      }),
    );
    assert.deepEqual(
      (await readdir(parent)).filter(name => name.startsWith('forma-design-')),
      [],
    );
    await assert.rejects(
      evaluateDesignData(
        { 'src/design.rs': 'pub fn value() -> i32 { panic!("expected failure") }' },
        ['design.value()'],
        { tempRoot: parent },
      ),
    );
    assert.deepEqual(
      (await readdir(parent)).filter(name => name.startsWith('forma-design-')),
      [],
    );
  } finally {
    await rm(parent, { recursive: true, force: true, maxRetries: 10, retryDelay: 100 });
  }
});

test('design result reads enforce a total byte budget', async () => {
  const dir = await mkdtemp(path.join(tmpdir(), 'forma-result-test-'));
  try {
    await writeFile(path.join(dir, 'value-0'), Buffer.alloc(1024 * 1024, 97));
    assert.equal((await readDesignValues(dir, ['design.a']))['design.a'].length, 1024 * 1024);
    await writeFile(path.join(dir, 'value-1'), 'b');
    await assert.rejects(readDesignValues(dir, ['design.a', 'design.b']), /1 МиБ/);
    await writeFile(path.join(dir, 'value-0'), Buffer.alloc(1024 * 1024 + 1));
    await assert.rejects(readDesignValues(dir, ['design.a']), /1 МиБ/);
  } finally {
    await rm(dir, { recursive: true, force: true, maxRetries: 10, retryDelay: 100 });
  }
});

test('design evaluation rejects unsafe blocks, process/fs/net APIs, bypass tricks, and include! macros before invoking rustc', async () => {
  for (const bad of [
    'pub fn x() -> i32 { unsafe { 1 } }',
    'pub fn x() -> i32 { r#unsafe { 1 } }',
    'pub fn x() -> String { std::process::Command::new("sh"); String::new() }',
    'use std :: { fs as f }; pub fn x() -> i32 { 1 }',
    'use core::{ptr, mem}; pub fn x() -> i32 { 1 }',
    'use r#std::r#net::TcpStream; pub fn x() -> i32 { 1 }',
    'pub const X: &str = include_str!("/etc/passwd");',
    'pub const X: &str = include_str!("/etc/hosts");',
    'pub const X: &str = env!("HOME");',
    '#[path = "/etc/passwd"] mod x; pub const X: i32 = 1;',
    'extern "C" { fn abort(); } pub const X: i32 = 1;',
    'pub fn x() -> usize { core::ptr::null::<u8>() as usize }',
    '#![allow(dead_code)] pub const X: i32 = 1;',
    'macro_rules! m { () => { 1 }; } pub fn x() -> i32 { 1 }',
    'pub async fn x() -> i32 { 1 }',
    'pub fn x() -> i32 { let s = "unclosed; 1 }',
  ]) {
    assert.throws(() => validateDesignRustSource(bad), /запрещённые конструкции/, bad);
    await assert.rejects(
      evaluateDesignData({ 'src/design.rs': bad }, ['design.x()']),
      /запрещённые конструкции/,
    );
  }
});

test('design evaluation allows comments, string literals, raw strings, lifetimes, derive/allow, and pure macros', async () => {
  const parent = await mkdtemp(path.join(tmpdir(), 'forma-design-benign-'));
  try {
    const source = `
// unsafe std::fs env!("PATH")
/* nested /* core::ptr::null extern "C" */ use std::process; */
#[allow(dead_code)]
#[derive(Debug)]
struct Palette {
  name: &'static str,
}

fn pick<'a>(s: &'a str) -> &'a str { s }

pub fn label() -> String {
  let msg = "unsafe std::fs env!(\\"PATH\\") 'quote";
  let raw = r#"std::net::TcpStream "#;
  let items = vec![pick("forma"), msg, raw];
  format!("{}:{}", items[0], items.len())
}
`;
    assert.deepEqual(
      await evaluateDesignData({ 'src/design.rs': source }, ['design.label()'], {
        tempRoot: parent,
      }),
      { 'design.label()': 'forma:3' },
    );
  } finally {
    await rm(parent, { recursive: true, force: true, maxRetries: 10, retryDelay: 100 });
  }
});

test('runSandboxedDesignBinary strips parent environment and confines writes to temp dir', async () => {
  const root = await mkdtemp(path.join(tmpdir(), 'forma-sandbox-test-'));
  const workDir = path.join(root, 'work');
  const outsideDir = path.join(root, 'outside');
  const { mkdir } = await import('node:fs/promises');
  await mkdir(workDir);
  await mkdir(outsideDir);
  const previousSecret = process.env.FORMA_TEST_SECRET;
  process.env.FORMA_TEST_SECRET = 'super-secret-token';
  try {
    await writeFile(
      path.join(workDir, 'probe.rs'),
      `fn main() {
        assert!(std::env::var("FORMA_TEST_SECRET").is_err(), "parent env leaked");
        std::fs::write("inside.txt", "ok").unwrap();
      }`,
    );
    await exec('rustc', ['--edition=2021', 'probe.rs', '-o', 'probe'], { cwd: workDir });
    await runSandboxedDesignBinary(await realpath(path.join(workDir, 'probe')), workDir);
    assert.equal(await readFile(path.join(workDir, 'inside.txt'), 'utf8'), 'ok');

    if (process.platform === 'darwin' && existsSync('/usr/bin/sandbox-exec')) {
      const escapeTarget = path.join(await realpath(outsideDir), 'escape.txt');
      await writeFile(
        path.join(workDir, 'escape.rs'),
        `fn main() {
          std::fs::write(${JSON.stringify(escapeTarget)}, "escaped").unwrap();
        }`,
      );
      await exec('rustc', ['--edition=2021', 'escape.rs', '-o', 'escape'], { cwd: workDir });
      await assert.rejects(
        runSandboxedDesignBinary(await realpath(path.join(workDir, 'escape')), workDir),
      );
      assert.equal(existsSync(escapeTarget), false);
    }
  } finally {
    if (previousSecret === undefined) delete process.env.FORMA_TEST_SECRET;
    else process.env.FORMA_TEST_SECRET = previousSecret;
    await rm(root, { recursive: true, force: true, maxRetries: 10, retryDelay: 100 });
  }
});
