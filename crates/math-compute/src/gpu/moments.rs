use super::support::*;

pub(super) struct GpuPointMoments {
    device: Device,
    queue: wgpu::Queue,
    kernel: Kernel,
    buffers: std::cell::RefCell<Option<PointMomentsBuffers>>,
}

struct PointMomentsBuffers {
    point_capacity: usize,
    partial_capacity: usize,
    params: Buffer,
    points: Buffer,
    out: Buffer,
    bind: BindGroup,
}

impl GpuPointMoments {
    pub(super) fn new(context: &GpuContext) -> Result<Self, compute_core::KernelError> {
        let kernel = Kernel::tuned(
            context,
            "point_moments",
            crate::POINT_MOMENTS_WGSL,
            "main",
            &UNIFORM_REDUCE1,
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

    fn ensure_buffers(&self, point_count: usize, partial_count: usize) {
        let stale = match &*self.buffers.borrow() {
            Some(b) => b.point_capacity < point_count || b.partial_capacity < partial_count,
            None => true,
        };
        if !stale {
            return;
        }
        let device = &self.device;
        let point_capacity = point_count.max(1);
        let partial_capacity = partial_count.max(1);
        let params = mk(
            device,
            "point_moments_params",
            16,
            BufferUsages::UNIFORM | BufferUsages::COPY_DST,
        );
        let points = mk(
            device,
            "point_moments_points",
            (point_capacity * 12) as u64,
            BufferUsages::STORAGE | BufferUsages::COPY_DST,
        );
        let partial_bytes = (partial_capacity * 9 * 4) as u64;
        let out = mk(
            device,
            "point_moments_out",
            partial_bytes,
            BufferUsages::STORAGE | BufferUsages::COPY_SRC,
        );
        let bind = self
            .kernel
            .create_bind_group(device, &[&params, &points, &out]);
        *self.buffers.borrow_mut() = Some(PointMomentsBuffers {
            point_capacity,
            partial_capacity,
            params,
            points,
            out,
            bind,
        });
    }

    pub(super) fn run(
        &self,
        points: &[V3],
    ) -> Result<crate::PointMoments, gpu_compute::ReadbackError> {
        Ok({
            let point_count = points.len();
            let partial_count = self.kernel.workgroup_count(point_count as u32) as usize;
            self.ensure_buffers(point_count, partial_count);
            let flat: Vec<f32> = points.iter().flatten().map(|&v| v as f32).collect();
            let params = pack_u32(&[point_count as u32, 0, 0, 0]);
            let buffers = self.buffers.borrow();
            let buffers = buffers.as_ref().expect("ensure_buffers was just called");
            self.queue.write_buffer(&buffers.params, 0, &params);
            self.queue
                .write_buffer(&buffers.points, 0, &pack_f32(&flat));
            self.kernel.dispatch_bind_group(
                &self.device,
                &self.queue,
                &buffers.bind,
                partial_count as u32,
            );
            let raw = read_f32(&self.device, &self.queue, &buffers.out, partial_count * 9)?;
            let mut accum = [0.; 9];
            for chunk in raw.as_chunks::<9>().0.iter().take(partial_count) {
                for item in 0..9 {
                    accum[item] += chunk[item] as f64;
                }
            }
            crate::PointMoments::from_sums(
                point_count,
                [accum[0], accum[1], accum[2]],
                [accum[3], accum[4], accum[5], accum[6], accum[7], accum[8]],
            )
        })
    }
}

pub(super) struct GpuPointCloudStats {
    device: Device,
    queue: wgpu::Queue,
    kernel: Kernel,
    buffers: std::cell::RefCell<Option<PointCloudStatsBuffers>>,
}

struct PointCloudStatsBuffers {
    point_capacity: usize,
    partial_capacity: usize,
    params: Buffer,
    points: Buffer,
    out: Buffer,
    bind: BindGroup,
}

impl GpuPointCloudStats {
    pub(super) fn new(context: &GpuContext) -> Result<Self, compute_core::KernelError> {
        let kernel = Kernel::tuned(
            context,
            "point_cloud_stats",
            crate::POINT_CLOUD_STATS_WGSL,
            "main",
            &UNIFORM_REDUCE1,
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

    fn ensure_buffers(&self, point_count: usize, partial_count: usize) {
        let stale = match &*self.buffers.borrow() {
            Some(b) => b.point_capacity < point_count || b.partial_capacity < partial_count,
            None => true,
        };
        if !stale {
            return;
        }
        let device = &self.device;
        let point_capacity = point_count.max(1);
        let partial_capacity = partial_count.max(1);
        let params = mk(
            device,
            "point_cloud_stats_params",
            16,
            BufferUsages::UNIFORM | BufferUsages::COPY_DST,
        );
        let points = mk(
            device,
            "point_cloud_stats_points",
            (point_capacity * 12) as u64,
            BufferUsages::STORAGE | BufferUsages::COPY_DST,
        );
        let partial_bytes = (partial_capacity * 15 * 4) as u64;
        let out = mk(
            device,
            "point_cloud_stats_out",
            partial_bytes,
            BufferUsages::STORAGE | BufferUsages::COPY_SRC,
        );
        let bind = self
            .kernel
            .create_bind_group(device, &[&params, &points, &out]);
        *self.buffers.borrow_mut() = Some(PointCloudStatsBuffers {
            point_capacity,
            partial_capacity,
            params,
            points,
            out,
            bind,
        });
    }

    pub(super) fn run(
        &self,
        points: &[V3],
    ) -> Result<crate::PointCloudStats, gpu_compute::ReadbackError> {
        Ok({
            let point_count = points.len();
            let partial_count = self.kernel.workgroup_count(point_count as u32) as usize;
            self.ensure_buffers(point_count, partial_count);
            let flat: Vec<f32> = points.iter().flatten().map(|&v| v as f32).collect();
            let params = pack_u32(&[point_count as u32, 0, 0, 0]);
            let buffers = self.buffers.borrow();
            let buffers = buffers.as_ref().expect("ensure_buffers was just called");
            self.queue.write_buffer(&buffers.params, 0, &params);
            self.queue
                .write_buffer(&buffers.points, 0, &pack_f32(&flat));
            self.kernel.dispatch_bind_group(
                &self.device,
                &self.queue,
                &buffers.bind,
                partial_count as u32,
            );
            let raw = read_f32(&self.device, &self.queue, &buffers.out, partial_count * 15)?;
            let mut min = [f64::INFINITY; 3];
            let mut max = [f64::NEG_INFINITY; 3];
            let mut accum = [0.; 9];
            for chunk in raw.as_chunks::<15>().0.iter().take(partial_count) {
                for axis in 0..3 {
                    min[axis] = min[axis].min(chunk[axis] as f64);
                    max[axis] = max[axis].max(chunk[axis + 3] as f64);
                }
                for item in 0..9 {
                    accum[item] += chunk[item + 6] as f64;
                }
            }
            let bounds = crate::PointBounds::new(point_count, min, max);
            let moments = crate::PointMoments::from_sums(
                point_count,
                [accum[0], accum[1], accum[2]],
                [accum[3], accum[4], accum[5], accum[6], accum[7], accum[8]],
            );
            crate::PointCloudStats::from_parts(bounds, moments)
        })
    }
}
