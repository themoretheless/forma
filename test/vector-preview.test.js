import {test} from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';

// Run real preview event handlers and DOM updates. Most tests keep GPU
// initialization pending; backend lifecycle checks inject a recording painter.
const source = (await readFile(new URL('../src/vector-preview.js', import.meta.url), 'utf8'))
  .replace("import {createGpuPainter} from './vector-gpu.js';", 'let gpuFactory; export const setGpuFactory = factory => {gpuFactory=factory;}; const createGpuPainter = (...args) => gpuFactory?.(...args) ?? new Promise(() => {});');
const {createVectorPreview,setGpuFactory} = await import(`data:text/javascript;base64,${Buffer.from(source).toString('base64')}`);

class Element extends EventTarget {
  constructor(tag) {
    super();
    Object.assign(this, {tag, style: {}, dataset: {}, children: [], attributes: new Map(), captures: new Set(), focusCalls: 0,
      classList: {add() {}}, isConnected: true, parentElement: null});
  }
  setAttribute(key, value) {this.attributes.set(key, String(value));}
  getAttribute(key) {return this.attributes.get(key) ?? null;}
  removeAttribute(key) {this.attributes.delete(key);}
  append(...children) {for (const child of children) {child.parentElement = this; this.children.push(child);}}
  insertBefore(child, before) {child.parentElement = this; this.children.splice(this.children.indexOf(before), 0, child);}
  replaceChildren(...children) {for (const child of this.children) child.parentElement = null; this.children = []; this.append(...children);}
  remove() {if (this.parentElement) this.parentElement.children = this.parentElement.children.filter(child => child !== this); this.parentElement = null;}
  getContext() {return {putImageData: image => {this.lastImage=image;this.paints=(this.paints??0)+1;}};}
  getBoundingClientRect() {return {left: 0, top: 0, width: 120, height: 120};}
  setPointerCapture(id) {this.captures.add(id);}
  hasPointerCapture(id) {return this.captures.has(id);}
  releasePointerCapture(id) {this.captures.delete(id);}
  focus() {this.focusCalls++; this.emit('focus');}
  emit(type, values = {}) {
    const event = Object.assign(new Event(type, {cancelable: true}), values);
    this.dispatchEvent(event);
    return event;
  }
}

// This stub records API calls rather than enforcing passive-control behavior;
// an accidental preview activation/focus must fail the tests independently of
// the runtime guard. Real Rust focus/activation rules have their own unit test.
class Model {
  constructor() {
    this.focusCalls = []; this.focusControlCalls = []; this.keyCalls = []; this.pointerCalls = []; this.revealCalls = [];
    this.revealPresent = false;
    this.activateCalls = 0; this.pointerPresses = 0; this.count = 0; this.focused = -1; this.rasterCalls = 0; this.revision = 0;
  }
  load_component(source) {this.controls = JSON.parse(source);}
  preserve_interaction() {}
  free() {}
  control_count() {return this.controls.length;}
  control_interactive(i) {return this.controls[i]?.interactive ?? false;}
  control_disabled(i) {return this.controls[i]?.disabled ?? false;}
  control_label(i) {return `Control ${i}`;}
  control_key(i) {return this.controls[i]?.key ?? `control-${i}`;}
  control_action() {return 'actions.test';}
  control_bounds(i) {return this.controls[i].bounds;}
  control_hovered() {return false;}
  control_focused(i) {return this.focused === i;}
  control_clicks() {return this.count;}
  event_index() {return 0;}
  focused_index() {return this.focused;}
  width() {return 120;}
  height() {return 120;}
  render_width() {return 120;}
  render_height() {return 120;}
  content_pixels(width, height) {this.rasterCalls++;return this.lastPixels=new Uint8Array(width * height * 4);}
  background_color() {return '#111111';}
  frame_radius() {return 0;}
  visual_revision() {return this.revision;}
  is_animating() {return false;}
  tick() {return false;}
  focus(value) {this.focusCalls.push(value); this.focused = value ? 0 : -1;}
  focus_control(index) {this.focusControlCalls.push(index); this.focused = index; return true;}
  focus_next(reverse = false) {
    const index = this.focused < 0 ? (reverse ? this.controls.length - 1 : 0) : this.focused + (reverse ? -1 : 1);
    if (index < 0 || index >= this.controls.length) return false;
    this.focused = index;
    return true;
  }
  key_event(...args) {this.keyCalls.push(args);}
  pointer(x, y, kind) {
    this.pointerCalls.push({x, y, kind});
    if (kind === 1) this.pointerPresses++;
    // The real shared Runtime.pointer updates Reveal independently of the
    // browser's pointer type, so the adapter must clear touch after forwarding.
    this.revealPresent = kind !== 3;
  }
  reveal_pointer(x, y, present) {this.revealPresent = present; this.revealCalls.push({x, y, present});}
  activate() {this.activateCalls++; this.count++;}
  clicks() {return this.count;}
  disabled() {return this.controls.every(control => control.disabled);}
  hit_index(x, y) {return this.controls.findLastIndex(({bounds: [left, top, width, height]}) => x >= left && y >= top && x < left + width && y < top + height);}
  hit(x, y) {return this.hit_index(x, y) >= 0;}
  clipped() {return false;}
  overflow() {return 'visible';}
  scrollable() {return false;}
  scroll_offset() {return [0, 0];}
  raster_stats() {return [];}
  bounds() {return this.control_bounds(0);}
  label() {return this.control_label(0);}
  key() {return this.control_key(0);}
  action() {return this.control_action(0);}
}

