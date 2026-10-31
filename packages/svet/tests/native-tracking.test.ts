import { expect, it } from 'vitest';
import { createWasmCoreInstance } from '../src/engine';

it('embedded WASM prunes and replaces dependencies without retaining terminal caches', () => {
  const core = createWasmCoreInstance(), a = core.svet_create_node(0), b = core.svet_create_node(0), observer = core.svet_create_node(2);
  core.svet_begin_tracking(observer); core.svet_track_read(a); core.svet_track_read(b); core.svet_end_tracking();
  expect(core.svet_trigger_and_flush(b)).toBe(observer + 1); core.svet_set_state(observer, 0);
  core.svet_begin_tracking(observer); core.svet_track_read(a); core.svet_end_tracking();
  expect(core.svet_trigger_and_flush(b)).toBe(0);
  core.svet_begin_tracking(observer); core.svet_track_read(b); core.svet_end_tracking();
  expect(core.svet_trigger_and_flush(a)).toBe(0); expect(core.svet_trigger_and_flush(b)).toBe(observer + 1);
});

it('embedded WASM preserves notifications after dependency reorder and ignores invalid reads', () => {
  const core = createWasmCoreInstance(), a = core.svet_create_node(0), b = core.svet_create_node(0), observer = core.svet_create_node(2);
  core.svet_begin_tracking(observer); core.svet_track_read(observer); core.svet_track_read(0xffffffff);
  core.svet_track_read(a); core.svet_track_read(b); core.svet_end_tracking();
  core.svet_begin_tracking(observer); core.svet_track_read(b); core.svet_track_read(a); core.svet_end_tracking();
  expect(core.svet_trigger_and_flush(b)).toBe(observer + 1); core.svet_set_state(observer,0);
  expect(core.svet_trigger_and_flush(a)).toBe(observer + 1);
});

it('embedded WASM rejects an old observer frame after the native ID is reused', () => {
  const core = createWasmCoreInstance(), a = core.svet_create_node(0), b = core.svet_create_node(0), old = core.svet_create_node(2);
  core.svet_begin_tracking(old); core.svet_track_read(a); core.svet_dispose_node(old);
  const fresh = core.svet_create_node(2); expect(fresh).toBe(old); expect(core.svet_is_tracking()).toBe(0);
  core.svet_track_read(b); core.svet_end_tracking(); expect(core.svet_get_state(fresh)).toBe(2);
  core.svet_begin_tracking(fresh); core.svet_track_read(b); core.svet_end_tracking();
  expect(core.svet_trigger_and_flush(a)).toBe(0); expect(core.svet_trigger_and_flush(b)).toBe(fresh + 1);
});

it('embedded WASM removes an unread dependency after a source is disposed mid-pass', () => {
  const core = createWasmCoreInstance(), a = core.svet_create_node(0), b = core.svet_create_node(0), c = core.svet_create_node(0), observer = core.svet_create_node(2);
  core.svet_begin_tracking(observer); core.svet_track_read(a); core.svet_track_read(b); core.svet_end_tracking();
  core.svet_begin_tracking(observer); core.svet_track_read(a); core.svet_dispose_node(a); core.svet_track_read(c); core.svet_end_tracking();
  expect(core.svet_trigger_and_flush(b)).toBe(0); expect(core.svet_trigger_and_flush(c)).toBe(observer + 1);
});

it('embedded WASM keeps outer tracking balanced across pause and invalid or recursive scopes', () => {
  const core = createWasmCoreInstance(), a = core.svet_create_node(0), ignored = core.svet_create_node(0), observer = core.svet_create_node(2);
  core.svet_begin_tracking(observer); expect(core.svet_is_tracking()).toBe(1);
  core.svet_pause_tracking(); expect(core.svet_is_tracking()).toBe(0); core.svet_track_read(ignored); core.svet_resume_tracking();
  core.svet_begin_tracking(a); expect(core.svet_is_tracking()).toBe(0); core.svet_track_read(ignored); core.svet_end_tracking();
  core.svet_begin_tracking(observer); expect(core.svet_is_tracking()).toBe(0); core.svet_track_read(ignored); core.svet_end_tracking();
  expect(core.svet_is_tracking()).toBe(1); core.svet_track_read(a); core.svet_end_tracking(); expect(core.svet_is_tracking()).toBe(0);
  expect(core.svet_trigger_and_flush(ignored)).toBe(0); expect(core.svet_trigger_and_flush(a)).toBe(observer + 1);
});
