import {
  getCurrentOwner,
  getReactiveGeneration,
  isOwnerScopeDisposed,
  runUntrackedInCore,
  runWithCapturedOwner,
} from './reactivity.ts';
import { getWasmCore } from './wasm-bridge.ts';
import {
  checkGpuSignals,
  computePipelineKey,
  computeQueues,
  computeQueueTelemetry,
  findComputePipeline,
  forgetComputePipeline,
  GpuComputeCapacityError,
  MAX_PENDING_COMPUTATIONS,
  pendingComputations,
  reserveGpuComputeAdmission,
  waitForGpu,
} from './gpu-compute-support.ts';
import {
  gpuState,
  retainComputePipeline,
  snapshotGpuComputeOptions,
  snapshotGpuOptionMetadata,
  tensorRecords,
  type WebGpuComputeDispatchOptions,
} from './gpu-state.ts';

interface GpuShaderModuleHandle {
  getCompilationInfo?(): PromiseLike<{
    messages: Array<{
      type: string;
      message: string;
      lineNum: number;
      linePos: number;
    }>;
  }>;
}

interface GpuComputePipelineHandle {
  getBindGroupLayout(index: number): unknown;
}

interface GpuComputeBufferHandle {
  destroy?(): void;
}

interface GpuStagingBufferHandle extends GpuComputeBufferHandle {
  mapAsync(mode: number, offset?: number, size?: number): PromiseLike<void>;
  getMappedRange(offset?: number, size?: number): ArrayBufferLike;
  unmap(): void;
}

interface GpuComputePassHandle {
  setPipeline(pipeline: unknown): void;
  setBindGroup(index: number, bindGroup: unknown): void;
  dispatchWorkgroups(x: number, y?: number, z?: number): void;
  end(): void;
}

interface GpuComputeEncoderHandle {
  clearBuffer(buffer: unknown, offset?: number, size?: number): void;
  copyBufferToBuffer(
    source: unknown,
    sourceOffset: number,
    destination: unknown,
    destinationOffset: number,
    size: number,
  ): void;
  beginComputePass(): GpuComputePassHandle;
  finish(): unknown;
}

export interface GpuComputeChainStage {
  wgsl: string;
  entryPoint?: string | undefined;
  workgroups?: [number, number?, number?];
  /** Private stage output length in floats; defaults to the final tensor length. */
  outputLength?: number;
}

export interface AsyncWebGpuComputeDispatchOptions extends Omit<
  WebGpuComputeDispatchOptions,
  'fallbackKernel'
> {
  /** Entry point defaults to main. Inputs use bindings 0..N-1, output uses N. */
  entryPoint?: string | undefined;
  signal?: AbortSignal | undefined;
  /** First private output length; the last stage must match output.length. */
  outputLength?: number;
  /** Up to 31 additional GPU stages. Each reads the previous result
   * at binding 0 and writes binding 1, seeded with the previous result's prefix.
   * One submission/readback/publication covers the whole chain. */
  chain?: readonly GpuComputeChainStage[];
  /** When hardware is absent, this callback implements the entire chain. */
  fallbackKernel?: (inputs: Float32Array[], output: Float32Array) => void | PromiseLike<void>;
}

export interface GpuComputeResult {
  backend: 'webgpu' | 'fallback';
  elements: number;
  uploadedBytes: number;
  readbackBytes: number;
}

/**
 * Executes WGSL on a configured device and atomically publishes mapped readback.
 * Inputs are snapshots captured at call time. Default workgroups assume 64 lanes;
 * supply workgroups when the shader uses a different workgroup size.
 * Missing compute support uses an explicit host fallback, never a silent no-op.
 */
