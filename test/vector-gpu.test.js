import {test} from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';

// Execute the real painter: only Vite's raw-shader import and the relative
// module URL need adapting for a Node in-memory module. No actual GPU is used.
const source = (await readFile(new URL('../src/vector-gpu.js', import.meta.url), 'utf8'))
  .replace("import shader from '../vector-ui/src/vector.wgsl?raw';", "const shader = 'mock WGSL';")
  .replace("'./gpu-buffers.js'", JSON.stringify(new URL('../src/gpu-buffers.js', import.meta.url).href));
const {createGpuPainter} = await import(`data:text/javascript;base64,${Buffer.from(source).toString('base64')}`);

function model(commandFloats = 20) {
  const commands = new Float32Array(commandFloats), edges = new Float32Array(4), tiles = new Uint32Array(8);
  return {commands, edges, tiles, scroll_offset: () => [0, 0], gpu_commands: () => commands,
    gpu_edges: () => edges, gpu_tiles: () => tiles, gpu_paints: () => new Float32Array(8), gpu_params: () => new Float32Array(12)};
}

async function fixture(t) {
  const created = [], groups = [], usedGroups = [];
  const state = {failWriteData: null, failBind: false, bindAttempts: 0, submissions: 0, writes: []};
  const device = {
    limits: {maxStorageBufferBindingSize: 4096, maxTextureDimension2D: 4096},
    lost: new Promise(() => {}), addEventListener() {}, destroy() {},
    createShaderModule() {return {};},
    async createRenderPipelineAsync() {return {getBindGroupLayout: () => ({})};},
    createBuffer({size}) {
      const buffer = {size, destroyed: false, destroy() {this.destroyed = true;}};
      created.push(buffer); return buffer;
    },
    createBindGroup({entries}) {
      state.bindAttempts++;
      if (state.failBind) {state.failBind = false; throw Error('injected bind-group failure');}
      const group = {buffers: entries.map(entry => entry.resource.buffer)};
      groups.push(group); return group;
    },
    queue: {
      writeBuffer(buffer, offset, data) {
        assert.equal(buffer.destroyed, false, 'upload to a destroyed buffer');
        if (data === state.failWriteData) {state.failWriteData = null; throw Error('injected upload failure');}
        buffer.data=Array.from(data);state.writes.push({buffer,data});
      },
      submit() {state.submissions++;},
    },
    createCommandEncoder() {
      return {finish: () => ({}), beginRenderPass: () => ({
        setPipeline() {}, draw() {}, end() {},
        setBindGroup(index, group) {
          assert.ok(group, 'missing bind group');
          assert.ok(group.buffers.every(buffer => !buffer.destroyed), 'stale bind group references a destroyed buffer');
          usedGroups.push(group);
        },
      })};
    },
  };
  const context = {configure() {}, unconfigure() {}, getCurrentTexture: () => ({createView: () => ({})})};
  const canvas = {width: 0, height: 0, getContext: () => context};
  const originals = ['navigator', 'GPUBufferUsage'].map(name => [name, Object.getOwnPropertyDescriptor(globalThis, name)]);
  for (const [name, value] of Object.entries({
    navigator: {gpu: {requestAdapter: async () => ({requestDevice: async () => device}), getPreferredCanvasFormat: () => 'bgra8unorm'}},
    GPUBufferUsage: {UNIFORM: 1, COPY_DST: 2, STORAGE: 4},
  })) Object.defineProperty(globalThis, name, {value, configurable: true});
  let painter;
  t.after(() => {
    try {painter?.destroy();} finally {
      for (const [name, descriptor] of originals) {
        if (descriptor) Object.defineProperty(globalThis, name, descriptor);
        else delete globalThis[name];
      }
    }
    assert.ok(created.every(buffer => buffer.destroyed), 'buffer leaked after painter destruction');
  });
  painter = await createGpuPainter(canvas, error => assert.fail(`unexpected GPU failure callback: ${error}`));
  return {painter, created, groups, usedGroups, state};
}

test('same painter retries after one buffer grows and a later upload fails', async t => {
  const {painter, groups, usedGroups, created, state} = await fixture(t);
  painter.draw(model(), 64, 64, 1);
  const oldGroup = groups[0], grown = model(100);
  state.failWriteData = grown.edges; // Command replacement succeeds first.
  assert.throws(() => painter.draw(grown, 64, 64, 1), /injected upload failure/);
  assert.equal(oldGroup.buffers[1].destroyed, true);
  assert.equal(state.submissions, 1);
  const count = created.length;
  painter.draw(grown, 64, 64, 1);
  assert.equal(created.length, count, 'retry must reuse the successful replacement');
  assert.equal(groups.length, 2, 'retry must rebuild the invalidated bind group');
  assert.notEqual(usedGroups.at(-1), oldGroup);
  assert.equal(state.submissions, 2);
  assert.equal(painter.snapshot().frames, 2);
});

