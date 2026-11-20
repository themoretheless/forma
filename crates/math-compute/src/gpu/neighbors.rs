use super::support::*;

pub(super) struct GpuNearestNeighbor {
    device: Device,
    queue: wgpu::Queue,
    kernels: super::nearest_dispatch::NearestKernels,
    buffers: std::cell::RefCell<Option<NearestNeighborBuffers>>,
}

struct NearestNeighborBuffers {
    query_capacity: usize,
    target_capacity: usize,
    params: Buffer,
    queries: Buffer,
    targets: Buffer,
    out_index: Buffer,
    out_dist: Buffer,
    binds: [BindGroup; 2],
}

impl GpuNearestNeighbor {
    pub(super) fn new(context: &GpuContext) -> Result<Self, compute_core::KernelError> {
        let kernels = super::nearest_dispatch::NearestKernels::new(context)?;
        Ok(Self {
            device: context.device.clone(),
            queue: context.queue.clone(),
            kernels,
            buffers: std::cell::RefCell::new(None),
        })
    }

    fn alloc_buffers(&self, query_count: usize, target_count: usize) -> NearestNeighborBuffers {
        let device = &self.device;
        let query_capacity = query_count.max(1);
        let target_capacity = target_count.max(1);
        let params = mk(
            device,
            "nn_params",
            16,
            BufferUsages::UNIFORM | BufferUsages::COPY_DST,
        );
        let queries = mk(
            device,
            "nn_queries",
            (query_capacity * 12) as u64,
            BufferUsages::STORAGE | BufferUsages::COPY_DST,
        );
        let targets = mk(
            device,
            "nn_targets",
            (target_capacity * 12) as u64,
            BufferUsages::STORAGE | BufferUsages::COPY_DST,
        );
        let out_bytes = (query_capacity * 4) as u64;
        let out_index = mk(
            device,
            "nn_out_index",
            out_bytes,
            BufferUsages::STORAGE | BufferUsages::COPY_SRC,
        );
        let out_dist = mk(
            device,
            "nn_out_dist",
            out_bytes,
            BufferUsages::STORAGE | BufferUsages::COPY_SRC,
        );
        let binds = self.kernels.bind_groups(
            device,
            &[&params, &queries, &targets, &out_index, &out_dist],
        );
        NearestNeighborBuffers {
            query_capacity,
            target_capacity,
            params,
            queries,
            targets,
            out_index,
            out_dist,
            binds,
        }
    }

    pub(super) fn run(
        &self,
        queries: &[V3],
        targets: &[V3],
    ) -> Result<Vec<(u32, f64)>, gpu_compute::ReadbackError> {
        let query_count = queries.len();
        let target_count = targets.len();
        let mut slot = self.buffers.borrow_mut();
        if slot
            .as_ref()
            .is_some_and(|b| b.query_capacity < query_count || b.target_capacity < target_count)
        {
            *slot = None;
        }
        let b = slot.get_or_insert_with(|| self.alloc_buffers(query_count, target_count));
        // Uniform layout matches nearest_neighbor.wgsl's `Params`: two counts
        // padded to a 16-byte uniform buffer.
        let params = pack_u32(&[query_count as u32, target_count as u32, 0, 0]);
        self.queue.write_buffer(&b.params, 0, &params);
        if !queries.is_empty() {
            write_points(&self.queue, &b.queries, queries)?;
        }
        if !targets.is_empty() {
            write_points(&self.queue, &b.targets, targets)?;
        }
        if query_count == 0 {
            return Ok(Vec::new());
        }
        let (kernel, index, groups) = self.kernels.select(query_count, target_count);
        let mut encoder = self.device.create_command_encoder(&Default::default());
        kernel.record_dispatch(&mut encoder, &b.binds[index], groups);
        let bytes = query_count as u64 * 4;
        let mut indices = gpu_compute::ByteReadback::copy_buffer(
            &self.device,
            &mut encoder,
            &b.out_index,
            0,
            bytes,
        )?;
        let mut distances = gpu_compute::ByteReadback::copy_buffer(
            &self.device,
            &mut encoder,
            &b.out_dist,
            0,
            bytes,
        )?;
        let submission = self.queue.submit([encoder.finish()]);
        indices.submitted(submission.clone());
        distances.submitted(submission);
        let timeout = std::time::Duration::from_secs(30);
        indices.wait_with(timeout, |raw_index| {
            distances.wait_with(timeout, |raw_dist| {
                wire::zip_nearest_one_bytes(raw_index, raw_dist)
            })
        })?
    }
}

