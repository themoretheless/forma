import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createProjectStore } from '../src/project-store.js';

/** A store over a recording browser storage and a folder whose writes the test releases. */
function storeWith({ browser = () => true } = {}) {
  const statuses = [];
  const folder = [];
  const store = createProjectStore({
    key: 'project',
    writeProject: (key, files) => browser(key, files),
    writeDirectoryProject: (handle, files, previous) =>
      new Promise((resolve, reject) => folder.push({ handle, files, previous, resolve, reject })),
    status: (status, text) => statuses.push([status, text]),
  });
  return { store, statuses, folder };
}
const turn = () => new Promise(resolve => setImmediate(resolve));

test('without a folder a save goes to the browser only', async () => {
  const { store, statuses, folder } = storeWith();
  await store.save({ 'a.ui': '1' });
  assert.deepEqual(statuses, [['saved', 'Сохранено']]);
  assert.deepEqual(folder, []);
  assert.equal(store.directory, null);
});

test('a failed browser write is reported and nothing goes to the folder', async () => {
  for (const browser of [
    () => false,
    () => {
      throw Error('quota');
    },
  ]) {
    const { store, statuses, folder } = storeWith({ browser });
    store.attach({ name: 'proj' }, {});
    await store.save({ 'a.ui': '1' });
    assert.deepEqual(statuses, [['unsaved', 'Не сохранено: экспортируйте проект']]);
    assert.deepEqual(folder, []);
  }
});

test('with a folder a save writes against what the folder already holds', async () => {
  const { store, statuses, folder } = storeWith();
  const handle = { name: 'proj' };
  store.attach(handle, { 'a.ui': '1' });
  const files = { 'a.ui': '2' };
  const saving = store.save(files);
  await turn();
  assert.equal(folder.length, 1);
  assert.equal(folder[0].handle, handle);
  assert.deepEqual(folder[0].previous, { 'a.ui': '1' });
  assert.deepEqual(folder[0].files, files);
  assert.notEqual(folder[0].files, files, 'the save works on a snapshot');
  assert.deepEqual(statuses, []);
  folder[0].resolve();
  await saving;
  assert.deepEqual(statuses, [['saved', 'Сохранено (proj)']]);

  // The next save is compared with what was just written.
  store.save({ 'a.ui': '3' });
  await turn();
  assert.deepEqual(folder[1].previous, { 'a.ui': '2' });
});

test('overlapping saves reach the folder one at a time, newest last', async () => {
  const { store, statuses, folder } = storeWith();
  store.attach({ name: 'proj' }, {});
  const first = store.save({ 'a.ui': 'old' });
  const second = store.save({ 'a.ui': 'new' });
  await turn();
  assert.equal(folder.length, 1, 'the second write waits for the first');
  assert.equal(folder[0].files['a.ui'], 'old');
  folder[0].resolve();
  await first;
  await turn();
  assert.equal(folder.length, 2);
  assert.equal(folder[1].files['a.ui'], 'new');
  assert.deepEqual(folder[1].previous, { 'a.ui': 'old' });
  folder[1].resolve();
  await second;
  assert.equal(statuses.length, 2);
});

test('losing the folder falls back to the browser and stops using it', async () => {
  const { store, statuses, folder } = storeWith();
  store.attach({ name: 'proj' }, {});
  const saving = store.save({ 'a.ui': '1' });
  await turn();
  folder[0].reject(Error('permission'));
  await saving;
  assert.deepEqual(statuses, [['saved', 'Сохранено в браузере (доступ к папке потерян)']]);
  assert.equal(store.directory, null);
  await store.save({ 'a.ui': '2' });
  assert.equal(folder.length, 1);
  assert.deepEqual(statuses.at(-1), ['saved', 'Сохранено']);
});

test('a save queued for a folder that was replaced is not written to the new one', async () => {
  const { store, folder } = storeWith();
  const old = { name: 'old' },
    next = { name: 'next' };
  store.attach(old, {});
  const first = store.save({ 'a.ui': '1' });
  const queued = store.save({ 'a.ui': '2' });
  await turn();
  store.attach(next, { 'b.ui': 'x' });
  folder[0].resolve();
  await first;
  await queued;
  assert.equal(folder.length, 1, 'the queued save for the old folder is dropped');
  store.save({ 'b.ui': 'y' });
  await turn();
  assert.equal(folder[1].handle, next);
  assert.deepEqual(
    folder[1].previous,
    { 'b.ui': 'x' },
    'the old folder did not overwrite the record',
  );

  store.detach();
  assert.equal(store.directory, null);
});

test('writeToDirectory replaces the record of the folder and reports failure', async () => {
  const { store, folder } = storeWith();
  store.attach({ name: 'proj' }, { 'a.ui': '1' });
  const writing = store.writeToDirectory({ 'Cargo.toml': 'x' });
  await turn();
  assert.deepEqual(folder[0].previous, { 'a.ui': '1' });
  folder[0].resolve();
  await writing;
  store.save({ 'Cargo.toml': 'y' });
  await turn();
  assert.deepEqual(folder[1].previous, { 'Cargo.toml': 'x' });
  folder[1].resolve();

  const failing = store.writeToDirectory({ 'Cargo.toml': 'z' });
  await turn();
  folder[2].reject(Error('disk full'));
  await assert.rejects(failing, /disk full/);
  // A failed export does not block later saves.
  store.save({ 'Cargo.toml': 'w' });
  await turn();
  assert.equal(folder.length, 4);
});
