/**
 * # High-Speed GPU / WASM Arena Sparkline (`gpu-sparkline.ts`)
 *
 * Renders a reactive `GpuTensorSignal` or numeric array directly onto a `<canvas>`.
 */

import { effect, type ReadonlySignal } from './reactivity.ts';
import type { GpuTensorSignal } from './gpu.ts';

export interface GpuSparklineProps {
  tensor: GpuTensorSignal | ReadonlySignal<number[] | Float32Array>;
  width?: number;
  height?: number;
  strokeStyle?: string;
  fillStyle?: string;
  maxPoints?: number;
}

export function GpuSparkline(props: GpuSparklineProps): HTMLCanvasElement {
  const width = props.width ?? 320;
  const height = props.height ?? 72;
  const canvas = document.createElement('canvas');
  canvas.width = width;
  canvas.height = height;
  canvas.style.width = `${width}px`;
  canvas.style.height = `${height}px`;
  canvas.style.borderRadius = '6px';
  canvas.style.display = 'block';
  canvas.style.background = '#070b14';
  canvas.style.border = '1px solid #1e293b';
  canvas.setAttribute('data-svet-sparkline', 'true');

  effect(() => {
    const data = props.tensor();
    const ctx = canvas.getContext?.('2d');
    if (!ctx || !data || data.length === 0) return;

    ctx.clearRect(0, 0, width, height);
    const n = Math.min(data.length, props.maxPoints ?? 64);
    let minVal = Infinity;
    let maxVal = -Infinity;
    for (let i = 0; i < n; i++) {
      const v = data[i];
      if (v === undefined) continue;
      if (v < minVal) minVal = v;
      if (v > maxVal) maxVal = v;
    }
    if (!Number.isFinite(minVal) || !Number.isFinite(maxVal)) return;
    const span = maxVal - minVal || 1;
    const pad = 8;
    const usableH = height - pad * 2;

    ctx.beginPath();
    for (let i = 0; i < n; i++) {
      const val = data[i] ?? minVal;
      const x = (i / Math.max(1, n - 1)) * width;
      const y = height - pad - ((val - minVal) / span) * usableH;
      if (i === 0) ctx.moveTo(x, y);
      else ctx.lineTo(x, y);
    }
    ctx.strokeStyle = props.strokeStyle ?? '#34d399';
    ctx.lineWidth = 2;
    ctx.stroke();

    ctx.lineTo(width, height);
    ctx.lineTo(0, height);
    ctx.closePath();
    ctx.fillStyle = props.fillStyle ?? 'rgba(52, 211, 153, 0.12)';
    ctx.fill();
  });

  return canvas;
}
