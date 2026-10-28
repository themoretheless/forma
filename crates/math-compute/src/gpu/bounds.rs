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
        *self.buffers.borrow_mut() = Some(PointBoundsBuffers {
            point_capacity,
            partial_capacity,
            params,
            points,
            out_min,
            out_max,
            bind,
        });
    }

    pub(super) fn run(
        &self,
        points: &[V3],
    ) -> Result<crate::PointBounds, gpu_compute::ReadbackError> {
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
            let mut min = [f64::INFINITY; 3];
            let mut max = [f64::NEG_INFINITY; 3];
            for chunk in raw_min.as_chunks::<3>().0.iter().take(partial_count) {
                for axis in 0..3 {
                    min[axis] = min[axis].min(chunk[axis] as f64);
                }
            }
            for chunk in raw_max.as_chunks::<3>().0.iter().take(partial_count) {
                for axis in 0..3 {
                    max[axis] = max[axis].max(chunk[axis] as f64);
                }
            }
            crate::PointBounds {
                samples: point_count,
                min,
                max,
                center: [
                    0.5 * (min[0] + max[0]),
                    0.5 * (min[1] + max[1]),
                    0.5 * (min[2] + max[2]),
                ],
                extent: [max[0] - min[0], max[1] - min[1], max[2] - min[2]],
            }
        })
    }
}

pub(super) struct GpuTransformedPointBounds {
    device: Device,
    queue: wgpu::Queue,
    kernel: Kernel,
    buffers: std::cell::RefCell<Option<TransformedPointBoundsBuffers>>,
}

struct TransformedPointBoundsBuffers {
    point_capacity: usize,
    partial_capacity: usize,
    params: Buffer,
    points: Buffer,
    out_min: Buffer,
    out_max: Buffer,
    bind: BindGroup,
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
        *self.buffers.borrow_mut() = Some(TransformedPointBoundsBuffers {
            point_capacity,
            partial_capacity,
            params,
            points,
            out_min,
            out_max,
            bind,
        });
    }

    pub(super) fn run(
        &self,
        points: &[V3],
        m: M3,
        t: V3,
    ) -> Result<crate::PointBounds, gpu_compute::ReadbackError> {
        Ok({
            let point_count = points.len();
            let partial_count = self.kernel.workgroup_count(point_count as u32) as usize;
            self.ensure_buffers(point_count, partial_count);
            let flat: Vec<f32> = points.iter().flatten().map(|&v| v as f32).collect();
            let mut params = pack_u32(&[point_count as u32, 0, 0, 0]);
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
            let mut min = [f64::INFINITY; 3];
            let mut max = [f64::NEG_INFINITY; 3];
            for chunk in raw_min.as_chunks::<3>().0.iter().take(partial_count) {
                for axis in 0..3 {
                    min[axis] = min[axis].min(chunk[axis] as f64);
                }
            }
            for chunk in raw_max.as_chunks::<3>().0.iter().take(partial_count) {
                for axis in 0..3 {
                    max[axis] = max[axis].max(chunk[axis] as f64);
                }
            }
            crate::PointBounds::new(point_count, min, max)
        })
    }
}
