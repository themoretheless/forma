import { beforeEach, expect, it } from 'vitest';
import { createPerspectiveCamera3D, createTransform3D, createMesh3D, effect, getGraphTelemetry, getGpuTelemetry, resetReactiveSystem, type PerspectiveCamera3DOptions } from '../src/engine';

beforeEach(() => resetReactiveSystem());

it.each<PerspectiveCamera3DOptions>([
  { aspect: 0 }, { aspect: -1 }, { aspect: Infinity }, { aspect: Number.MAX_VALUE },
  { fovYRad: 0 }, { fovYRad: Math.PI }, { fovYRad: NaN }, { fovYRad: Number.MIN_VALUE },
  { near: 0 }, { near: -1 }, { far: 0 }, { near: 10, far: 10 },
  { near: 1, far: 1 + 1e-9 }, { aspect: 1e-45 },
])('rejects invalid or float32-degenerate initial projection %j before allocating', options => {
  const nodes = getGraphTelemetry().nodes, bytes = getGpuTelemetry().arenaByteLength;
  expect(() => createPerspectiveCamera3D(options)).toThrow('projection parameters');
  expect(getGraphTelemetry().nodes).toBe(nodes); expect(getGpuTelemetry().arenaByteLength).toBe(bytes);
});

it.each([['aspect', 0], ['fovYRad', Math.PI], ['near', 1000], ['far', 0]] as const)('rejects invalid %s set/update without notifying or changing scene data', (field, invalid) => {
  const camera = createPerspectiveCamera3D(), transform = createTransform3D();
  const mesh = createMesh3D({ positions: [1, 2, -3], transform, camera });
  const before = Array.from(mesh.transformedVertices.peek()), projection = Array.from(camera.projectionMatrix.peek());
  const old = camera[field].peek(); let runs = 0;
  const observer = effect(() => { camera.projectionMatrix(); runs++; });
  expect(() => camera[field].set(invalid)).toThrow('projection parameters');
  expect(() => camera[field].update(() => invalid)).toThrow('projection parameters');
  expect(camera[field].peek()).toBe(old); expect(runs).toBe(1);
  expect(Array.from(camera.projectionMatrix.peek())).toEqual(projection);
  expect(Array.from(mesh.transformedVertices.peek())).toEqual(before);
  observer.dispose(); mesh.dispose(); transform.dispose(); camera.dispose();
});

it('atomically replaces interdependent near/far values with one projection publication', () => {
  const camera = createPerspectiveCamera3D(); let runs = 0;
  const observer = effect(() => { camera.projectionMatrix(); runs++; });
  camera.setProjection({ near: 2000, far: 3000, aspect: 2 });
  expect(runs).toBe(2); expect(camera.near.peek()).toBe(2000); expect(camera.far.peek()).toBe(3000);
  const last = Array.from(camera.projectionMatrix.peek());
  expect(() => camera.setProjection({ far: 1000, aspect: 3 })).toThrow('projection parameters');
  expect(camera.aspect.peek()).toBe(2); expect(camera.far.peek()).toBe(3000);
  expect(Array.from(camera.projectionMatrix.peek())).toEqual(last); expect(runs).toBe(2);
  observer.dispose(); camera.dispose();
});

it('projects a large finite depth range without intermediate float32 overflow', () => {
  const camera = createPerspectiveCamera3D({ near: 1e20, far: 1e30 });
  expect(Array.from(camera.projectionMatrix.peek()).every(Number.isFinite)).toBe(true);
  expect(camera.projectionMatrix.peek()[14]).toBeLessThan(-9e19); camera.dispose();
});

it('does not resurrect a camera disposed by its parameter updater', () => {
  const camera = createPerspectiveCamera3D();
  expect(() => camera.aspect.set(() => { camera.dispose(); return 2; })).toThrow('disposed');
  expect(getGraphTelemetry().nodes).toBe(0);
  camera.aspect.update(() => { throw new Error('disposed updater must not run'); });
  expect(() => camera.setProjection({ aspect: 2 })).toThrow('disposed');
});