function fixture(t, controls, designMode = false, options = {}) {
  const models = [], actions = [], selected = [], errors = [];
  const globals = {
    document: {createElement: tag => new Element(tag)},
    window: Object.assign(new EventTarget(), {devicePixelRatio: 1}),
    ImageData: class {constructor(data, width, height) {Object.assign(this, {data, width, height});}},
    requestAnimationFrame: () => assert.fail('static test scene must not animate'),
    cancelAnimationFrame() {},
  };
  const originals = Object.keys(globals).map(name => [name, Object.getOwnPropertyDescriptor(globalThis, name)]);
  for (const [name, value] of Object.entries(globals)) Object.defineProperty(globalThis, name, {value, configurable: true});
  const runtime = {Runtime: class extends Model {constructor() {super(); models.push(this);}}};
  setGpuFactory(options.gpuFactory);
  const preview = createVectorPreview({...options, runtime, onAction: action => actions.push(action), onSelect: node => selected.push(node), onError: error => errors.push(error)});
  setGpuFactory(null);
  const container = new Element('div');
  const render = (controls, designMode = false, extra = {}) => {
    const nodes = [{start: 1, type: 'Frame', children: controls.map((control, index) => ({start: index + 10, type: 'Control', props: {key: control.key ?? `control-${index}`}}))}];
    assert.equal(preview.render({container, source: JSON.stringify(controls), designMode, nodes, ...extra}), true);
    assert.deepEqual(errors, []);
  };
  render(controls, designMode);
  const canvas = container.children[0].children.find(child => child.className === 'forma-vector-canvas');
  t.after(() => {
    preview.destroy();
    for (const [name, descriptor] of originals) {
      if (descriptor) Object.defineProperty(globalThis, name, descriptor);
      else delete globalThis[name];
    }
    assert.deepEqual(errors, []);
  });
  return {preview, canvas, models, actions, selected, render, window: globals.window};
}

const passive = {interactive: false, disabled: false, bounds: [10, 10, 30, 30]};
const button = {...passive, interactive: true};
const pointer = (clientX, clientY) => ({clientX, clientY, pointerId: 1, button: 0, isPrimary: true});