export async function dispatchGpuComputeKernelAsync(
  options: AsyncWebGpuComputeDispatchOptions,
): Promise<GpuComputeResult> {
  const core = getWasmCore();
  const generation = getReactiveGeneration(core);
  const owner = getCurrentOwner();
  const assertContext = () => {
    if (isOwnerScopeDisposed(owner)) throw new Error('GPU computation owner disposed');
    if (generation !== getReactiveGeneration(core))
      throw new Error('GPU context reset during computation');
  };
  assertContext();
  options = snapshotGpuComputeOptions(options, assertContext);
  const chain: GpuComputeChainStage[] = [];
  if (options.chain !== undefined) {
    if (!Array.isArray(options.chain))
      throw new RangeError('GPU compute chain supports at most 31 additional stages');
    const length = options.chain.length;
    assertContext();
    if (!Number.isInteger(length) || length < 0 || length > 31)
      throw new RangeError('GPU compute chain supports at most 31 additional stages');
    for (const value of options.chain) {
      assertContext();
      if (chain.length >= 31)
        throw new RangeError('GPU compute chain supports at most 31 additional stages');
      const stage = snapshotGpuOptionMetadata(value, assertContext);
      if (
        typeof stage.wgsl !== 'string' ||
        (stage.entryPoint !== undefined && typeof stage.entryPoint !== 'string')
      ) {
        throw new TypeError('Invalid GPU compute chain shader or entry point');
      }
      chain.push(stage);
    }
    assertContext();
  }
  options.signal?.throwIfAborted();
  assertContext();
  const state = gpuState(core);
  const admissionDevice = state.device;
  const hardware =
    admissionDevice?.createComputePipeline &&
    admissionDevice.createShaderModule &&
    admissionDevice.createBindGroup &&
    admissionDevice.createBuffer &&
    admissionDevice.createCommandEncoder &&
    admissionDevice.queue.submit;
  assertContext();
  let releaseAdmission = () => {};
  if (hardware) {
    if ((pendingComputations.get(admissionDevice!) ?? 0) >= MAX_PENDING_COMPUTATIONS)
      throw new GpuComputeCapacityError('count', computeQueueTelemetry(admissionDevice!));
    let snapshotBytes = 0;
    for (const tensor of [...options.inputs, options.output]) {
      if (tensorRecords.get(tensor)?.core !== core)
        throw new Error('GPU tensor belongs to a different reactive context');
      snapshotBytes += tensor.length * Float32Array.BYTES_PER_ELEMENT;
    }
    releaseAdmission = reserveGpuComputeAdmission(admissionDevice!, snapshotBytes);
  }
  let enqueued = false;
  try {
    const inputViews = options.inputs.map(tensor => {
      if (tensorRecords.get(tensor)?.core !== core)
        throw new Error('GPU tensor belongs to a different reactive context');
      return tensor().slice();
    });
    const outputRecord = tensorRecords.get(options.output);
    if (outputRecord?.core !== core)
      throw new Error('GPU output belongs to a different reactive context');
    const workingOutput = options.output.peek().slice();
    const candidate = state.device;
    const kernels = [
      {
        wgsl: options.wgsl,
        entryPoint: options.entryPoint,
        workgroups: options.workgroups,
        outputLength: options.outputLength,
      },
      ...chain,
    ];
    const outputLengths = kernels.map(kernel => {
      const length = kernel.outputLength ?? workingOutput.length;
      // GPU-only intermediates are independent of the WASM arena limit.
      if (
        !Number.isSafeInteger(length) ||
        length < 0 ||
        length > Math.floor(Number.MAX_SAFE_INTEGER / Float32Array.BYTES_PER_ELEMENT)
      ) {
        throw new RangeError('Invalid GPU compute stage outputLength');
      }
      return length;
    });
    if (outputLengths.at(-1) !== workingOutput.length)
      throw new RangeError('Final GPU compute stage must match output tensor length');
    const outputByteLengths = outputLengths.map(length => length * Float32Array.BYTES_PER_ELEMENT);
    const maxOutputBytes = Math.max(...outputByteLengths);
    // The length check above fails for an empty chain, so there is a first stage here.
    const firstOutputBytes = outputByteLengths[0] ?? 0;
    const initialOutputBytes = Math.min(workingOutput.byteLength, firstOutputBytes);
    const dimensions: Array<[number, number, number]> = [];
    if (
      candidate?.createComputePipeline &&
      candidate.createShaderModule &&
      candidate.createBindGroup &&
      candidate.createBuffer &&
      candidate.createCommandEncoder &&
      candidate.queue.submit
    ) {
      for (const [stage, kernel] of kernels.entries()) {
        const groups = kernel.workgroups ?? [Math.ceil((outputLengths[stage] ?? 0) / 64), 1, 1];
        if (!Array.isArray(groups)) throw new RangeError('Invalid GPU workgroup dimensions');
        const length = groups.length;
        assertContext();
        if (!Number.isInteger(length) || length < 1 || length > 3)
          throw new RangeError('Invalid GPU workgroup dimensions');
        const counts: number[] = [];
        for (let i = 0; i < 3; i++) {
          const count = groups[i];
          assertContext();
          counts.push(i === 0 ? count! : (count ?? 1));
        }
        if (
          counts.some(
            n =>
              !Number.isInteger(n) ||
              n < 1 ||
              n > (candidate.limits?.maxComputeWorkgroupsPerDimension ?? 65535),
          )
        ) {
          throw new RangeError('Invalid GPU workgroup count');
        }
        dimensions.push([counts[0]!, counts[1]!, counts[2]!]);
      }
      for (const byteLength of [...inputViews.map(view => view.byteLength), ...outputByteLengths]) {
        if (
          !byteLength ||
          byteLength > (candidate.limits?.maxBufferSize ?? Infinity) ||
          byteLength > (candidate.limits?.maxStorageBufferBindingSize ?? Infinity)
        )
          throw new RangeError('Invalid GPU storage binding size');
      }
    } else if (!options.fallbackKernel)
      throw new Error('WebGPU compute is unavailable and no fallbackKernel was supplied');
    assertContext();
    const revision = ++outputRecord.revision;
    const cancellation = new AbortController();
    const cancel = () => cancellation.abort(new Error('GPU computation owner disposed'));
    if (owner && !owner.disposed) owner.disposables.push(cancel);
    const signals = [
      cancellation.signal,
      state.contextLifetime.signal,
      ...(options.signal ? [options.signal] : []),
    ];
    const assertCurrent = () => {
      if (isOwnerScopeDisposed(owner)) throw new Error('GPU computation owner disposed');
      checkGpuSignals(signals);
      if (generation !== getReactiveGeneration(core))
        throw new Error('GPU context reset during computation');
      if (outputRecord.revision !== revision)
        throw new Error('GPU output changed during computation');
    };
    const publish = (values: Float32Array, backend: GpuComputeResult['backend']) =>
      runUntrackedInCore(core, () => {
        assertCurrent();
        options.output.write(values);
        return {
          backend,
          elements: values.length,
          uploadedBytes:
            backend === 'webgpu'
              ? inputViews.reduce((sum, input) => sum + input.byteLength, initialOutputBytes)
              : 0,
          readbackBytes: backend === 'webgpu' ? values.byteLength : 0,
        };
      });
    try {
      const device = state.device;
      if (
        !device?.createComputePipeline ||
        !device.createShaderModule ||
        !device.createBindGroup ||
        !device.createBuffer ||
        !device.createCommandEncoder ||
        !device.queue.submit
      ) {
        if (!options.fallbackKernel)
          throw new Error('WebGPU compute is unavailable and no fallbackKernel was supplied');
        await waitForGpu(
          Promise.resolve().then(() =>
            runUntrackedInCore(core, () => {
              assertCurrent();
              return runWithCapturedOwner(core, owner, () =>
                options.fallbackKernel!(inputViews, workingOutput),
              );
            }),
          ),
          signals,
        );
        return publish(workingOutput, 'fallback');
      }
      const lifetime = state.lifetime;
      signals.push(lifetime.signal);
      let started = false;
      const execute = async (): Promise<GpuComputeResult> => {
        started = true;
        assertCurrent();
        const buffers: GpuComputeBufferHandle[] = [];
        const scopes: Array<'validation' | 'out-of-memory' | 'internal'> = [];
        let mapped = false;
        let staging: GpuStagingBufferHandle | undefined;
        let result: Float32Array | undefined;
        const pipelines: Array<{
          key: string | undefined;
          pipeline: GpuComputePipelineHandle | undefined;
        }> = [];
        let failed = false;
        let failure: unknown;
        try {
          if (device.pushErrorScope && device.popErrorScope) {
            for (const filter of ['validation', 'out-of-memory', 'internal'] as const) {
              device.pushErrorScope(filter);
              scopes.push(filter);
            }
          }
          for (const kernel of kernels) {
            const entryPoint = kernel.entryPoint ?? 'main';
            const key = computePipelineKey(kernel.wgsl, entryPoint);
            let pipeline =
              (findComputePipeline(device, key) as GpuComputePipelineHandle | undefined) ??
              (key === undefined ? undefined : pipelines.find(item => item.key === key)?.pipeline);
            // Remember attempted keys as well, so a later failure evicts all
            // pipelines used by this transaction before any result is published.
            const record = { key, pipeline };
            pipelines.push(record);
            if (!pipeline) {
              const shader = device.createShaderModule!({
                code: kernel.wgsl,
              }) as GpuShaderModuleHandle;
              if (shader.getCompilationInfo) {
                const info = await waitForGpu(shader.getCompilationInfo(), signals);
                const error = info.messages.find(message => message.type === 'error');
                if (error)
                  throw new Error(`WGSL ${error.lineNum}:${error.linePos}: ${error.message}`);
              }
              assertCurrent();
              pipeline = device.createComputePipeline!({
                layout: 'auto',
                compute: { module: shader, entryPoint },
              }) as GpuComputePipelineHandle;
              record.pipeline = pipeline;
            }
          }
          assertCurrent();
          const views = [...inputViews, workingOutput];
          const storage = views.map((view, binding) => {
            const isOutput = binding === inputViews.length;
            const size = isOutput ? firstOutputBytes : view.byteLength;
            const buffer = device.createBuffer!({
              size: isOutput ? maxOutputBytes : size,
              usage: 0x80 | 0x08 | (isOutput ? 0x04 : 0),
            }) as GpuComputeBufferHandle | null | undefined;
            if (!buffer) throw new Error('GPU compute buffer allocation failed');
            buffers.push(buffer);
            const uploaded = Math.min(view.byteLength, size);
            if (uploaded)
              device.queue.writeBuffer(buffer, 0, view.buffer, view.byteOffset, uploaded);
            return { binding, resource: { buffer, size } };
          });
          staging = device.createBuffer!({
            size: workingOutput.byteLength,
            usage: 0x01 | 0x08,
          }) as GpuStagingBufferHandle | undefined;
          if (!staging) throw new Error('GPU readback allocation failed');
          buffers.push(staging);
          const encoder = device.createCommandEncoder!() as GpuComputeEncoderHandle;
          const outputSlot = storage[inputViews.length];
          if (!outputSlot) throw new Error('GPU compute buffer allocation failed');
          let previous: GpuComputeBufferHandle = outputSlot.resource.buffer;
          let alternate: GpuComputeBufferHandle | undefined;
          if (chain.length) {
            alternate = device.createBuffer!({
              size: maxOutputBytes,
              usage: 0x80 | 0x08 | 0x04,
            }) as GpuComputeBufferHandle | undefined;
            if (!alternate) throw new Error('GPU compute chain buffer allocation failed');
            buffers.push(alternate);
          }
          for (const [i, { pipeline }] of pipelines.entries()) {
            assertCurrent();
            let entries = storage;
            if (i > 0) {
              const previousBytes = outputByteLengths[i - 1] ?? 0;
              const stageBytes = outputByteLengths[i] ?? 0;
              const copiedBytes = Math.min(previousBytes, stageBytes);
              if (stageBytes > copiedBytes)
                encoder.clearBuffer(alternate, copiedBytes, stageBytes - copiedBytes);
              encoder.copyBufferToBuffer(previous, 0, alternate, 0, copiedBytes);
              entries = [previous, alternate!].map((buffer, binding) => ({
                binding,
                resource: { buffer, size: binding === 0 ? previousBytes : stageBytes },
              }));
            }
            const group = device.createBindGroup!({
              layout: pipeline!.getBindGroupLayout(0),
              entries,
            });
            const pass = encoder.beginComputePass();
            pass.setPipeline(pipeline);
            pass.setBindGroup(0, group);
            const groups = dimensions[i];
            if (!groups) throw new RangeError('Invalid GPU workgroup dimensions');
            pass.dispatchWorkgroups(...groups);
            pass.end();
            if (i > 0) [previous, alternate] = [alternate!, previous];
          }
          encoder.copyBufferToBuffer(previous, 0, staging, 0, workingOutput.byteLength);
          device.queue.submit!([encoder.finish()]);
          await waitForGpu(staging.mapAsync(0x01, 0, workingOutput.byteLength), signals);
          mapped = true;
          result = new Float32Array(staging.getMappedRange(0, workingOutput.byteLength)).slice();
        } catch (error) {
          failed = true;
          failure = error;
        } finally {
          if (mapped && staging) {
            try {
              staging.unmap();
            } catch (error) {
              if (!failed) {
                failed = true;
                failure = error;
              }
            }
          }
          for (const buffer of buffers) {
            try {
              buffer.destroy?.();
            } catch (error) {
              if (!failed) {
                failed = true;
                failure = error;
              }
            }
          }
          if (scopes.length) {
            // Pop the whole group synchronously before awaiting any result.
            const results = scopes.reverse().map(filter => {
              try {
                return Promise.resolve(device.popErrorScope!()).then(error => {
                  if (error) throw new Error(`WebGPU ${filter}: ${error.message}`);
                });
              } catch (error) {
                return Promise.reject(error);
              }
            });
            try {
              await waitForGpu(Promise.all(results), signals);
            } catch (error) {
              if (!failed) {
                failed = true;
                failure = error;
              }
            }
          }
        }
        if (failed) {
          for (const { key } of pipelines) forgetComputePipeline(device, key);
          throw failure;
        }
        assertCurrent();
        // Error scopes must finish successfully before publishing to this cache.
        for (const { key, pipeline } of pipelines) retainComputePipeline(device, key, pipeline);
        return publish(result!, 'webgpu');
      };
      // Error scopes belong to a device, so serialize complete validation/readback
      // transactions on that device rather than interleave their scope stacks.
      const queued = (computeQueues.get(device) ?? Promise.resolve()).then(execute);
      const tail = queued.then(releaseAdmission, releaseAdmission);
      enqueued = true;
      computeQueues.set(device, tail);
      void tail.then(() => {
        if (computeQueues.get(device) === tail) computeQueues.delete(device);
      });
      try {
        return await waitForGpu(queued, signals);
      } catch (error) {
        // If work started, cancellation settles only after its resources are freed.
        if (started) {
          try {
            await queued;
          } catch {
            /* preserve cancellation */
          }
        }
        throw error;
      }
    } finally {
      const index = owner?.disposables.indexOf(cancel) ?? -1;
      if (index >= 0) owner!.disposables.splice(index, 1);
    }
  } finally {
    // Queued cancellation must retain its credit until the closure actually drains.
    if (!enqueued) releaseAdmission();
  }
}
