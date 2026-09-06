import assert from 'node:assert/strict';
import {EventEmitter} from 'node:events';
import test from 'node:test';
import {createVectorRunner} from '../vector-runner.js';

function harness(overrides = {}) {
  const events = [];
  const calls = {mkdir: [], mkdtemp: [], writes: [], spawns: [], signals: []};
  const children = [];
  const dependencies = {
    root: '/project with spaces',
    platform: 'darwin',
    async mkdir(...args) { calls.mkdir.push(args); },
    async mkdtemp(prefix) {
      calls.mkdtemp.push(prefix);
      return `${prefix}${calls.mkdtemp.length}`;
    },
    async writeFile(...args) { calls.writes.push(args); },
    spawn(...args) {
      calls.spawns.push(args);
      const child = new EventEmitter();
      child.pid = 100 + children.length;
      child.stdout = new EventEmitter();
      child.stderr = new EventEmitter();
      child.kill = signal => calls.signals.push([child.pid, signal]);
      children.push(child);
      return child;
    },
    kill(...args) { calls.signals.push(args); },
    ...overrides,
  };
  return {runner: createVectorRunner(event => events.push(event), dependencies), events, calls, children};
}

test('vector runner preserves the source in a unique snapshot and passes it to native Cargo', async () => {
  const {runner, events, calls, children} = harness();
  const source = "component ButtonDemo { Button { text: 'Найти'; } }";
  await runner.run({source});
  assert.deepEqual(calls.writes[0], ['/project with spaces/.forma/vector-1/snapshot.ui', source, 'utf8']);
  assert.deepEqual(calls.spawns[0], [
    'cargo', ['run', '--release', '--offline', '--manifest-path', '/project with spaces/vector-ui/Cargo.toml', '--features', 'native', '--', '/project with spaces/.forma/vector-1/snapshot.ui'],
    {cwd: '/project with spaces', detached: true, stdio: ['ignore', 'pipe', 'pipe']},
  ]);
  children[0].stdout.emit('data', Buffer.from('window opened'));
  children[0].stderr.emit('data', Buffer.from('compiling'));
  children[0].emit('close', 0, null);
  assert.deepEqual(events.map(event => event.kind), ['started', 'stdout', 'stderr', 'finished']);
  assert.equal(events.at(-1).code, 0);
  await runner.run({source: 'component Other {}'});
  assert.equal(calls.writes[1][0], '/project with spaces/.forma/vector-2/snapshot.ui');
  assert.equal(calls.writes[0][1], source);
  children[1].emit('close', 0, null);
});

test('vector runner rejects missing, empty, and oversized source before writing or spawning', async () => {
  const {runner, calls, events} = harness();
  for (const source of [undefined, 123, '', '  \n ', 'x'.repeat(100_001)]) await runner.run({source});
  await runner.run();
  assert.equal(calls.mkdir.length, 0);
  assert.equal(calls.spawns.length, 0);
  assert.equal(events.length, 6);
  assert.ok(events.every(event => event.kind === 'error'));
});

test('vector runner is busy from preparation until close and stops the Cargo process group', async () => {
  const {runner, calls, events, children} = harness();
  const launching = runner.run({source: 'component Demo {}'});
  await runner.run({source: 'component Duplicate {}'});
  await launching;
  runner.stop();
  runner.stop();
  await runner.run({source: 'component StillStopping {}'});
  assert.equal(calls.spawns.length, 1);
  assert.deepEqual(calls.signals, [[-100, 'SIGTERM']]);
  assert.equal(events.filter(event => event.kind === 'error').length, 2);
  children[0].emit('close', null, 'SIGTERM');
  assert.equal(events.at(-1).signal, 'SIGTERM');
  await runner.run({source: 'component Again {}'});
  assert.equal(calls.spawns.length, 2);
  children[1].emit('close', 0, null);
});

test('stopping during snapshot preparation never spawns a window later', async () => {
  let releaseWrite;
  const {runner, calls, events} = harness({writeFile: () => new Promise(resolve => { releaseWrite = resolve; })});
  const launching = runner.run({source: 'component Demo {}'});
  runner.stop();
  // Let the two preceding filesystem awaits complete.
  await new Promise(resolve => setImmediate(resolve));
  releaseWrite();
  await launching;
  assert.equal(calls.spawns.length, 0);
  assert.deepEqual(events.map(event => event.kind), ['finished']);
  assert.equal(events[0].signal, 'SIGTERM');
});

test('spawn failure and its stale close cannot clear the next running window', async () => {
  const {runner, calls, children, events} = harness();
  await runner.run({source: 'component First {}'});
  children[0].pid = undefined;
  children[0].emit('error', new Error('cargo not found'));
  await runner.run({source: 'component Second {}'});
  children[0].emit('close', -2, null);
  children[0].stdout.emit('data', Buffer.from('stale'));
  await runner.run({source: 'component Duplicate {}'});
  assert.equal(calls.spawns.length, 2);
  assert.equal(events.filter(event => event.kind === 'finished').length, 1);
  assert.equal(events.some(event => event.text === 'stale'), false);
  assert.match(events.at(-1).text, /уже/);
  children[1].emit('close', 0, null);
});

test('a failed snapshot write releases the busy guard and never invokes Cargo', async () => {
  let shouldFail = true;
  const {runner, events, calls, children} = harness({async writeFile() {
    if (shouldFail) throw new Error('disk full');
  }});
  await runner.run({source: 'component Demo {}'});
  assert.equal(calls.spawns.length, 0);
  assert.ok(events.some(event => event.kind === 'error' && event.text === 'disk full'));
  shouldFail = false;
  await runner.run({source: 'component Demo {}'});
  assert.equal(calls.spawns.length, 1);
  children[0].emit('close', 0, null);
});

test('Windows uses the direct child signal instead of a Unix process group', async () => {
  const {runner, calls, children} = harness({platform: 'win32'});
  await runner.run({source: 'component Demo {}'});
  assert.equal(calls.spawns[0][2].detached, false);
  runner.stop();
  assert.deepEqual(calls.signals, [[100, 'SIGTERM']]);
  children[0].emit('close', null, 'SIGTERM');
});