test('canvas geometry snapshots reuse bounds without requesting diagnostic strings',t=>{
  const {preview,models,render}=fixture(t,[button,passive],true),model=models[0];
  const bounds=model.control_bounds.bind(model);let reads=0;
  model.control_bounds=i=>{reads++;return bounds(i);};
  model.control_label=()=>assert.fail('canvas layout must not marshal labels');
  model.control_action=()=>assert.fail('canvas layout must not marshal actions');
  const first=preview.layoutSnapshot();assert.equal(reads,2);
  for(let i=0;i<100;i++)assert.equal(preview.layoutSnapshot(),first);
  assert.equal(reads,2);
  assert.throws(()=>{first.controls[0].bounds[1]=999;},TypeError);
  model.controls[0].bounds=[10,5,30,30];model.revision++;
  const scrolled=preview.layoutSnapshot();assert.notEqual(scrolled,first);assert.equal(scrolled.controls[0].bounds[1],5);
  assert.equal(first.controls[0].bounds[1],10);
  model.control_label=Model.prototype.control_label;model.control_action=Model.prototype.control_action;
  render([{...button,bounds:[1,2,3,4]}],true);
  assert.deepEqual(preview.layoutSnapshot().controls[0].bounds,[1,2,3,4]);
  preview.destroy();assert.equal(preview.layoutSnapshot(),null);
});

test('paint revisions do not invalidate layout snapshots on versioned runtimes',t=>{
  const {preview,models}=fixture(t,[button],true),model=models[0];let layout=0,reads=0;
  const bounds=model.control_bounds.bind(model);
  model.layout_revision=()=>layout;model.control_bounds=i=>{reads++;return bounds(i);};
  const first=preview.layoutSnapshot();
  for(let i=0;i<100;i++){model.revision++;assert.equal(preview.layoutSnapshot(),first);}
  assert.equal(reads,1,'animation must reuse the immutable bounds');
  model.controls[0].bounds=[10,5,30,30];layout++;
  assert.notEqual(preview.layoutSnapshot(),first);assert.equal(reads,2);
});

test('selection and same-DPR resizes reuse the canvas while revision, DPI and model changes repaint',t=>{
  const {preview,canvas,models,render,window}=fixture(t,[button],true),model=models[0];
  assert.equal(model.rasterCalls,1);assert.equal(canvas.paints,1);
  for(let i=0;i<10;i++){
    render([button],true,{selectedStart:i%2?1:10});
    window.dispatchEvent(new Event('resize'));
    preview.select(10);
  }
  assert.equal(model.rasterCalls,1,'selection and layout-only updates must not rasterize unchanged pixels');
  assert.equal(canvas.paints,1);
  assert.equal(preview.snapshot().selectedStart,10);
  model.revision++;
  render([button],true);
  assert.equal(model.rasterCalls,2);assert.equal(canvas.paints,2);
  window.devicePixelRatio=2;window.dispatchEvent(new Event('resize'));
  assert.equal(model.rasterCalls,3);assert.equal(canvas.width,240);
  window.dispatchEvent(new Event('resize'));assert.equal(model.rasterCalls,3);
  render([{...button,value:1}],true);
  assert.equal(models[1].rasterCalls,1,'a replacement runtime with an equal revision still paints');
  assert.equal(canvas.paints,4);
});

test('CPU ImageData shares the owned RGBA result, including a nonzero byte offset',t=>{
  const {canvas,models,render}=fixture(t,[button]),model=models[0];
  assert.equal(canvas.lastImage.data.buffer,model.lastPixels.buffer);
  const bytes=new Uint8Array(120*120*4+16),pixels=bytes.subarray(8,bytes.length-8);
  pixels.fill(42);model.content_pixels=()=>pixels;model.revision++;
  render([button]);
  assert.equal(canvas.lastImage.data.buffer,bytes.buffer,'no full-frame copy is needed');
  assert.equal(canvas.lastImage.data.byteOffset,8);
  assert.equal(canvas.lastImage.data.byteLength,120*120*4);
  assert.equal(canvas.lastImage.data[0],42);
});

test('mode changes rasterize once after clearing interaction and unchanged reduced motion is not reapplied',t=>{
  const {models,render}=fixture(t,[button]),model=models[0];let reducedCalls=0;
  model.set_reduced_motion=()=>{reducedCalls++;model.revision++;};
  const focus=model.focus.bind(model);model.focus=value=>{focus(value);model.revision++;};
  render([button],true);assert.equal(model.rasterCalls,2,'mode change should render only the final interaction state');
  render([button],true);assert.equal(model.rasterCalls,2);assert.equal(reducedCalls,0);
  render([button],true,{reducedMotion:true});assert.equal(model.rasterCalls,3);assert.equal(reducedCalls,1);
  render([button],true,{reducedMotion:true});assert.equal(model.rasterCalls,3);assert.equal(reducedCalls,1);
});