test('same painter retries bind-group creation after replacing a buffer', async t => {
  const {painter, groups, usedGroups, created, state} = await fixture(t);
  painter.draw(model(), 64, 64, 1);
  const oldGroup = groups[0], grown = model(100);
  state.failBind = true;
  assert.throws(() => painter.draw(grown, 64, 64, 1), /injected bind-group failure/);
  assert.equal(oldGroup.buffers[1].destroyed, true);
  assert.equal(groups.length, 1);
  assert.equal(state.submissions, 1);
  const count = created.length;
  painter.draw(grown, 64, 64, 1);
  assert.equal(created.length, count, 'bind-group retry must not recreate storage');
  assert.equal(state.bindAttempts, 3);
  assert.equal(groups.length, 2);
  assert.equal(usedGroups.at(-1), groups[1]);
  assert.equal(state.submissions, 2);
  assert.equal(painter.snapshot().frames, 2);
});

test('editing geometry in the same runtime uploads fresh text; unchanged paint keeps geometry',async t=>{
 const {painter}=await fixture(t);const scene=model();let revision=0;scene.geometry_revision=()=>revision;
 painter.draw(scene,64,64,1);painter.draw(scene,64,64,1);assert.equal(painter.snapshot().uploads,1);
 revision++;painter.draw(scene,64,64,1);assert.equal(painter.snapshot().uploads,2);
 painter.draw(scene,64,64,1);assert.equal(painter.snapshot().uploads,2);
});

test('unchanged revisions reuse GPU payloads while paint and viewport changes invalidate only their data',async t=>{
  const {painter,state}=await fixture(t),scene=model();
  const calls={};let visual=0,geometry=0;
  scene.visual_revision=()=>visual;scene.geometry_revision=()=>geometry;
  for(const key of ['gpu_commands','gpu_edges','gpu_tiles','gpu_paints','gpu_params']){
    const original=scene[key];calls[key]=0;scene[key]=(...args)=>{calls[key]++;return original(...args);};
  }
  painter.draw(scene,60,60,1);
  const firstWrites=state.writes.length,firstBytes=painter.snapshot().uploadedBytes;
  painter.draw(scene,60,60,1);
  assert.equal(state.writes.length,firstWrites,'same scene requires no storage or uniform uploads');
  assert.equal(painter.snapshot().uploadedBytes,firstBytes);
  assert.deepEqual(calls,{gpu_commands:1,gpu_edges:1,gpu_tiles:1,gpu_paints:1,gpu_params:1});
  visual++;
  painter.draw(scene,60,60,1);
  assert.equal(state.writes.length,firstWrites+2,'animation uploads only paint and uniform data');
  painter.draw(scene,64,64,1);
  assert.deepEqual(calls,{gpu_commands:1,gpu_edges:1,gpu_tiles:1,gpu_paints:2,gpu_params:3},'same tile grid updates only viewport params');
  painter.draw(scene,65,64,1);
  assert.deepEqual(calls,{gpu_commands:1,gpu_edges:1,gpu_tiles:2,gpu_paints:2,gpu_params:4});
  geometry++;
  painter.draw(scene,65,64,1);
  assert.deepEqual(calls,{gpu_commands:2,gpu_edges:2,gpu_tiles:3,gpu_paints:3,gpu_params:5},'geometry changes invalidate all dependent payloads even without a paint revision');
  assert.equal(painter.snapshot().frames,6);
});

test('retrying the previous model after a partial upload restores overwritten retained buffers',async t=>{
  const {painter,state,groups}=await fixture(t),original=model(),replacement=model();
  original.commands.fill(1);replacement.commands.fill(2);
  original.visual_revision=replacement.visual_revision=()=>0;
  painter.draw(original,64,64,1);
  const commandsBuffer=groups[0].buffers[1];
  state.failWriteData=replacement.edges;
  assert.throws(()=>painter.draw(replacement,64,64,1),/injected upload failure/);
  assert.equal(commandsBuffer.data[0],2,'the failed scene already overwrote retained command storage');
  painter.draw(original,64,64,1);
  assert.equal(commandsBuffer.data[0],1,'returning to the previous model must reload its geometry');
  assert.equal(groups.length,1,'capacity and bind-group identity remain reusable');
});
