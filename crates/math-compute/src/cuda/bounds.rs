use super::*;

struct CudaPointBounds {
    device: CudaDevice,
    kernel: CudaFunction,
    buffers: std::cell::RefCell<Option<PointBoundsBuffers>>,
}

struct PointBoundsBuffers {
    point_capacity: usize,
    partial_capacity: usize,
    points: CudaSlice<f32>,
    out_min: CudaSlice<f32>,
    out_max: CudaSlice<f32>,
}

impl CudaPointBounds {
    fn new() -> Option<Self> {
        let device = CudaDevice::new()?;
        let module = device.load_ptx(POINT_BOUNDS_PTX)?;
        let kernel = module.load_function("point_bounds_reduce").ok()?;
        Some(Self {
            device,
            kernel,
            buffers: std::cell::RefCell::new(None),
        })
    }

    fn ensure_buffers(&self, point_count: usize, partial_count: usize) -> Option<()> {
        let stale = match &*self.buffers.borrow() {
            Some(b) => b.point_capacity < point_count || b.partial_capacity < partial_count,
            None => true,
        };
        if !stale {
            return Some(());
        }
        let point_capacity = point_count.max(1);
        let partial_capacity = partial_count.max(1);
        let stream = &self.device.stream;
        let points = stream.alloc_zeros::<f32>(point_capacity * 3).ok()?;
        let out_min = stream.alloc_zeros::<f32>(partial_capacity * 3).ok()?;
        let out_max = stream.alloc_zeros::<f32>(partial_capacity * 3).ok()?;
        *self.buffers.borrow_mut() = Some(PointBoundsBuffers {
            point_capacity,
            partial_capacity,
            points,
            out_min,
            out_max,
        });
        Some(())
    }

    fn run(&self, points: &[V3]) -> Option<crate::PointBounds> {
        if points.is_empty() {
            return None;
        }
        let point_count = points.len();
        let partial_count = point_count.div_ceil(BLOCK as usize).max(1);
        let stream = &self.device.stream;
        let flat: Vec<f32> = points.iter().flatten().map(|&v| v as f32).collect();
        self.ensure_buffers(point_count, partial_count)?;
        let mut buffers = self.buffers.borrow_mut();
        let buffers = buffers.as_mut().expect("ensure_buffers was just called");
        {
            let mut points_view = buffers.points.slice_mut(0..point_count * 3);
            stream.memcpy_htod(&flat, &mut points_view).ok()?;
        }
        let point_count_u32 = point_count as u32;
        let points_view = buffers.points.slice(0..point_count * 3);
        let mut min_view = buffers.out_min.slice_mut(0..partial_count * 3);
        let mut max_view = buffers.out_max.slice_mut(0..partial_count * 3);
        let mut launch = stream.launch_builder(&self.kernel);
        launch
            .arg(&point_count_u32)
            .arg(&points_view)
            .arg(&mut min_view)
            .arg(&mut max_view);
        unsafe { launch.launch(launch_1d(point_count_u32, BLOCK)) }.ok()?;
        let mut mins = vec![0f32; partial_count * 3];
        let mut maxes = vec![0f32; partial_count * 3];
        stream.memcpy_dtoh(&min_view, &mut mins).ok()?;
        stream.memcpy_dtoh(&max_view, &mut maxes).ok()?;
        let mut min = [f64::INFINITY; 3];
        let mut max = [f64::NEG_INFINITY; 3];
        for chunk in mins.as_chunks::<3>().0 {
            for axis in 0..3 {
                min[axis] = min[axis].min(chunk[axis] as f64);
            }
        }
        for chunk in maxes.as_chunks::<3>().0 {
            for axis in 0..3 {
                max[axis] = max[axis].max(chunk[axis] as f64);
            }
        }
        Some(crate::PointBounds {
            samples: point_count,
            min,
            max,
            center: [
                0.5 * (min[0] + max[0]),
                0.5 * (min[1] + max[1]),
                0.5 * (min[2] + max[2]),
            ],
            extent: [max[0] - min[0], max[1] - min[1], max[2] - min[2]],
        })
    }
}