test('GPU activation releases the CPU backing store and fallback repaints the same revision',async t=>{
  const draws=[];let fail,destroys=0;
  const {preview,canvas,models,render}=fixture(t,[button],false,{
    gpuFactory(gpuCanvas,onFailure){
      fail=onFailure;
      return Promise.resolve({draw(...args){draws.push(args);},snapshot(){return {};},destroy(){destroys++;}});
    },
  });
  assert.equal(models[0].rasterCalls,1);assert.equal(canvas.width,120);
  await Promise.resolve();
  assert.equal(draws.length,1,'backend activation draws even when model and revision match');
  assert.equal(canvas.width,1);assert.equal(canvas.height,1,'GPU presentation releases the unused CPU bitmap');
  assert.equal(canvas.style.width,'120px','pointer coordinates still use the original CSS size');
  render([button]);assert.equal(draws.length,1,'unchanged GPU presentation is reused');
  fail(Error('device disconnected'));
  assert.equal(destroys,1);assert.equal(canvas.width,120);assert.equal(canvas.height,120);
  assert.equal(models[0].rasterCalls,2,'fallback cannot reuse the released CPU canvas');
  assert.equal(canvas.style.opacity,'1');assert.equal(preview.snapshot().renderer,'rust-wasm-cpu');
  render([button]);assert.equal(models[0].rasterCalls,2);
});

test('destroy releases the canvas backing store even while the preview API remains referenced',t=>{
  const {preview,canvas,models}=fixture(t,[button]);
  assert.equal(canvas.width,120);assert.equal(canvas.height,120);
  preview.destroy();
  assert.equal(canvas.width,1);assert.equal(canvas.height,1);
  assert.equal(preview.snapshot(),null);assert.equal(models[0].rasterCalls,1);
  preview.destroy();assert.equal(canvas.width,1,'destroy remains idempotent');
});

test('a passive canvas is an image with no tab stop, pointer capture, or activation', t => {
  const {preview, canvas, models, actions} = fixture(t, [passive]);
  const model = models[0];
  assert.equal(canvas.getAttribute('role'), 'img');
  assert.equal(canvas.getAttribute('aria-disabled'), null);
  assert.equal(canvas.tabIndex, -1);
  canvas.emit('pointermove', pointer(20, 20));
  assert.equal(canvas.style.cursor, 'default');
  assert.equal(canvas.emit('pointerdown', pointer(20, 20)).defaultPrevented, true);
  assert.equal(canvas.focusCalls, 0);
  assert.equal(canvas.captures.size, 0);
  assert.equal(model.pointerPresses, 0);
  canvas.emit('focus');
  assert.deepEqual(model.focusCalls, []);
  assert.equal(canvas.emit('keydown', {key: 'Enter'}).defaultPrevented, false);
  canvas.emit('click', {detail: 0});
  assert.equal(preview.activate(), false);
  assert.equal(model.activateCalls, 0);
  assert.deepEqual(model.keyCalls, []);
  assert.deepEqual(actions, []);
  assert.equal(preview.snapshot().controls[0].interactive, false);
});

test('disabled clickable controls retain button semantics without accepting input', t => {
  const {preview, canvas, models} = fixture(t, [{...button, disabled: true}]);
  assert.equal(canvas.getAttribute('role'), 'button');
  assert.equal(canvas.getAttribute('aria-disabled'), 'true');
  assert.equal(canvas.tabIndex, -1);
  canvas.emit('pointermove', pointer(20, 20));
  canvas.emit('pointerdown', pointer(20, 20));
  canvas.emit('click', {detail: 0});
  assert.equal(canvas.style.cursor, 'default');
  assert.equal(canvas.focusCalls, 0);
  assert.equal(models[0].activateCalls, 0);
  assert.equal(preview.snapshot().controls[0].interactive, true);
});

