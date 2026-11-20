import type { SvetWasmExports } from './wasm-bridge.ts';
import type { Mesh3D } from './gpu-scene3d-model.ts';
import type { CapturePickFrameInput } from './gpu-scene3d-controls.ts';

export interface Scene3DCanvas2DOptions {
  renderCore: SvetWasmExports;
  faces?: Array<[number, number, number, number]> | undefined;
  edges?: Array<[number, number]> | undefined;
  fillStyle?: string | undefined;
  strokeStyle?: string | undefined;
  pointColor?: string | undefined;
  showVertices?: boolean | undefined;
  getTargetCanvas(): HTMLCanvasElement;
  selectBackend(backend: string): void;
  onClearPick(): void;
  onCapturePick(frame: CapturePickFrameInput): void;
}

export function createScene3DCanvas2DRenderer(options: Scene3DCanvas2DOptions) {
  let screenCoords = new Float32Array(0);

  return (mesh: Mesh3D, clipVerts: Float32Array, width: number, height: number): void => {
    const halfW = width * 0.5;
    const halfH = height * 0.5;
    const count = mesh.vertexCount;
    if (screenCoords.length < count * 2) {
      screenCoords = new Float32Array(count * 2);
    }
    for (let i = 0; i < count; i++) {
      const x = clipVerts[i * 4] ?? 0;
      const y = clipVerts[i * 4 + 1] ?? 0;
      const w = clipVerts[i * 4 + 3] ?? 1;
      const invW = 1.0 / w;
      screenCoords[i * 2] = halfW + x * invW * halfW;
      screenCoords[i * 2 + 1] = halfH - y * invW * halfH;
    }

    const ctx = options.getTargetCanvas().getContext?.('2d');
    options.selectBackend(ctx ? 'canvas2d-zsort' : 'unavailable');
    if (!ctx) return;

    options.onClearPick();
    ctx.clearRect(0, 0, width, height);

    // Rust clips homogeneous primitives before JS transports finite screen points.
    const clipPrimitive = (primIndices: readonly number[]): Array<[number, number, number]> => {
      const i0 = primIndices[0];
      const i1 = primIndices[1];
      if (i0 === undefined || i1 === undefined) return [];
      for (const index of primIndices) {
        if (!Number.isInteger(index) || index < 0 || index >= count) return [];
      }
      const size = options.renderCore.svet_gpu_clip_primitive_3d(
        mesh.transformedVertices.offset,
        count,
        i0,
        i1,
        primIndices[2] ?? 0,
        primIndices[3] ?? 0,
        primIndices.length,
      );
      if (size === 0) return [];
      // Clipping may grow WASM memory; construct the view after the call.
      const data = new Float32Array(
        options.renderCore.memory.buffer,
        options.renderCore.svet_gpu_clip_output_ptr() >>> 0,
        size * 4,
      );
      const points: Array<[number, number, number]> = [];
      for (let i = 0; i < size; i++) {
        const dx = data[i * 4] ?? 0;
        const dy = data[i * 4 + 1] ?? 0;
        const dz = data[i * 4 + 2] ?? 0;
        const w = data[i * 4 + 3] ?? 0;
        const px = halfW * (1 + dx / w);
        const py = halfH * (1 - dy / w);
        const pz = dz / w;
        if (!Number.isFinite(px) || !Number.isFinite(py) || !Number.isFinite(pz)) return [];
        points.push([px, py, pz]);
      }
      return points;
    };

    if (options.faces?.length) {
      const sortedFaces: Array<{ points: Array<[number, number, number]>; avgZ: number }> = [];
      for (const face of options.faces) {
        const points = clipPrimitive(face);
        if (points.length >= 3) {
          let sumZ = 0;
          for (const pt of points) sumZ += pt[2];
          sortedFaces.push({ points, avgZ: sumZ / points.length });
        }
      }
      sortedFaces.sort((a, b) => b.avgZ - a.avgZ);
      ctx.fillStyle = options.fillStyle ?? 'rgba(56, 189, 248, 0.14)';
      for (const face of sortedFaces) {
        const first = face.points[0];
        if (!first) continue;
        ctx.beginPath();
        ctx.moveTo(first[0], first[1]);
        for (let j = 1; j < face.points.length; j++) {
          const pt = face.points[j];
          if (pt) ctx.lineTo(pt[0], pt[1]);
        }
        ctx.closePath();
        ctx.fill();
      }
    }

    if (options.edges?.length) {
      ctx.strokeStyle = options.strokeStyle ?? '#38bdf8';
      ctx.lineWidth = 2;
      ctx.beginPath();
      for (const edge of options.edges) {
        const points = clipPrimitive(edge);
        const p0 = points[0];
        const p1 = points[1];
        if (points.length !== 2 || !p0 || !p1) continue;
        ctx.moveTo(p0[0], p0[1]);
        ctx.lineTo(p1[0], p1[1]);
      }
      ctx.stroke();
    }

    if (options.showVertices !== false) {
      // Scratch allocation can grow WASM memory and detach the original view.
      const markerVertices = mesh.transformedVertices.view();
      ctx.fillStyle = options.pointColor ?? '#34d399';
      for (let i = 0; i < count; i++) {
        const base = i * 4;
        const x = markerVertices[base] ?? NaN;
        const y = markerVertices[base + 1] ?? NaN;
        const z = markerVertices[base + 2] ?? NaN;
        const w = markerVertices[base + 3] ?? NaN;
        if (
          !Number.isFinite(x) ||
          !Number.isFinite(y) ||
          !Number.isFinite(z) ||
          !Number.isFinite(w) ||
          w <= 0 ||
          Math.abs(x) > w ||
          Math.abs(y) > w ||
          z < 0 ||
          z > w
        )
          continue;
        const px = screenCoords[i * 2] ?? 0;
        const py = screenCoords[i * 2 + 1] ?? 0;
        ctx.beginPath();
        ctx.arc(px, py, 3.5, 0, Math.PI * 2);
        ctx.fill();
      }
    }

    options.onCapturePick({
      mesh,
      vertices: mesh.transformedVertices.view(),
      indices: mesh.indices,
      width,
      height,
      backend: 'canvas2d-zsort',
    });
  };
}