thread_local! {
    static SHARED_POINT_BOUNDS: std::cell::LazyCell<Option<&'static CudaPointBounds>> =
        std::cell::LazyCell::new(|| {
            CudaPointBounds::new().map(|kernel| Box::leak(Box::new(kernel)) as &'static CudaPointBounds)
        });
}

/// Point-cloud axis-aligned bounds on the CUDA device; `None` without a device.
pub(crate) fn point_bounds_cuda(points: &[V3]) -> Option<crate::PointBounds> {
    SHARED_POINT_BOUNDS.with(|cell| {
        let shared: &Option<&CudaPointBounds> = cell;
        shared.and_then(|kernel| kernel.run(points))
    })
}

struct CudaTransformedPointBounds {
    device: CudaDevice,
    kernel: CudaFunction,
    buffers: std::cell::RefCell<Option<TransformedPointBoundsBuffers>>,
}

struct TransformedPointBoundsBuffers {
    point_capacity: usize,
    partial_capacity: usize,
    points: CudaSlice<f32>,
    transform: CudaSlice<f32>,
    out_min: CudaSlice<f32>,
    out_max: CudaSlice<f32>,
}

impl CudaTransformedPointBounds {
    fn new() -> Option<Self> {
        let device = CudaDevice::new()?;
        let module = device.load_ptx(TRANSFORMED_POINT_BOUNDS_PTX)?;
        let kernel = module
            .load_function("transformed_point_bounds_reduce")
            .ok()?;
        Some(Self {
            device,
            kernel,
            buffers: std::cell::RefCell::new(None),
        })
    }

    fn ensure_buffers(&self, point_count: usize, partial_count: usize) -> Option<()> {
        let stale = match &*self.buffers.borrow() {
            Some(b) => b.point_capacity < point_count || b.partial_capacity < partial_count,
            None => true,
        };
        if !stale {
            return Some(());
        }
        let point_capacity = point_count.max(1);
        let partial_capacity = partial_count.max(1);
        let stream = &self.device.stream;
        let points = stream.alloc_zeros::<f32>(point_capacity * 3).ok()?;
        let transform = stream.alloc_zeros::<f32>(12).ok()?;
        let out_min = stream.alloc_zeros::<f32>(partial_capacity * 3).ok()?;
        let out_max = stream.alloc_zeros::<f32>(partial_capacity * 3).ok()?;
        *self.buffers.borrow_mut() = Some(TransformedPointBoundsBuffers {
            point_capacity,
            partial_capacity,
            points,
            transform,
            out_min,
            out_max,
        });
        Some(())
    }

    fn run(&self, points: &[V3], m: M3, t: V3) -> Option<crate::PointBounds> {
        if points.is_empty() {
            return None;
        }
        let point_count = points.len();
        let partial_count = point_count.div_ceil(BLOCK as usize).max(1);
        let stream = &self.device.stream;
        let flat: Vec<f32> = points.iter().flatten().map(|&v| v as f32).collect();
        let mut transform = Vec::with_capacity(12);
        for row in 0..3 {
            transform.extend(m[row].map(|v| v as f32));
            transform.push(t[row] as f32);
        }
        self.ensure_buffers(point_count, partial_count)?;
        let mut buffers = self.buffers.borrow_mut();
        let buffers = buffers.as_mut().expect("ensure_buffers was just called");
        {
            let mut points_view = buffers.points.slice_mut(0..point_count * 3);
            stream.memcpy_htod(&flat, &mut points_view).ok()?;
        }
        {
            let mut transform_view = buffers.transform.slice_mut(0..12);
            stream.memcpy_htod(&transform, &mut transform_view).ok()?;
        }
        let point_count_u32 = point_count as u32;
        let points_view = buffers.points.slice(0..point_count * 3);
        let transform_view = buffers.transform.slice(0..12);
        let mut min_view = buffers.out_min.slice_mut(0..partial_count * 3);
        let mut max_view = buffers.out_max.slice_mut(0..partial_count * 3);
        let mut launch = stream.launch_builder(&self.kernel);
        launch
            .arg(&point_count_u32)
            .arg(&points_view)
            .arg(&transform_view)
            .arg(&mut min_view)
            .arg(&mut max_view);
        unsafe { launch.launch(launch_1d(point_count_u32, BLOCK)) }.ok()?;
        let mut mins = vec![0f32; partial_count * 3];
        let mut maxes = vec![0f32; partial_count * 3];
        stream.memcpy_dtoh(&min_view, &mut mins).ok()?;
        stream.memcpy_dtoh(&max_view, &mut maxes).ok()?;
        let mut min = [f64::INFINITY; 3];
        let mut max = [f64::NEG_INFINITY; 3];
        for chunk in mins.as_chunks::<3>().0 {
            for axis in 0..3 {
                min[axis] = min[axis].min(chunk[axis] as f64);
            }
        }
        for chunk in maxes.as_chunks::<3>().0 {
            for axis in 0..3 {
                max[axis] = max[axis].max(chunk[axis] as f64);
            }
        }
        Some(crate::PointBounds::new(point_count, min, max))
    }
}

thread_local! {
    static SHARED_TRANSFORMED_POINT_BOUNDS: std::cell::LazyCell<Option<&'static CudaTransformedPointBounds>> =
        std::cell::LazyCell::new(|| {
            CudaTransformedPointBounds::new().map(|kernel| Box::leak(Box::new(kernel)) as &'static CudaTransformedPointBounds)
        });
}

/// Transformed point-cloud axis-aligned bounds on CUDA; `None` without a device.
pub(crate) fn transformed_point_bounds_cuda(
    points: &[V3],
    m: M3,
    t: V3,
) -> Option<crate::PointBounds> {
    SHARED_TRANSFORMED_POINT_BOUNDS.with(|cell| {
        let shared: &Option<&CudaTransformedPointBounds> = cell;
        shared.and_then(|kernel| kernel.run(points, m, t))
    })
}