pub(super) struct GpuChamfer {
    device: Device,
    queue: wgpu::Queue,
    kernel: Kernel,
    buffers: std::cell::RefCell<Option<ChamferBuffers>>,
}

struct ChamferBuffers {
    query_capacity: usize,
    target_capacity: usize,
    partial_capacity: usize,
    params: Buffer,
    queries: Buffer,
    targets: Buffer,
    out_sum: Buffer,
    out_max: Buffer,
    bind: BindGroup,
}

impl GpuChamfer {
    pub(super) fn new(context: &GpuContext) -> Result<Self, compute_core::KernelError> {
        let kernel = Kernel::tuned(
            context,
            "chamfer",
            crate::CHAMFER_WGSL,
            "main",
            &NN_BINDINGS,
            WG_METAL,
            WG_DEFAULT,
        )?;
        Ok(Self {
            device: context.device.clone(),
            queue: context.queue.clone(),
            kernel,
            buffers: std::cell::RefCell::new(None),
        })
    }

    fn alloc_buffers(
        &self,
        query_count: usize,
        target_count: usize,
        partial_count: usize,
    ) -> ChamferBuffers {
        let device = &self.device;
        let query_capacity = query_count.max(1);
        let target_capacity = target_count.max(1);
        let partial_capacity = partial_count.max(1);
        let params = mk(
            device,
            "chamfer_params",
            16,
            BufferUsages::UNIFORM | BufferUsages::COPY_DST,
        );
        let queries = mk(
            device,
            "chamfer_queries",
            (query_capacity * 12) as u64,
            BufferUsages::STORAGE | BufferUsages::COPY_DST,
        );
        let targets = mk(
            device,
            "chamfer_targets",
            (target_capacity * 12) as u64,
            BufferUsages::STORAGE | BufferUsages::COPY_DST,
        );
        let partial_bytes = (partial_capacity * 4) as u64;
        let out_sum = mk(
            device,
            "chamfer_out_sum",
            partial_bytes,
            BufferUsages::STORAGE | BufferUsages::COPY_SRC,
        );
        let out_max = mk(
            device,
            "chamfer_out_max",
            partial_bytes,
            BufferUsages::STORAGE | BufferUsages::COPY_SRC,
        );
        let bind = self
            .kernel
            .create_bind_group(device, &[&params, &queries, &targets, &out_sum, &out_max]);
        ChamferBuffers {
            query_capacity,
            target_capacity,
            partial_capacity,
            params,
            queries,
            targets,
            out_sum,
            out_max,
            bind,
        }
    }

    pub(super) fn run(
        &self,
        queries: &[V3],
        targets: &[V3],
    ) -> Result<crate::DirectedChamfer, gpu_compute::ReadbackError> {
        let query_count = queries.len();
        let target_count = targets.len();
        let partial_count = self.kernel.workgroup_count(query_count as u32) as usize;
        let mut slot = self.buffers.borrow_mut();
        if slot.as_ref().is_some_and(|b| {
            b.query_capacity < query_count
                || b.target_capacity < target_count
                || b.partial_capacity < partial_count
        }) {
            *slot = None;
        }
        let buffers = slot
            .get_or_insert_with(|| self.alloc_buffers(query_count, target_count, partial_count));
        let params = pack_u32(&[query_count as u32, target_count as u32, 0, 0]);
        self.queue.write_buffer(&buffers.params, 0, &params);
        write_points(&self.queue, &buffers.queries, queries)?;
        write_points(&self.queue, &buffers.targets, targets)?;
        self.kernel.dispatch_bind_group(
            &self.device,
            &self.queue,
            &buffers.bind,
            partial_count as u32,
        );
        let raw_sum = read_f32(&self.device, &self.queue, &buffers.out_sum, partial_count)?;
        let raw_max = read_f32(&self.device, &self.queue, &buffers.out_max, partial_count)?;
        Ok(wire::fold_chamfer(&raw_sum, &raw_max, query_count))
    }
}

