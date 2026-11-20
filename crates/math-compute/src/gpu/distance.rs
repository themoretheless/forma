use super::support::*;

pub(super) struct GpuDistancePairs {
    device: Device,
    queue: wgpu::Queue,
    kernel: Kernel,
    buffers: std::cell::RefCell<Option<DistancePairBuffers>>,
}

struct DistancePairBuffers {
    capacity: usize,
    params: Buffer,
    a: Buffer,
    b: Buffer,
    out: Buffer,
    bind: BindGroup,
}

impl GpuDistancePairs {
    pub(super) fn new(context: &GpuContext) -> Result<Self, compute_core::KernelError> {
        let kernel = Kernel::tuned(
            context,
            "distance_pairs",
            crate::DISTANCE_PAIRS_WGSL,
            "main",
            &UNIFORM_PAIR,
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

    fn alloc_buffers(&self, pair_count: usize) -> DistancePairBuffers {
        let device = &self.device;
        let capacity = pair_count.max(1);
        let params = mk(
            device,
            "distance_pairs_params",
            16,
            BufferUsages::UNIFORM | BufferUsages::COPY_DST,
        );
        let a = mk(
            device,
            "distance_pairs_a",
            (capacity * 12) as u64,
            BufferUsages::STORAGE | BufferUsages::COPY_DST,
        );
        let b = mk(
            device,
            "distance_pairs_b",
            (capacity * 12) as u64,
            BufferUsages::STORAGE | BufferUsages::COPY_DST,
        );
        let out = mk(
            device,
            "distance_pairs_out",
            (capacity * 4) as u64,
            BufferUsages::STORAGE | BufferUsages::COPY_SRC,
        );
        let bind = self
            .kernel
            .create_bind_group(device, &[&params, &a, &b, &out]);
        DistancePairBuffers {
            capacity,
            params,
            a,
            b,
            out,
            bind,
        }
    }

    pub(super) fn run(&self, a: &[V3], b: &[V3]) -> Result<Vec<f64>, gpu_compute::ReadbackError> {
        let pair_count = a.len();
        let flat_a = wire::pack_points_f32(a);
        let flat_b = wire::pack_points_f32(b);
        let params = pack_u32(&[pair_count as u32, 0, 0, 0]);
        let mut slot = self.buffers.borrow_mut();
        if slot.as_ref().is_some_and(|buf| buf.capacity < pair_count) {
            *slot = None;
        }
        let buffers = slot.get_or_insert_with(|| self.alloc_buffers(pair_count));
        self.queue.write_buffer(&buffers.params, 0, &params);
        self.queue.write_buffer(&buffers.a, 0, &pack_f32(&flat_a));
        self.queue.write_buffer(&buffers.b, 0, &pack_f32(&flat_b));
        self.kernel.dispatch_bind_group(
            &self.device,
            &self.queue,
            &buffers.bind,
            self.kernel.workgroup_count(pair_count.max(1) as u32),
        );
        Ok(wire::fold_f64_vec(&read_f32(
            &self.device,
            &self.queue,
            &buffers.out,
            pair_count,
        )?))
    }
}

pub(super) struct GpuDistancePairSum {
    device: Device,
    queue: wgpu::Queue,
    kernel: Kernel,
    buffers: std::cell::RefCell<Option<DistanceSumBuffers>>,
}

struct DistanceSumBuffers {
    pair_capacity: usize,
    partial_capacity: usize,
    params: Buffer,
    a: Buffer,
    b: Buffer,
    out: Buffer,
    bind: BindGroup,
}

impl GpuDistancePairSum {
    pub(super) fn new(context: &GpuContext) -> Result<Self, compute_core::KernelError> {
        let kernel = Kernel::tuned(
            context,
            "distance_pair_sum",
            crate::DISTANCE_PAIR_SUM_WGSL,
            "main",
            &UNIFORM_PAIR,
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

    fn alloc_buffers(&self, pair_count: usize, partial_count: usize) -> DistanceSumBuffers {
        let device = &self.device;
        let pair_capacity = pair_count.max(1);
        let partial_capacity = partial_count.max(1);
        let params = mk(
            device,
            "distance_pair_sum_params",
            16,
            BufferUsages::UNIFORM | BufferUsages::COPY_DST,
        );
        let a = mk(
            device,
            "distance_pair_sum_a",
            (pair_capacity * 12) as u64,
            BufferUsages::STORAGE | BufferUsages::COPY_DST,
        );
        let b = mk(
            device,
            "distance_pair_sum_b",
            (pair_capacity * 12) as u64,
            BufferUsages::STORAGE | BufferUsages::COPY_DST,
        );
        let out = mk(
            device,
            "distance_pair_sum_out",
            (partial_capacity * 4) as u64,
            BufferUsages::STORAGE | BufferUsages::COPY_SRC,
        );
        let bind = self
            .kernel
            .create_bind_group(device, &[&params, &a, &b, &out]);
        DistanceSumBuffers {
            pair_capacity,
            partial_capacity,
            params,
            a,
            b,
            out,
            bind,
        }
    }

    pub(super) fn run(&self, a: &[V3], b: &[V3]) -> Result<f64, gpu_compute::ReadbackError> {
        let pair_count = a.len();
        let partial_count = self.kernel.workgroup_count(pair_count.max(1) as u32) as usize;
        let flat_a = wire::pack_points_f32(a);
        let flat_b = wire::pack_points_f32(b);
        let params = pack_u32(&[pair_count as u32, 0, 0, 0]);
        let mut slot = self.buffers.borrow_mut();
        if slot.as_ref().is_some_and(|buf| {
            buf.pair_capacity < pair_count || buf.partial_capacity < partial_count
        }) {
            *slot = None;
        }
        let buffers = slot.get_or_insert_with(|| self.alloc_buffers(pair_count, partial_count));
        self.queue.write_buffer(&buffers.params, 0, &params);
        self.queue.write_buffer(&buffers.a, 0, &pack_f32(&flat_a));
        self.queue.write_buffer(&buffers.b, 0, &pack_f32(&flat_b));
        self.kernel.dispatch_bind_group(
            &self.device,
            &self.queue,
            &buffers.bind,
            partial_count as u32,
        );
        Ok(wire::fold_f64_sum(&read_f32(
            &self.device,
            &self.queue,
            &buffers.out,
            partial_count,
        )?))
    }
}

pub(super) struct GpuTransformedDistancePairSum {
    device: Device,
    queue: wgpu::Queue,
    kernel: Kernel,
    buffers: std::cell::RefCell<Option<DistanceSumBuffers>>,
}

impl GpuTransformedDistancePairSum {
    pub(super) fn new(context: &GpuContext) -> Result<Self, compute_core::KernelError> {
        let kernel = Kernel::tuned(
            context,
            "transformed_distance_pair_sum",
            crate::TRANSFORMED_DISTANCE_PAIR_SUM_WGSL,
            "main",
            &UNIFORM_PAIR,
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

    fn alloc_buffers(&self, pair_count: usize, partial_count: usize) -> DistanceSumBuffers {
        let device = &self.device;
        let pair_capacity = pair_count.max(1);
        let partial_capacity = partial_count.max(1);
        let params = mk(
            device,
            "transformed_distance_pair_sum_params",
            64,
            BufferUsages::UNIFORM | BufferUsages::COPY_DST,
        );
        let a = mk(
            device,
            "transformed_distance_pair_sum_source",
            (pair_capacity * 12) as u64,
            BufferUsages::STORAGE | BufferUsages::COPY_DST,
        );
        let b = mk(
            device,
            "transformed_distance_pair_sum_target",
            (pair_capacity * 12) as u64,
            BufferUsages::STORAGE | BufferUsages::COPY_DST,
        );
        let out = mk(
            device,
            "transformed_distance_pair_sum_out",
            (partial_capacity * 4) as u64,
            BufferUsages::STORAGE | BufferUsages::COPY_SRC,
        );
        let bind = self
            .kernel
            .create_bind_group(device, &[&params, &a, &b, &out]);
        DistanceSumBuffers {
            pair_capacity,
            partial_capacity,
            params,
            a,
            b,
            out,
            bind,
        }
    }

    pub(super) fn run(
        &self,
        source: &[V3],
        target: &[V3],
        m: M3,
        t: V3,
    ) -> Result<f64, gpu_compute::ReadbackError> {
        let pair_count = source.len();
        let partial_count = self.kernel.workgroup_count(pair_count.max(1) as u32) as usize;
        let flat_source = wire::pack_points_f32(source);
        let flat_target = wire::pack_points_f32(target);
        // Params: count, pad x3, then 4 rows of (matrix row, translation comp).
        let mut params = pack_u32(&[pair_count as u32, 0, 0, 0]);
        params.extend_from_slice(&pack_f32(&wire::pack_transform_f32(m, t)));
        let mut slot = self.buffers.borrow_mut();
        if slot.as_ref().is_some_and(|buf| {
            buf.pair_capacity < pair_count || buf.partial_capacity < partial_count
        }) {
            *slot = None;
        }
        let buffers = slot.get_or_insert_with(|| self.alloc_buffers(pair_count, partial_count));
        self.queue.write_buffer(&buffers.params, 0, &params);
        self.queue
            .write_buffer(&buffers.a, 0, &pack_f32(&flat_source));
        self.queue
            .write_buffer(&buffers.b, 0, &pack_f32(&flat_target));
        self.kernel.dispatch_bind_group(
            &self.device,
            &self.queue,
            &buffers.bind,
            partial_count as u32,
        );
        Ok(wire::fold_f64_sum(&read_f32(
            &self.device,
            &self.queue,
            &buffers.out,
            partial_count,
        )?))
    }
}
