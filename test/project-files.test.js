import { test } from 'node:test';
import assert from 'node:assert/strict';
import { compileComponents } from '../src/compiler/components.js';
import {
  componentPath,
  createFile,
  duplicateFile,
  moveDestination,
  newFileContent,
  renameFile,
  suggestComponentName,
} from '../src/workspace/project-files.js';

const project = () => ({ 'ui/Demo.ui': 'demo', 'ui/Other.ui': 'other', README: 'read me' });

test('a new file is added only where nothing exists', () => {
  const files = project();
  assert.deepEqual(createFile(files, 'ui/New.ui', 'new'), { ok: true });
  assert.equal(files['ui/New.ui'], 'new');
  assert.deepEqual(createFile(files, 'ui/Demo.ui', ''), {
    ok: false,
    reason: 'Файл уже существует: ui/Demo.ui',
  });
  assert.equal(files['ui/Demo.ui'], 'demo', 'the existing file keeps its text');
});

test('a name that would make the project unsaveable is refused', () => {
  const files = project();
  for (const path of [
    '../Outside.ui',
    'ui//Double.ui',
    'ui/./Dot.ui',
    '',
    'ui/',
    '__proto__',
    'a\\b',
  ]) {
    assert.deepEqual(createFile(files, path, ''), {
      ok: false,
      reason: `Недопустимое имя: ${path}`,
    });
    assert.equal(renameFile(files, 'README', path).ok, false, path);
  }
  assert.deepEqual(files, project());
  assert.equal(Object.getPrototypeOf(files), Object.prototype);
});

test('a rename moves the text and never lands on another file', () => {
  const files = project();
  assert.deepEqual(renameFile(files, 'ui/Demo.ui', 'ui/Main.ui'), { ok: true });
  assert.deepEqual(files, { 'ui/Main.ui': 'demo', 'ui/Other.ui': 'other', README: 'read me' });

  assert.deepEqual(renameFile(files, 'ui/Main.ui', 'ui/Other.ui'), {
    ok: false,
    reason: 'Файл уже существует: ui/Other.ui',
  });
  assert.equal(files['ui/Other.ui'], 'other');
  assert.equal(files['ui/Main.ui'], 'demo');
  assert.deepEqual(renameFile(files, 'ui/Gone.ui', 'ui/X.ui'), {
    ok: false,
    reason: 'Файл не найден: ui/Gone.ui',
  });
  // A name every object has is not a file.
  assert.equal(renameFile(files, 'toString', 'ui/X.ui').ok, false);
});

test('each duplicate gets a name of its own', () => {
  const files = project();
  assert.deepEqual(duplicateFile(files, 'ui/Demo.ui'), { ok: true, path: 'ui/Demo-copy.ui' });
  files['ui/Demo-copy.ui'] = 'edited copy';
  assert.deepEqual(duplicateFile(files, 'ui/Demo.ui'), { ok: true, path: 'ui/Demo-copy-2.ui' });
  assert.deepEqual(duplicateFile(files, 'ui/Demo.ui'), { ok: true, path: 'ui/Demo-copy-3.ui' });
  assert.equal(files['ui/Demo-copy.ui'], 'edited copy', 'an earlier copy is not overwritten');
  assert.equal(files['ui/Demo-copy-3.ui'], 'demo');

  assert.equal(duplicateFile(files, 'README').path, 'README-copy');
  files['a.b/notes'] = 'n';
  assert.equal(
    duplicateFile(files, 'a.b/notes').path,
    'a.b/notes-copy',
    'a dot in a folder is not an extension',
  );
  files['.env'] = 'e';
  assert.equal(duplicateFile(files, '.env').path, '.env-copy');
  assert.equal(duplicateFile(files, 'ui/Gone.ui').ok, false);
});

test('a drop names the destination, or nothing when the item would not move', () => {
  assert.equal(moveDestination('ui/Demo.ui', 'components'), 'components/Demo.ui');
  assert.equal(moveDestination('ui/Demo.ui', ''), 'Demo.ui');
  assert.equal(moveDestination('ui/Demo.ui', 'ui'), null);
  assert.equal(moveDestination('ui', 'ui'), null, 'a folder is not moved into itself');
  assert.equal(moveDestination('ui', 'ui/inner'), null, 'nor into a folder inside it');
  assert.equal(moveDestination('ui', 'components'), 'components/ui');
});

test('an extracted control is offered a component name no file has', () => {
  assert.equal(componentPath('Card'), 'components/Card.ui');
  const keyed = { type: 'Frame', props: { key: 'search-box' } };
  assert.equal(suggestComponentName({}, keyed), 'Searchbox');
  assert.equal(suggestComponentName({}, { type: 'Row', props: {} }), 'CustomRow');
  assert.equal(suggestComponentName({}, { props: { key: 5 } }), 'CustomComponent');
  assert.equal(suggestComponentName({}, undefined), 'CustomComponent');
  assert.equal(suggestComponentName({}, { type: 'Frame', props: { key: '--' } }), 'CustomFrame');

  const files = { 'components/Searchbox.ui': 'x', 'components/Searchbox2.ui': '' };
  // An empty file still has the name.
  assert.equal(suggestComponentName(files, keyed), 'Searchbox3');
});

test('a form created from the tree compiles whatever the file is called', () => {
  assert.equal(
    newFileContent('ui/Card.ui'),
    "component Card {\n    Frame {\n        Text {\n            text: 'Новое окно';\n        }\n    }\n}\n",
  );
  const names = {
    'ui/my-form.ui': 'myform',
    'ui/2col.ui': 'Form2col',
    'ui/123.ui': 'Form123',
    'ui/Окно.ui': 'NewWindow',
    'ui/_x.ui': '_x',
    'a.b/Deep Name.ui': 'DeepName',
  };
  for (const [path, name] of Object.entries(names)) {
    const content = newFileContent(path);
    assert.ok(content.startsWith(`component ${name} {`), `${path}: ${content.split('\n')[0]}`);
    assert.doesNotThrow(
      () => compileComponents({ [path]: content }, path, {}, { measureText: () => [80, 20] }),
      path,
    );
  }
  assert.equal(newFileContent('src/main.rs'), '');
  assert.equal(newFileContent('notes'), '');
});