test('a mixed group uses a pointer and accepts pointer focus only over an enabled interactive control', t => {
  const controls = [passive, {...button, bounds: [60, 10, 30, 30]}, {...button, disabled: true, bounds: [10, 60, 30, 30]}];
  const {preview, canvas, models} = fixture(t, controls);
  assert.equal(canvas.getAttribute('role'), 'group');
  assert.equal(canvas.getAttribute('aria-disabled'), 'false');
  assert.equal(canvas.tabIndex, 0);
  for (const [x, y, cursor] of [[20, 20, 'default'], [70, 20, 'pointer'], [50, 20, 'default'], [20, 70, 'default']]) {
    canvas.emit('pointermove', pointer(x, y));
    assert.equal(canvas.style.cursor, cursor);
  }
  canvas.emit('pointerdown', pointer(20, 20));
  canvas.emit('pointerdown', pointer(20, 70));
  assert.equal(canvas.focusCalls, 0);
  canvas.emit('pointerdown', pointer(70, 20));
  assert.equal(canvas.focusCalls, 1);
  assert.equal(canvas.captures.size, 1);
  assert.equal(models[0].pointerPresses, 1);
  canvas.emit('pointerup', pointer(70, 20));
  assert.equal(preview.activate(), true);
  assert.equal(models[0].activateCalls, 1);
  canvas.emit('pointerleave', pointer(121, 20));
  assert.equal(canvas.style.cursor, 'default');
  assert.deepEqual(preview.snapshot().controls.map(control => control.interactive), [false, true, true]);
});

test('passive items do not make a group with only disabled buttons focusable', t => {
  const {preview, canvas, models} = fixture(t, [passive, {...button, disabled: true, bounds: [60, 10, 30, 30]}]);
  assert.equal(models[0].disabled(), false, 'runtime disabled alone cannot describe this group');
  assert.equal(canvas.getAttribute('role'), 'group');
  assert.equal(canvas.tabIndex, -1);
  assert.equal(canvas.getAttribute('aria-disabled'), 'true');
  assert.equal(preview.activate(), false);
  assert.equal(models[0].activateCalls, 0);
});

test('replacing an interactive template with a passive one updates semantics and a stationary cursor', t => {
  const {canvas, render} = fixture(t, [button]);
  canvas.emit('pointermove', pointer(20, 20));
  assert.equal(canvas.style.cursor, 'pointer');
  render([passive]);
  assert.equal(canvas.getAttribute('role'), 'img');
  assert.equal(canvas.getAttribute('aria-disabled'), null);
  assert.equal(canvas.tabIndex, -1);
  assert.equal(canvas.style.cursor, 'default');
});

test('design mode keeps its crosshair and selects passive geometry', t => {
  const {canvas, selected, models} = fixture(t, [passive], true);
  assert.equal(canvas.style.cursor, 'crosshair');
  canvas.emit('pointermove', pointer(20, 20));
  canvas.emit('pointerdown', pointer(20, 20));
  assert.equal(canvas.style.cursor, 'crosshair');
  assert.equal(selected[0].start, 10);
  assert.equal(models[0].pointerPresses, 0);
  assert.equal(models[0].activateCalls, 0);
});

test('Shift+Tab enters a group at its last control and traverses every control before leaving', t => {
  const controls = Array.from({length: 3}, (_, index) => ({...button, bounds: [10 + index * 30, 10, 20, 20]}));
  const {canvas, models} = fixture(t, controls);
  const model = models[0];
  // The key originates at the following DOM element, outside the canvas.
  window.dispatchEvent(Object.assign(new Event('keydown'), {key: 'Tab', shiftKey: true}));
  canvas.emit('focus');
  assert.equal(model.focused_index(), 2);
  for (const index of [1, 0]) {
    assert.equal(canvas.emit('keydown', {key: 'Tab', shiftKey: true}).defaultPrevented, true);
    assert.equal(model.focused_index(), index);
  }
  assert.equal(canvas.emit('keydown', {key: 'Tab', shiftKey: true}).defaultPrevented, false);
  canvas.emit('blur');
  window.dispatchEvent(Object.assign(new Event('keydown'), {key: 'Tab', shiftKey: false}));
  canvas.emit('focus');
  assert.equal(model.focused_index(), 0, 'forward Tab must still enter at the first control');
});

