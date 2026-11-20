// Behaviour of the MCP project commands against a stand-in for the Studio.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createProjectCommands } from '../src/app/mcp-commands-project.js';

const DEMO = "component Demo { Frame { Text { text: 'a'; } } }";

function studioWith(files = { 'ui/Demo.ui': DEMO, 'assets/icon.svg': '<svg/>' }) {
  const calls = [];
  const studio = {
    files: { ...files },
    entry: 'ui/Demo.ui',
    active: 'ui/Demo.ui',
    scenario: 3,
    open: path => calls.push(['open', path]),
    codeEditor: { fold: args => calls.push(['fold', args]) },
    resetProjectEditing: () => calls.push(['resetProjectEditing']),
  };
  const commands = createProjectCommands(studio, {
    snapshot: () => 'snapshot',
    requireFile: path => {
      if (!Object.hasOwn(studio.files, path)) throw Error(`No file ${path}`);
    },
    applyFiles: () => calls.push(['applyFiles']),
  });
  return { studio, commands, calls };
}

test('reading commands return copies and refuse unknown files', () => {
  const { commands, studio, calls } = studioWith();
  assert.equal(commands.ide_status(), 'snapshot');
  const project = commands.project_read();
  project['ui/Demo.ui'] = 'changed';
  assert.equal(studio.files['ui/Demo.ui'], DEMO, 'project_read returns a copy');

  assert.deepEqual(commands.file_read({ path: 'ui/Demo.ui' }), {
    path: 'ui/Demo.ui',
    content: DEMO,
  });
  assert.throws(() => commands.file_read({ path: 'ui/None.ui' }), /No file/);

  assert.equal(commands.file_open({ path: 'assets/icon.svg' }), 'snapshot');
  assert.throws(() => commands.file_open({ path: 'ui/None.ui' }), /No file/);
  assert.equal(commands.editor_fold({ line: 2 }), 'snapshot');
  assert.deepEqual(calls, [
    ['open', 'assets/icon.svg'],
    ['fold', { line: 2 }],
  ]);
});

test('file_write needs the current content of a file it replaces', () => {
  const { commands, studio, calls } = studioWith();
  const next = DEMO.replace("'a'", "'b'");

  assert.throws(
    () => commands.file_write({ path: 'ui/Demo.ui', content: next }),
    /expectedContent missing/,
  );
  assert.throws(
    () => commands.file_write({ path: 'ui/Demo.ui', content: next, expectedContent: 'stale' }),
    /File changed/,
  );
  assert.equal(studio.files['ui/Demo.ui'], DEMO);
  assert.deepEqual(calls, []);

  assert.equal(
    commands.file_write({ path: 'ui/Demo.ui', content: next, expectedContent: DEMO }),
    'snapshot',
  );
  assert.equal(studio.files['ui/Demo.ui'], next);
  // A new file needs no expected content.
  assert.equal(commands.file_write({ path: 'ui/Other.ui', content: DEMO }), 'snapshot');
  assert.equal(studio.files['ui/Other.ui'], DEMO);
  assert.deepEqual(calls, [['applyFiles'], ['applyFiles']]);

  for (const path of ['', 5, undefined])
    assert.throws(() => commands.file_write({ path, content: DEMO }), /Invalid project path/);
  for (const path of [
    '../outside.ui',
    'ui//Double.ui',
    'ui/./Dot.ui',
    '__proto__/x.ui',
    'ui\\Back.ui',
  ])
    assert.throws(() => commands.file_write({ path, content: DEMO }), /Недопустимый путь проекта/);
  assert.throws(
    () => commands.file_write({ path: 'ui/Number.ui', content: 5 }),
    /Ожидается JSON проекта/,
  );
  assert.equal(Object.keys(studio.files).length, 3, 'a refused write adds nothing');
});

test('project_replace swaps the whole project only against the project it was read from', () => {
  const { commands, studio, calls } = studioWith();
  const replacement = {
    'ui/Main.design.ui': "design Main { Text { key: 't'; text: 'x'; } }",
    'ui/Main.ui': "component Main { Frame { Text { key: 't'; text: 'm'; } } }",
  };

  assert.throws(
    () => commands.project_replace({ files: replacement, expectedFiles: { 'ui/Demo.ui': DEMO } }),
    /Project changed/,
  );
  assert.deepEqual(calls, []);

  // The comparison ignores the order of the files.
  const expectedFiles = { 'assets/icon.svg': '<svg/>', 'ui/Demo.ui': DEMO };
  assert.equal(commands.project_replace({ files: replacement, expectedFiles }), 'snapshot');
  assert.deepEqual(studio.files, replacement);
  assert.notEqual(studio.files, replacement, 'the project is copied, not adopted');
  assert.equal(studio.active, 'ui/Main.design.ui', 'the first file becomes active');
  assert.equal(studio.entry, 'ui/Main.ui', 'a design file is never the entry');
  assert.equal(studio.scenario, 0);
  assert.deepEqual(calls, [['resetProjectEditing'], ['applyFiles']]);
});

test('project_export_native builds the export from the entry file', () => {
  const { commands } = studioWith();
  const exported = commands.project_export_native({});
  assert.equal(typeof exported, 'object');
  assert.ok(Object.keys(exported).length > 0);
});