pub(super) struct GpuNearestTwo {
    device: Device,
    queue: wgpu::Queue,
    kernel: Kernel,
    buffers: std::cell::RefCell<Option<NearestTwoBuffers>>,
}

struct NearestTwoBuffers {
    query_capacity: usize,
    target_capacity: usize,
    params: Buffer,
    queries: Buffer,
    targets: Buffer,
    out_i0: Buffer,
    out_d0: Buffer,
    out_i1: Buffer,
    out_d1: Buffer,
    bind: BindGroup,
}

impl GpuNearestTwo {
    pub(super) fn new(context: &GpuContext) -> Result<Self, compute_core::KernelError> {
        let kernel = Kernel::tuned(
            context,
            "nearest_two",
            crate::NEAREST_TWO_WGSL,
            "main",
            &[
                Binding::Uniform,
                Binding::StorageRead,
                Binding::StorageRead,
                Binding::StorageReadWrite,
                Binding::StorageReadWrite,
                Binding::StorageReadWrite,
                Binding::StorageReadWrite,
            ],
            WG_METAL,
            WG_DEFAULT,
        )?;
        Ok(Self {
            device: context.device.clone(),
            queue: context.queue.clone(),
            kernel,
            buffers: std::cell::RefCell::new(None),
        })
    }

    fn alloc_buffers(&self, query_count: usize, target_count: usize) -> NearestTwoBuffers {
        let device = &self.device;
        let query_capacity = query_count.max(1);
        let target_capacity = target_count.max(1);
        let params = mk(
            device,
            "nearest_two_params",
            16,
            BufferUsages::UNIFORM | BufferUsages::COPY_DST,
        );
        let queries = mk(
            device,
            "nearest_two_queries",
            (query_capacity * 12) as u64,
            BufferUsages::STORAGE | BufferUsages::COPY_DST,
        );
        let targets = mk(
            device,
            "nearest_two_targets",
            (target_capacity * 12) as u64,
            BufferUsages::STORAGE | BufferUsages::COPY_DST,
        );
        let out_bytes = (query_capacity * 4) as u64;
        let outs: [Buffer; 4] = std::array::from_fn(|k| {
            mk(
                device,
                [
                    "nearest_two_i0",
                    "nearest_two_d0",
                    "nearest_two_i1",
                    "nearest_two_d1",
                ][k],
                out_bytes,
                BufferUsages::STORAGE | BufferUsages::COPY_SRC,
            )
        });
        let bind = self.kernel.create_bind_group(
            device,
            &[
                &params, &queries, &targets, &outs[0], &outs[1], &outs[2], &outs[3],
            ],
        );
        NearestTwoBuffers {
            query_capacity,
            target_capacity,
            params,
            queries,
            targets,
            out_i0: outs[0].clone(),
            out_d0: outs[1].clone(),
            out_i1: outs[2].clone(),
            out_d1: outs[3].clone(),
            bind,
        }
    }

    pub(super) fn run(
        &self,
        queries: &[V3],
        targets: &[V3],
    ) -> Result<Vec<crate::TwoNearest>, gpu_compute::ReadbackError> {
        let query_count = queries.len();
        let target_count = targets.len();
        let mut slot = self.buffers.borrow_mut();
        if slot
            .as_ref()
            .is_some_and(|b| b.query_capacity < query_count || b.target_capacity < target_count)
        {
            *slot = None;
        }
        let b = slot.get_or_insert_with(|| self.alloc_buffers(query_count, target_count));
        let params = pack_u32(&[query_count as u32, target_count as u32, 0, 0]);
        self.queue.write_buffer(&b.params, 0, &params);
        write_points(&self.queue, &b.queries, queries)?;
        write_points(&self.queue, &b.targets, targets)?;
        self.kernel.dispatch_bind_group(
            &self.device,
            &self.queue,
            &b.bind,
            self.kernel.workgroup_count(query_count.max(1) as u32),
        );
        let i0 = read_u32(&self.device, &self.queue, &b.out_i0, query_count)?;
        let d0 = read_f32(&self.device, &self.queue, &b.out_d0, query_count)?;
        let i1 = read_u32(&self.device, &self.queue, &b.out_i1, query_count)?;
        let d1 = read_f32(&self.device, &self.queue, &b.out_d1, query_count)?;
        Ok(wire::zip_nearest_two(&i0, &d0, &i1, &d1))
    }
}

