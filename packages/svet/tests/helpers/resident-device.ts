export function gate<T = void>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>(yes => {
    resolve = yes;
  });
  return { promise, resolve };
}
export function device() {
  const done = gate(),
    started = gate(),
    mapping = gate(),
    mapped = gate(),
    lost = gate<{ message: string }>();
  const buffers: any[] = [],
    filters: string[] = [];
  const gpu = {
    buffers,
    filters,
    done,
    started,
    mapping,
    mapped,
    lost: lost.promise,
    renderDraws: [] as Array<{ buffer: any; values: number[]; count: number }>,
    textures: [] as any[],
    renderFailure: false,
    compute: undefined as undefined | ((inputs: Float32Array[], output: Float32Array) => void),
    onSubmit: undefined as undefined | (() => void),
    lose: () => lost.resolve({ message: 'test loss' }),
    pauseDone: false,
    pauseMap: false,
    validationError: false,
    limits: {
      maxBufferSize: 1024 * 1024,
      maxStorageBufferBindingSize: 1024 * 1024,
      maxComputeWorkgroupsPerDimension: 65535,
    },
    pushErrorScope(filter: string) {
      filters.push(filter);
    },
    async popErrorScope() {
      return filters.pop() === 'validation' && gpu.validationError
        ? { message: 'invalid kernel' }
        : null;
    },
    createBuffer({ size, usage = 0 }: { size: number; usage?: number }) {
      const buffer = {
        usage,
        bytes: new Uint8Array(size),
        destroyed: false,
        maps: 0,
        isMapped: false,
        async mapAsync() {
          this.maps++;
          mapping.resolve();
          if (gpu.pauseMap) await mapped.promise;
          if (this.destroyed) throw Error('buffer destroyed');
          this.isMapped = true;
        },
        getMappedRange() {
          return this.bytes.buffer;
        },
        unmap() {
          this.isMapped = false;
        },
        destroy() {
          this.destroyed = true;
          this.isMapped = false;
        },
      };
      buffers.push(buffer);
      return buffer;
    },
    createShaderModule({ code }: { code: string }) {
      return { code };
    },
    createBindGroupLayout(descriptor: any) {
      return descriptor;
    },
    createPipelineLayout(descriptor: any) {
      return descriptor;
    },
    createComputePipeline(descriptor: any) {
      return {
        getBindGroupLayout: () =>
          descriptor.layout === 'auto' ? {} : descriptor.layout.bindGroupLayouts[0],
      };
    },
    createBindGroup({ entries }: any) {
      return entries;
    },
    createRenderPipeline(descriptor: any) {
      if (gpu.renderFailure) throw Error('render failed');
      return descriptor;
    },
    createTexture() {
      const texture = {
        destroyed: false,
        createView: () => ({}),
        destroy() {
          this.destroyed = true;
        },
      };
      gpu.textures.push(texture);
      return texture;
    },
    createCommandEncoder() {
      const commands: Array<() => void> = [];
      let bindings: any[];
      return {
        beginComputePass() {
          return {
            setPipeline() {},
            setBindGroup(_slot: number, entries: any[]) {
              bindings = entries;
            },
            dispatchWorkgroups() {
              const entries = bindings;
              commands.push(() => {
                const output = entries.at(-1)!.resource;
                const values = new Float32Array(output.buffer.bytes.buffer, 0, output.size / 4);
                const inputs = entries
                  .slice(0, -1)
                  .map(
                    entry =>
                      new Float32Array(
                        entry.resource.buffer.bytes.buffer,
                        0,
                        entry.resource.size / 4,
                      ),
                  );
                if (gpu.compute) {
                  gpu.compute(inputs, values);
                  return;
                }
                for (let i = 0; i < values.length; i++)
                  values[i] = inputs.reduce((sum, input) => sum + (input[i] ?? 0), 1) * 2;
              });
            },
            end() {},
          };
        },
        beginRenderPass() {
          let vertex: any;
          const draw = (count: number) =>
            commands.push(() => {
              if (!vertex || vertex.destroyed || !(vertex.usage & 0x20))
                throw Error('invalid resident vertex binding');
              gpu.renderDraws.push({
                buffer: vertex,
                values: Array.from(new Float32Array(vertex.bytes.buffer)),
                count,
              });
            });
          return {
            setPipeline() {},
            setVertexBuffer(_slot: number, buffer: any) {
              vertex = buffer;
            },
            setIndexBuffer() {},
            draw,
            drawIndexed: draw,
            end() {},
          };
        },
        copyBufferToBuffer(
          source: any,
          offset: number,
          target: any,
          targetOffset: number,
          length: number,
        ) {
          commands.push(() =>
            target.bytes.set(source.bytes.subarray(offset, offset + length), targetOffset),
          );
        },
        finish() {
          return commands;
        },
      };
    },
    queue: {
      writeBuffer(buffer: any, offset: number, data: any, start = 0, length = 0) {
        buffer.bytes.set(new Uint8Array(data, start, length), offset);
      },
      submit(batches: Array<Array<() => void>>) {
        for (const batch of batches) for (const command of batch) command();
        gpu.onSubmit?.();
      },
      async onSubmittedWorkDone() {
        started.resolve();
        if (gpu.pauseDone) await done.promise;
      },
    },
  };
  return gpu;
}