test('a custom drag focuses its control before start and survives synchronous value rerenders', t => {
  const controls = [{...button, key: 'other'}, {...button, key: 'range', bounds: [60, 10, 30, 30]}];
  const calls = [];
  let view, value = 0;
  view = fixture(t, controls, false, {
    onControlPointer(phase, {node, x, y, width, height}) {
      calls.push({phase, key: node.props.key, x, y, width, height, focused: view.models.at(-1).focused_index()});
      // Real range specimens replace their source immediately when the value
      // changes, including inside the pointerdown/start callback.
      view.render([controls[0], {...controls[1], value: ++value}]);
      return true;
    },
  });
  const {canvas, models, actions} = view;
  canvas.emit('pointerdown', pointer(70, 20));
  assert.equal(canvas.focusCalls, 1);
  assert.deepEqual(models[0].focusControlCalls, [1]);
  assert.deepEqual(calls[0], {phase: 'start', key: 'range', x: 10, y: 10, width: 30, height: 30, focused: 1});
  assert.equal(models.length, 2, 'start has replaced the runtime');
  assert.equal(canvas.hasPointerCapture(1), true, 'value replacement must retain the active capture');

  canvas.emit('pointermove', {...pointer(75, 20), pointerId: 2});
  assert.equal(calls.length, 1, 'another pointer cannot update the drag');
  canvas.emit('pointermove', pointer(80, 20));
  assert.equal(models.length, 3);
  assert.equal(canvas.hasPointerCapture(1), true);
  assert.equal(calls[1].x, 20);
  canvas.emit('pointerup', pointer(85, 20));
  assert.equal(models.length, 4);
  assert.equal(canvas.captures.size, 0);
  canvas.emit('pointermove', pointer(80, 20));
  canvas.emit('pointerup', pointer(80, 20));
  assert.deepEqual(calls.map(({phase, key}) => [phase, key]), [['start', 'range'], ['move', 'range'], ['end', 'range']]);
  assert.equal(models.reduce((total, model) => total + model.pointerPresses + model.activateCalls, 0), 0);
  assert.deepEqual(actions, []);
});

for (const reason of ['designMode', 'disabled', 'removed']) {
  test(`a custom drag is cancelled when its control becomes ${reason}`, t => {
    const controls = [{...button, key: 'other'}, {...button, key: 'range', bounds: [60, 10, 30, 30]}];
    const phases = [];
    const {canvas, render, actions} = fixture(t, controls, false, {
      onControlPointer(phase) {phases.push(phase); return true;},
    });
    canvas.emit('pointerdown', pointer(70, 20));
    assert.equal(canvas.hasPointerCapture(1), true);
    if (reason === 'designMode') render(controls, true);
    else if (reason === 'disabled') render([controls[0], {...controls[1], disabled: true}]);
    else render([controls[0]]);
    assert.equal(canvas.captures.size, 0, 'the invalidated drag must release pointer capture');
    // Returning to the original scene must not resurrect the interrupted drag.
    render(controls);
    canvas.emit('pointermove', pointer(80, 20));
    canvas.emit('pointerup', pointer(80, 20));
    assert.deepEqual(phases, ['start']);
    assert.deepEqual(actions, []);
    canvas.emit('pointerdown', pointer(70, 20));
    assert.deepEqual(phases, ['start', 'start'], 'a new gesture can start after cancellation');
    assert.equal(canvas.hasPointerCapture(1), true);
    canvas.emit('pointercancel', pointer(70, 20));
    assert.equal(canvas.captures.size, 0);
  });
}

