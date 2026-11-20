use super::support::*;

pub(super) struct GpuPointBounds {
    device: Device,
    queue: wgpu::Queue,
    kernel: Kernel,
    buffers: std::cell::RefCell<Option<PointBoundsBuffers>>,
}

struct PointBoundsBuffers {
    point_capacity: usize,
    partial_capacity: usize,
    params: Buffer,
    points: Buffer,
    out_min: Buffer,
    out_max: Buffer,
    bind: BindGroup,
}

impl GpuPointBounds {
    pub(super) fn new(context: &GpuContext) -> Result<Self, compute_core::KernelError> {
        let kernel = Kernel::tuned(
            context,
            "point_bounds",
            crate::POINT_BOUNDS_WGSL,
            "main",
            &UNIFORM_STORAGE2,
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

    fn alloc_buffers(&self, point_count: usize, partial_count: usize) -> PointBoundsBuffers {
        let device = &self.device;
        let point_capacity = point_count.max(1);
        let partial_capacity = partial_count.max(1);
        let params = mk(
            device,
            "point_bounds_params",
            16,
            BufferUsages::UNIFORM | BufferUsages::COPY_DST,
        );
        let points = mk(
            device,
            "point_bounds_points",
            (point_capacity * 12) as u64,
            BufferUsages::STORAGE | BufferUsages::COPY_DST,
        );
        let partial_bytes = (partial_capacity * 12) as u64;
        let out_min = mk(
            device,
            "point_bounds_out_min",
            partial_bytes,
            BufferUsages::STORAGE | BufferUsages::COPY_SRC,
        );
        let out_max = mk(
            device,
            "point_bounds_out_max",
            partial_bytes,
            BufferUsages::STORAGE | BufferUsages::COPY_SRC,
        );
        let bind = self
            .kernel
            .create_bind_group(device, &[&params, &points, &out_min, &out_max]);
        PointBoundsBuffers {
            point_capacity,
            partial_capacity,
            params,
            points,
            out_min,
            out_max,
            bind,
        }
    }

    pub(super) fn run(
        &self,
        points: &[V3],
    ) -> Result<crate::PointBounds, gpu_compute::ReadbackError> {
        let point_count = points.len();
        let partial_count = self.kernel.workgroup_count(point_count as u32) as usize;
        let flat = wire::pack_points_f32(points);
        let params = pack_u32(&[point_count as u32, 0, 0, 0]);
        let mut slot = self.buffers.borrow_mut();
        if slot
            .as_ref()
            .is_some_and(|b| b.point_capacity < point_count || b.partial_capacity < partial_count)
        {
            *slot = None;
        }
        let buffers = slot.get_or_insert_with(|| self.alloc_buffers(point_count, partial_count));
        self.queue.write_buffer(&buffers.params, 0, &params);
        self.queue
            .write_buffer(&buffers.points, 0, &pack_f32(&flat));
        self.kernel.dispatch_bind_group(
            &self.device,
            &self.queue,
            &buffers.bind,
            partial_count as u32,
        );
        let raw_min = read_f32(
            &self.device,
            &self.queue,
            &buffers.out_min,
            partial_count * 3,
        )?;
        let raw_max = read_f32(
            &self.device,
            &self.queue,
            &buffers.out_max,
            partial_count * 3,
        )?;
        Ok(wire::fold_point_bounds(
            point_count,
            &raw_min,
            &raw_max,
            partial_count,
        ))
    }
}

pub(super) struct GpuTransformedPointBounds {
    device: Device,
    queue: wgpu::Queue,
    kernel: Kernel,
    buffers: std::cell::RefCell<Option<PointBoundsBuffers>>,
}

impl GpuTransformedPointBounds {
    pub(super) fn new(context: &GpuContext) -> Result<Self, compute_core::KernelError> {
        let kernel = Kernel::tuned(
            context,
            "transformed_point_bounds",
            crate::TRANSFORMED_POINT_BOUNDS_WGSL,
            "main",
            &UNIFORM_STORAGE2,
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

    fn alloc_buffers(&self, point_count: usize, partial_count: usize) -> PointBoundsBuffers {
        let device = &self.device;
        let point_capacity = point_count.max(1);
        let partial_capacity = partial_count.max(1);
        let params = mk(
            device,
            "transformed_point_bounds_params",
            64,
            BufferUsages::UNIFORM | BufferUsages::COPY_DST,
        );
        let points = mk(
            device,
            "transformed_point_bounds_points",
            (point_capacity * 12) as u64,
            BufferUsages::STORAGE | BufferUsages::COPY_DST,
        );
        let partial_bytes = (partial_capacity * 12) as u64;
        let out_min = mk(
            device,
            "transformed_point_bounds_out_min",
            partial_bytes,
            BufferUsages::STORAGE | BufferUsages::COPY_SRC,
        );
        let out_max = mk(
            device,
            "transformed_point_bounds_out_max",
            partial_bytes,
            BufferUsages::STORAGE | BufferUsages::COPY_SRC,
        );
        let bind = self
            .kernel
            .create_bind_group(device, &[&params, &points, &out_min, &out_max]);
        PointBoundsBuffers {
            point_capacity,
            partial_capacity,
            params,
            points,
            out_min,
            out_max,
            bind,
        }
    }

    pub(super) fn run(
        &self,
        points: &[V3],
        m: M3,
        t: V3,
    ) -> Result<crate::PointBounds, gpu_compute::ReadbackError> {
        let point_count = points.len();
        let partial_count = self.kernel.workgroup_count(point_count as u32) as usize;
        let flat = wire::pack_points_f32(points);
        let mut params = pack_u32(&[point_count as u32, 0, 0, 0]);
        params.extend_from_slice(&pack_f32(&wire::pack_transform_f32(m, t)));
        let mut slot = self.buffers.borrow_mut();
        if slot
            .as_ref()
            .is_some_and(|b| b.point_capacity < point_count || b.partial_capacity < partial_count)
        {
            *slot = None;
        }
        let buffers = slot.get_or_insert_with(|| self.alloc_buffers(point_count, partial_count));
        self.queue.write_buffer(&buffers.params, 0, &params);
        self.queue
            .write_buffer(&buffers.points, 0, &pack_f32(&flat));
        self.kernel.dispatch_bind_group(
            &self.device,
            &self.queue,
            &buffers.bind,
            partial_count as u32,
        );
        let raw_min = read_f32(
            &self.device,
            &self.queue,
            &buffers.out_min,
            partial_count * 3,
        )?;
        let raw_max = read_f32(
            &self.device,
            &self.queue,
            &buffers.out_max,
            partial_count * 3,
        )?;
        Ok(wire::fold_point_bounds(
            point_count,
            &raw_min,
            &raw_max,
            partial_count,
        ))
    }
}
