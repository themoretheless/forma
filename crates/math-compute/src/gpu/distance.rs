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

    fn ensure_buffers(&self, pair_count: usize) {
        let stale = match &*self.buffers.borrow() {
            Some(buffers) => buffers.capacity < pair_count,
            None => true,
        };
        if !stale {
            return;
        }
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
        *self.buffers.borrow_mut() = Some(DistancePairBuffers {
            capacity,
            params,
            a,
            b,
            out,
            bind,
        });
    }

    pub(super) fn run(&self, a: &[V3], b: &[V3]) -> Result<Vec<f64>, gpu_compute::ReadbackError> {
        Ok({
            let pair_count = a.len();
            self.ensure_buffers(pair_count);
            let flat_a: Vec<f32> = a.iter().flatten().map(|&v| v as f32).collect();
            let flat_b: Vec<f32> = b.iter().flatten().map(|&v| v as f32).collect();
            let params = pack_u32(&[pair_count as u32, 0, 0, 0]);
            let buffers = self.buffers.borrow();
            let buffers = buffers.as_ref().expect("ensure_buffers was just called");
            self.queue.write_buffer(&buffers.params, 0, &params);
            self.queue.write_buffer(&buffers.a, 0, &pack_f32(&flat_a));
            self.queue.write_buffer(&buffers.b, 0, &pack_f32(&flat_b));
            self.kernel.dispatch_bind_group(
                &self.device,
                &self.queue,
                &buffers.bind,
                self.kernel.workgroup_count(pair_count.max(1) as u32),
            );
            read_f32(&self.device, &self.queue, &buffers.out, pair_count)?
                .iter()
                .map(|&dist| dist as f64)
                .collect()
        })
    }
}

pub(super) struct GpuDistancePairSum {
    device: Device,
    queue: wgpu::Queue,
    kernel: Kernel,
    buffers: std::cell::RefCell<Option<DistancePairSumBuffers>>,
}

struct DistancePairSumBuffers {
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

    fn ensure_buffers(&self, pair_count: usize, partial_count: usize) {
        let stale = match &*self.buffers.borrow() {
            Some(b) => b.pair_capacity < pair_count || b.partial_capacity < partial_count,
            None => true,
        };
        if !stale {
            return;
        }
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
        *self.buffers.borrow_mut() = Some(DistancePairSumBuffers {
            pair_capacity,
            partial_capacity,
            params,
            a,
            b,
            out,
            bind,
        });
    }

    pub(super) fn run(&self, a: &[V3], b: &[V3]) -> Result<f64, gpu_compute::ReadbackError> {
        Ok({
            let pair_count = a.len();
            let partial_count = self.kernel.workgroup_count(pair_count.max(1) as u32) as usize;
            self.ensure_buffers(pair_count, partial_count);
            let flat_a: Vec<f32> = a.iter().flatten().map(|&v| v as f32).collect();
            let flat_b: Vec<f32> = b.iter().flatten().map(|&v| v as f32).collect();
            let params = pack_u32(&[pair_count as u32, 0, 0, 0]);
            let buffers = self.buffers.borrow();
            let buffers = buffers.as_ref().expect("ensure_buffers was just called");
            self.queue.write_buffer(&buffers.params, 0, &params);
            self.queue.write_buffer(&buffers.a, 0, &pack_f32(&flat_a));
            self.queue.write_buffer(&buffers.b, 0, &pack_f32(&flat_b));
            self.kernel.dispatch_bind_group(
                &self.device,
                &self.queue,
                &buffers.bind,
                partial_count as u32,
            );
            read_f32(&self.device, &self.queue, &buffers.out, partial_count)?
                .iter()
                .map(|&dist| dist as f64)
                .sum()
        })
    }
}

pub(super) struct GpuTransformedDistancePairSum {
    device: Device,
    queue: wgpu::Queue,
    kernel: Kernel,
    buffers: std::cell::RefCell<Option<TransformedDistancePairSumBuffers>>,
}

struct TransformedDistancePairSumBuffers {
    pair_capacity: usize,
    partial_capacity: usize,
    params: Buffer,
    source: Buffer,
    target: Buffer,
    out: Buffer,
    bind: BindGroup,
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

    fn ensure_buffers(&self, pair_count: usize, partial_count: usize) {
        let stale = match &*self.buffers.borrow() {
            Some(b) => b.pair_capacity < pair_count || b.partial_capacity < partial_count,
            None => true,
        };
        if !stale {
            return;
        }
        let device = &self.device;
        let pair_capacity = pair_count.max(1);
        let partial_capacity = partial_count.max(1);
        let params = mk(
            device,
            "transformed_distance_pair_sum_params",
            64,
            BufferUsages::UNIFORM | BufferUsages::COPY_DST,
        );
        let source = mk(
            device,
            "transformed_distance_pair_sum_source",
            (pair_capacity * 12) as u64,
            BufferUsages::STORAGE | BufferUsages::COPY_DST,
        );
        let target = mk(
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
            .create_bind_group(device, &[&params, &source, &target, &out]);
        *self.buffers.borrow_mut() = Some(TransformedDistancePairSumBuffers {
            pair_capacity,
            partial_capacity,
            params,
            source,
            target,
            out,
            bind,
        });
    }

    pub(super) fn run(
        &self,
        source: &[V3],
        target: &[V3],
        m: M3,
        t: V3,
    ) -> Result<f64, gpu_compute::ReadbackError> {
        Ok({
            let pair_count = source.len();
            let partial_count = self.kernel.workgroup_count(pair_count.max(1) as u32) as usize;
            self.ensure_buffers(pair_count, partial_count);
            let flat_source: Vec<f32> = source.iter().flatten().map(|&v| v as f32).collect();
            let flat_target: Vec<f32> = target.iter().flatten().map(|&v| v as f32).collect();
            // Params: count, pad x3, then 4 rows of (matrix row, translation comp).
            let mut params = pack_u32(&[pair_count as u32, 0, 0, 0]);
            for row in 0..3 {
                for &value in &m[row] {
                    push_f32(&mut params, value as f32);
                }
                push_f32(&mut params, t[row] as f32);
            }
            let buffers = self.buffers.borrow();
            let buffers = buffers.as_ref().expect("ensure_buffers was just called");
            self.queue.write_buffer(&buffers.params, 0, &params);
            self.queue
                .write_buffer(&buffers.source, 0, &pack_f32(&flat_source));
            self.queue
                .write_buffer(&buffers.target, 0, &pack_f32(&flat_target));
            self.kernel.dispatch_bind_group(
                &self.device,
                &self.queue,
                &buffers.bind,
                partial_count as u32,
            );
            read_f32(&self.device, &self.queue, &buffers.out, partial_count)?
                .iter()
                .map(|&dist| dist as f64)
                .sum()
        })
    }
}