test('window blur clears Reveal proximity and rerender, scroll, or resize cannot revive it', t => {
  const {models, render, window} = fixture(t, [button]);
  const move = () => window.dispatchEvent(Object.assign(new Event('pointermove'), {...pointer(20, 20), pointerType: 'mouse'}));
  move();
  assert.deepEqual(models[0].revealCalls.at(-1), {x: 20, y: 20, present: true});
  window.dispatchEvent(new Event('blur'));
  assert.equal(models[0].revealCalls.at(-1).present, false);
  const callsAfterBlur = models[0].revealCalls.length;
  render([button]);
  window.dispatchEvent(new Event('scroll'));
  window.dispatchEvent(new Event('resize'));
  assert.equal(models[0].revealCalls.length, callsAfterBlur, 'the previous runtime must not reuse a stale pointer');
  render([{...button, value: 1}]);
  window.dispatchEvent(new Event('scroll'));
  window.dispatchEvent(new Event('resize'));
  assert.deepEqual(models[1].revealCalls, [], 'a replacement runtime must wait for a new pointer event');
  move();
  assert.deepEqual(models[1].revealCalls, [{x: 20, y: 20, present: true}]);
});

test('touch forwards ordinary pointer gestures and clears Reveal before a cached mouse can revive it', t => {
  const {canvas, models, render, window} = fixture(t, [button]);
  const model = models[0];
  const touch = (x = 20, y = 20) => ({...pointer(x, y), pointerType: 'touch'});
  const mouse = () => window.dispatchEvent(Object.assign(new Event('pointermove'), {...pointer(20, 20), pointerType: 'mouse'}));
  for (const [type, event, kind, captured] of [
    ['pointerdown', touch(), 1, true],
    ['pointermove', touch(25), 0, true],
    ['pointerleave', touch(121), 0, true],
    ['pointerup', touch(), 2, false],
    ['pointerleave', touch(121), 3, false],
  ]) {
    mouse();
    assert.equal(model.revealPresent, true);
    canvas.emit(type, event);
    assert.equal(model.pointerCalls.at(-1).kind, kind, `${type} still reaches Runtime.pointer`);
    assert.equal(model.revealPresent, false, `${type} clears Reveal after Runtime.pointer`);
    assert.equal(canvas.hasPointerCapture(1), captured);
    assert.equal(model.focused_index(), 0, 'clearing touch proximity must preserve focus');
    const revealCalls = model.revealCalls.length;
    render([button]);
    window.dispatchEvent(new Event('scroll'));
    window.dispatchEvent(new Event('resize'));
    assert.equal(model.revealCalls.length, revealCalls, `${type} clears the cached mouse position`);
  }
  assert.equal(canvas.focusCalls, 1);
  assert.deepEqual(model.pointerCalls.map(call => call.kind), [1, 0, 0, 2, 3]);
  for (const type of ['pointercancel', 'lostpointercapture']) {
    canvas.emit('pointerdown', touch());
    mouse();
    canvas.emit(type, touch());
    assert.equal(canvas.captures.size, 0);
    assert.equal(model.revealPresent, false);
    render([button]);
    assert.equal(model.revealPresent, false, `${type} cannot restore the cached mouse on render`);
  }
  mouse();
  canvas.emit('pointermove', {...pointer(20, 20), pointerType: 'mouse'});
  assert.equal(model.revealPresent, true, 'mouse Reveal remains available after a touch gesture');
});

test('a touch custom drag keeps focus and capture across value rerenders without mouse Reveal', t => {
  const controls = [{...button, key: 'other'}, {...button, key: 'range', bounds: [60, 10, 30, 30]}];
  const calls = [];
  let view, value = 0;
  view = fixture(t, controls, false, {
    onControlPointer(phase) {
      calls.push({phase, focused: view.models.at(-1).focused_index(), reveal: view.models.at(-1).revealPresent});
      view.render([controls[0], {...controls[1], value: ++value}]);
      return true;
    },
  });
  const {canvas, models, window, actions} = view;
  window.dispatchEvent(Object.assign(new Event('pointermove'), {...pointer(70, 20), pointerType: 'mouse'}));
  assert.equal(models[0].revealPresent, true);
  for (const type of ['pointerdown', 'pointermove', 'pointerup']) {
    canvas.emit(type, {...pointer(70, 20), pointerType: 'touch'});
    assert.equal(models.at(-1).revealPresent, false, 'a synchronous render must not reuse the previous mouse');
    assert.equal(canvas.hasPointerCapture(1), type !== 'pointerup');
  }
  assert.deepEqual(calls.map(call => call.phase), ['start', 'move', 'end']);
  assert.equal(calls[0].focused, 1);
  assert.ok(calls.every(call => !call.reveal), 'touch clears pointer Reveal before custom callbacks');
  assert.equal(models.reduce((total, model) => total + model.pointerPresses + model.activateCalls, 0), 0);
  assert.deepEqual(actions, []);
});

