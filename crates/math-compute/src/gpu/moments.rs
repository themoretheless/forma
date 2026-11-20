use super::support::*;

struct PointReduceBuffers {
    point_capacity: usize,
    partial_capacity: usize,
    params: Buffer,
    points: Buffer,
    out: Buffer,
    bind: BindGroup,
}

fn run_point_reduce(
    device: &Device,
    queue: &wgpu::Queue,
    kernel: &Kernel,
    cache: &std::cell::RefCell<Option<PointReduceBuffers>>,
    points: &[V3],
    label: &str,
    floats_per_partial: usize,
) -> Result<(usize, Vec<f32>), gpu_compute::ReadbackError> {
    let point_count = points.len();
    let partial_count = kernel.workgroup_count(point_count as u32) as usize;
    let flat = wire::pack_points_f32(points);
    let params = pack_u32(&[point_count as u32, 0, 0, 0]);
    let mut slot = cache.borrow_mut();
    if slot
        .as_ref()
        .is_some_and(|b| b.point_capacity < point_count || b.partial_capacity < partial_count)
    {
        *slot = None;
    }
    let buffers = slot.get_or_insert_with(|| {
        let point_capacity = point_count.max(1);
        let partial_capacity = partial_count.max(1);
        let params_buf = mk(
            device,
            label,
            16,
            BufferUsages::UNIFORM | BufferUsages::COPY_DST,
        );
        let points_buf = mk(
            device,
            label,
            (point_capacity * 12) as u64,
            BufferUsages::STORAGE | BufferUsages::COPY_DST,
        );
        let out_buf = mk(
            device,
            label,
            (partial_capacity * floats_per_partial * 4) as u64,
            BufferUsages::STORAGE | BufferUsages::COPY_SRC,
        );
        let bind = kernel.create_bind_group(device, &[&params_buf, &points_buf, &out_buf]);
        PointReduceBuffers {
            point_capacity,
            partial_capacity,
            params: params_buf,
            points: points_buf,
            out: out_buf,
            bind,
        }
    });
    queue.write_buffer(&buffers.params, 0, &params);
    queue.write_buffer(&buffers.points, 0, &pack_f32(&flat));
    kernel.dispatch_bind_group(device, queue, &buffers.bind, partial_count as u32);
    let raw = read_f32(
        device,
        queue,
        &buffers.out,
        partial_count * floats_per_partial,
    )?;
    Ok((partial_count, raw))
}

pub(super) struct GpuPointMoments {
    device: Device,
    queue: wgpu::Queue,
    kernel: Kernel,
    buffers: std::cell::RefCell<Option<PointReduceBuffers>>,
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

    pub(super) fn run(
        &self,
        points: &[V3],
    ) -> Result<crate::PointMoments, gpu_compute::ReadbackError> {
        let (partial_count, raw) = run_point_reduce(
            &self.device,
            &self.queue,
            &self.kernel,
            &self.buffers,
            points,
            "point_moments",
            9,
        )?;
        Ok(wire::fold_point_moments(points.len(), &raw, partial_count))
    }
}

pub(super) struct GpuPointCloudStats {
    device: Device,
    queue: wgpu::Queue,
    kernel: Kernel,
    buffers: std::cell::RefCell<Option<PointReduceBuffers>>,
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

    pub(super) fn run(
        &self,
        points: &[V3],
    ) -> Result<crate::PointCloudStats, gpu_compute::ReadbackError> {
        let (partial_count, raw) = run_point_reduce(
            &self.device,
            &self.queue,
            &self.kernel,
            &self.buffers,
            points,
            "point_cloud_stats",
            15,
        )?;
        Ok(wire::fold_point_cloud_stats(
            points.len(),
            &raw,
            partial_count,
        ))
    }
}
