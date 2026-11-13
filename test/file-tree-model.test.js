import { test } from 'node:test';
import assert from 'node:assert/strict';
import { fileTreeHtml, groupTreePaths, visibleTreePaths } from '../src/file-tree-model.js';

const project = {
  'Cargo.toml': '',
  'ui/Search.ui': '',
  'components/Button.ui': '',
  'assets/icon.svg': '',
  'src/main.rs': '',
  'ui/Search.design.ui': '',
  'README.md': '',
};

test('a project with application forms hides its library and its images', () => {
  assert.deepEqual(visibleTreePaths(project), [
    'Cargo.toml',
    'ui/Search.ui',
    'src/main.rs',
    'ui/Search.design.ui',
    'README.md',
  ]);
  // A component library has no ui/ folder, so everything is listed.
  const library = { 'components/Button.ui': '', 'assets/icon.svg': '', 'README.md': '' };
  assert.deepEqual(visibleTreePaths(library), Object.keys(library));
});

test('the filter matches any part of the path, ignoring case', () => {
  assert.deepEqual(visibleTreePaths(project, 'SEARCH'), ['ui/Search.ui', 'ui/Search.design.ui']);
  assert.deepEqual(visibleTreePaths(project, 'src/'), ['src/main.rs']);
  assert.deepEqual(visibleTreePaths(project, 'button'), [], 'a hidden file is not found either');
  assert.deepEqual(visibleTreePaths(project, 'nothing'), []);
});

test('paths are grouped by folder in the order folders first appear', () => {
  assert.deepEqual(groupTreePaths(visibleTreePaths(project)), {
    folders: [
      ['ui', ['ui/Search.ui', 'ui/Search.design.ui']],
      ['src', ['src/main.rs']],
    ],
    rootFiles: ['Cargo.toml', 'README.md'],
  });
  assert.deepEqual(groupTreePaths(['a/b/c.ui', 'a/d.ui']), {
    folders: [
      ['a/b', ['a/b/c.ui']],
      ['a', ['a/d.ui']],
    ],
    rootFiles: [],
  });
  // A folder named like a property of every object is still a folder.
  assert.deepEqual(groupTreePaths(['constructor/x.ui']).folders, [
    ['constructor', ['constructor/x.ui']],
  ]);
});

test('rows mark the selected file, the kind of file and escape names', () => {
  const html = fileTreeHtml(['ui/Search.ui', 'src/main.rs', 'Cargo.toml'], 'src/main.rs');
  assert.equal(
    html,
    '<div class="folder">⌄ &nbsp; ui</div>' +
      '<button class="file " draggable="true" data-path="ui/Search.ui"><span class="ui">◇</span>Search.ui</button>' +
      '<div class="folder">⌄ &nbsp; src</div>' +
      '<button class="file active" draggable="true" data-path="src/main.rs"><span class="rust">R</span>main.rs</button>' +
      '<button class="file root-file " draggable="true" data-path="Cargo.toml"><span class="rust">⚙</span>Cargo.toml</button>',
  );
  const hostile = fileTreeHtml(['<img src=x onerror=1>/"a".ui'], null);
  assert.ok(!hostile.includes('<img'), hostile);
  assert.ok(hostile.includes('&lt;img'), hostile);
  assert.ok(!/data-path="[^"]*"a"/.test(hostile), 'a quote does not end the attribute');
  assert.equal(fileTreeHtml([], null), '');
});