test('browser adapter commits composition once and routes clipboard into the Rust editor', t=>{
 const f=fixture(t,[button]);const model=f.models[0];const input=f.canvas.parentElement.children.find(c=>c.tag==='textarea');
 const inserted=[],preedit=[],keys=[];model.text_editing=()=>true;model.text_caret_bounds=()=>[10,10,1,20];
 model.text_insert=text=>inserted.push(text);model.text_preedit=text=>preedit.push(text);model.text_selected=()=>'selected';
 model.text_key=(...args)=>{keys.push(args);return args[0]==='Backspace';};
 input.emit('compositionstart');input.emit('compositionupdate',{data:'日本'});
 input.emit('beforeinput',{inputType:'insertCompositionText',data:'日本',isComposing:true});
 input.emit('compositionend',{data:'日本'});assert.deepEqual(inserted,['日本']);assert.deepEqual(preedit,['日本']);
 input.emit('beforeinput',{inputType:'insertText',data:'!'});assert.deepEqual(inserted,['日本','!']);
 input.emit('paste',{clipboardData:{getData:()=> 'pasted'}});assert.equal(inserted.at(-1),'pasted');
 let copied;input.emit('cut',{clipboardData:{setData:(type,text)=>copied=[type,text]}});assert.deepEqual(copied,['text/plain','selected']);assert.equal(inserted.at(-1),'');
 const key=input.emit('keydown',{key:'Backspace',shiftKey:false,ctrlKey:false,metaKey:false});assert.equal(key.defaultPrevented,true);assert.equal(keys.at(-1)[0],'Backspace');
 assert.equal(input.emit('keydown',{key:' ',shiftKey:false,ctrlKey:false,metaKey:false}).defaultPrevented,false,'space must reach beforeinput instead of activating the Button');
});

test('binding callbacks target expanded controls and publish normalized values before recompile', t=>{
 const changes=[];
 const f=fixture(t,[button],false,{onBindingChange:items=>changes.push(...items)});
 const node={start:90,type:'TextField',props:{key:'row/a',value:'old'},bindings:{value:'item.name'},environment:{locals:{item:{id:'a',name:'old'}}}};
 f.render([button],false,{nodes:[{type:'Frame',children:[{type:'Column',children:[node]}]}],previewControls:[node]});
 const model=f.models[0],input=f.canvas.parentElement.children.find(c=>c.tag==='textarea');
 model.control_editable=()=>true;model.text_value=()=>model.value??'old';
 model.text_insert=value=>{model.value=value;model.revision++;};
 input.emit('beforeinput',{inputType:'insertText',data:'new'});
 assert.equal(changes.length,1);assert.equal(changes[0].node,node);assert.equal(changes[0].value,'new');assert.equal(changes[0].path,'item.name');
 const slider={...node,type:'Slider',props:{key:'range',value:0.25},bindings:{value:'state.amount'}};
 f.render([button],false,{previewControls:[slider]});
 model.control_editable=()=>false;model.range_value=()=>0.75;
 input.emit('beforeinput',{inputType:'insertText',data:'unused'});
 assert.equal(changes.at(-1).value,0.75,'model range stays normalized to 0…1');
});

test('selected binding remains true on repeated activation while checked toggles',t=>{
 const changes=[];
 const f=fixture(t,[button],false,{onBindingChange:items=>changes.push(...items)});
 const node={start:90,type:'Checkbox',props:{key:'a',checked:false},bindings:{checked:'state.checked'}};
 f.render([button],false,{previewControls:[node]});
 f.canvas.emit('click',{detail:0});assert.equal(changes.at(-1).value,true);
 const selected={...node,props:{key:'a',selected:true},bindings:{selected:'state.selected'}};
 f.render([button],false,{previewControls:[selected]});
 f.canvas.emit('click',{detail:0});assert.equal(changes.length,1,'selecting an already selected item must not clear it');
});
