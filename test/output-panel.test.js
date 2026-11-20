import { test } from 'node:test';
import assert from 'node:assert/strict';
import {
  appendLog,
  diagnosticTargets,
  outputHtml,
  problemCount,
} from '../src/output/output-panel.js';

const base = { tab: 'problems', state: {}, logs: [], error: '', warnings: [] };

test('the journal keeps the newest two hundred entries', () => {
  const logs = [];
  for (let i = 0; i < 205; i++) appendLog(logs, `event ${i}`, '10:00:00');
  assert.equal(logs.length, 200);
  assert.deepEqual(logs[0], { time: '10:00:00', text: 'event 5' });
  assert.equal(logs.at(-1).text, 'event 204');
});

test('an error counts once and hides the warnings', () => {
  const warnings = [{ message: 'a' }, { message: 'b' }];
  assert.equal(problemCount('', []), 0);
  assert.equal(problemCount('', warnings), 2);
  assert.equal(problemCount('bad', warnings), 1);
  assert.equal(
    outputHtml({ ...base, error: 'bad <b>', warnings }),
    '<p class="error">● bad &lt;b&gt;</p>',
  );
  assert.equal(outputHtml({ ...base, warnings }), '<p class="warn">⚠ a</p><p class="warn">⚠ b</p>');
  assert.equal(outputHtml(base), '<p class="ok">✓ Разметка проверена. Ошибок нет.</p>');
});

test('the events tab lists the journal and the state tab shows the state', () => {
  assert.equal(
    outputHtml({ ...base, tab: 'events' }),
    '<p class="muted">События появятся при взаимодействии с предпросмотром.</p>',
  );
  assert.equal(
    outputHtml({ ...base, tab: 'events', logs: [{ time: '10:00:01', text: 'a <i>' }] }),
    '<div class="log"><time>10:00:01</time>a &lt;i&gt;</div>',
  );
  assert.equal(
    outputHtml({ ...base, tab: 'state', state: { query: '<x>', n: 1 }, error: 'ignored' }),
    '<pre>{\n  &quot;query&quot;: &quot;&lt;x&gt;&quot;,\n  &quot;n&quot;: 1\n}</pre>',
  );
});

test('an error links to its place in source and to the node it concerns', () => {
  const files = { 'ui/Demo.ui': 'line one\nline two\nline three', 'components/Button.ui': 'x' };
  const diagnostic = {
    message: 'bad',
    source: { file: 'ui/Demo.ui', from: 14, to: 17 },
    related: { file: 'components/Button.ui', from: 0, to: 1 },
  };
  assert.deepEqual(diagnosticTargets({ tab: 'problems', error: 'bad', diagnostic, files }), [
    { label: 'К ошибке · ui/Demo.ui:2', file: 'ui/Demo.ui', from: 14, to: 17 },
    {
      label: 'К определению узла · components/Button.ui:1',
      file: 'components/Button.ui',
      from: 0,
      to: 1,
    },
  ]);
});

test('no links for another tab, a stale diagnostic or a file that is gone', () => {
  const files = { 'ui/Demo.ui': 'a' };
  const diagnostic = { message: 'bad', source: { file: 'ui/Demo.ui', from: 0, to: 1 } };
  const ask = overrides =>
    diagnosticTargets({ tab: 'problems', error: 'bad', diagnostic, files, ...overrides });
  assert.equal(ask({}).length, 1);
  assert.deepEqual(ask({ tab: 'events' }), []);
  assert.deepEqual(ask({ error: 'another error' }), []);
  assert.deepEqual(ask({ diagnostic: null }), []);
  assert.deepEqual(ask({ files: {} }), []);
  // A file name every object has is not a project file.
  assert.deepEqual(
    ask({ diagnostic: { message: 'bad', source: { file: 'toString', from: 0, to: 1 } } }),
    [],
  );
});