pub(super) struct GpuNearestFour {
    device: Device,
    queue: wgpu::Queue,
    kernel: Kernel,
    buffers: std::cell::RefCell<Option<NearestFourBuffers>>,
}

struct NearestFourBuffers {
    query_capacity: usize,
    target_capacity: usize,
    params: Buffer,
    queries: Buffer,
    targets: Buffer,
    out_indices: Buffer,
    out_distances: Buffer,
    bind: BindGroup,
}

impl GpuNearestFour {
    pub(super) fn new(context: &GpuContext) -> Result<Self, compute_core::KernelError> {
        let kernel = Kernel::tuned(
            context,
            "nearest_four",
            crate::NEAREST_FOUR_WGSL,
            "main",
            &NN_BINDINGS,
            WG_METAL,
            WG_DEFAULT,
        )?;
        Ok(Self {
            device: context.device.clone(),
            queue: context.queue.clone(),
            kernel,
            buffers: std::cell::RefCell::new(None),
        })
    }

    fn alloc_buffers(&self, query_count: usize, target_count: usize) -> NearestFourBuffers {
        let device = &self.device;
        let query_capacity = query_count.max(1);
        let target_capacity = target_count.max(1);
        let params = mk(
            device,
            "nearest_four_params",
            16,
            BufferUsages::UNIFORM | BufferUsages::COPY_DST,
        );
        let queries = mk(
            device,
            "nearest_four_queries",
            (query_capacity * 12) as u64,
            BufferUsages::STORAGE | BufferUsages::COPY_DST,
        );
        let targets = mk(
            device,
            "nearest_four_targets",
            (target_capacity * 12) as u64,
            BufferUsages::STORAGE | BufferUsages::COPY_DST,
        );
        let out_bytes = (query_capacity * 4 * 4) as u64;
        let out_indices = mk(
            device,
            "nearest_four_indices",
            out_bytes,
            BufferUsages::STORAGE | BufferUsages::COPY_SRC,
        );
        let out_distances = mk(
            device,
            "nearest_four_distances",
            out_bytes,
            BufferUsages::STORAGE | BufferUsages::COPY_SRC,
        );
        let bind = self.kernel.create_bind_group(
            device,
            &[&params, &queries, &targets, &out_indices, &out_distances],
        );
        NearestFourBuffers {
            query_capacity,
            target_capacity,
            params,
            queries,
            targets,
            out_indices,
            out_distances,
            bind,
        }
    }

    pub(super) fn run(
        &self,
        queries: &[V3],
        targets: &[V3],
    ) -> Result<Vec<crate::FourNearest>, gpu_compute::ReadbackError> {
        let query_count = queries.len();
        let target_count = targets.len();
        let mut slot = self.buffers.borrow_mut();
        if slot
            .as_ref()
            .is_some_and(|b| b.query_capacity < query_count || b.target_capacity < target_count)
        {
            *slot = None;
        }
        let b = slot.get_or_insert_with(|| self.alloc_buffers(query_count, target_count));
        let params = pack_u32(&[query_count as u32, target_count as u32, 0, 0]);
        self.queue.write_buffer(&b.params, 0, &params);
        write_points(&self.queue, &b.queries, queries)?;
        write_points(&self.queue, &b.targets, targets)?;
        self.kernel.dispatch_bind_group(
            &self.device,
            &self.queue,
            &b.bind,
            self.kernel.workgroup_count(query_count.max(1) as u32),
        );
        let indices = read_u32(&self.device, &self.queue, &b.out_indices, query_count * 4)?;
        let distances = read_f32(&self.device, &self.queue, &b.out_distances, query_count * 4)?;
        Ok(wire::zip_nearest_four(&indices, &distances, query_count))
    }
}
