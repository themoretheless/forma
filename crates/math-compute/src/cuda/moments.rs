use super::*;

struct CudaPointMoments {
    device: CudaDevice,
    kernel: CudaFunction,
    buffers: std::cell::RefCell<Option<PointMomentsBuffers>>,
}

struct PointMomentsBuffers {
    point_capacity: usize,
    partial_capacity: usize,
    points: CudaSlice<f32>,
    out: CudaSlice<f32>,
}

impl CudaPointMoments {
    fn new() -> Option<Self> {
        let device = CudaDevice::new()?;
        let module = device.load_ptx(POINT_MOMENTS_PTX)?;
        let kernel = module.load_function("point_moments_reduce").ok()?;
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
        let out = stream.alloc_zeros::<f32>(partial_capacity * 9).ok()?;
        *self.buffers.borrow_mut() = Some(PointMomentsBuffers {
            point_capacity,
            partial_capacity,
            points,
            out,
        });
        Some(())
    }

    fn run(&self, points: &[V3]) -> Option<crate::PointMoments> {
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
        let mut out_view = buffers.out.slice_mut(0..partial_count * 9);
        let mut launch = stream.launch_builder(&self.kernel);
        launch
            .arg(&point_count_u32)
            .arg(&points_view)
            .arg(&mut out_view);
        unsafe { launch.launch(launch_1d(point_count_u32, BLOCK)) }.ok()?;
        let mut partials = vec![0f32; partial_count * 9];
        stream.memcpy_dtoh(&out_view, &mut partials).ok()?;
        let mut accum = [0.; 9];
        for chunk in partials.as_chunks::<9>().0 {
            for item in 0..9 {
                accum[item] += chunk[item] as f64;
            }
        }
        Some(crate::PointMoments::from_sums(
            point_count,
            [accum[0], accum[1], accum[2]],
            [accum[3], accum[4], accum[5], accum[6], accum[7], accum[8]],
        ))
    }
}

thread_local! {
    static SHARED_POINT_MOMENTS: std::cell::LazyCell<Option<&'static CudaPointMoments>> =
        std::cell::LazyCell::new(|| {
            CudaPointMoments::new().map(|kernel| Box::leak(Box::new(kernel)) as &'static CudaPointMoments)
        });
}

/// Point-cloud centroid/covariance reduction on the CUDA device; `None` without a device.
pub(crate) fn point_moments_cuda(points: &[V3]) -> Option<crate::PointMoments> {
    SHARED_POINT_MOMENTS.with(|cell| {
        let shared: &Option<&CudaPointMoments> = cell;
        shared.and_then(|kernel| kernel.run(points))
    })
}

struct CudaPointCloudStats {
    device: CudaDevice,
    kernel: CudaFunction,
    buffers: std::cell::RefCell<Option<PointCloudStatsBuffers>>,
}

struct PointCloudStatsBuffers {
    point_capacity: usize,
    partial_capacity: usize,
    points: CudaSlice<f32>,
    out: CudaSlice<f32>,
}

impl CudaPointCloudStats {
    fn new() -> Option<Self> {
        let device = CudaDevice::new()?;
        let module = device.load_ptx(POINT_CLOUD_STATS_PTX)?;
        let kernel = module.load_function("point_cloud_stats_reduce").ok()?;
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
        let out = stream.alloc_zeros::<f32>(partial_capacity * 15).ok()?;
        *self.buffers.borrow_mut() = Some(PointCloudStatsBuffers {
            point_capacity,
            partial_capacity,
            points,
            out,
        });
        Some(())
    }

    fn run(&self, points: &[V3]) -> Option<crate::PointCloudStats> {
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
        let mut out_view = buffers.out.slice_mut(0..partial_count * 15);
        let mut launch = stream.launch_builder(&self.kernel);
        launch
            .arg(&point_count_u32)
            .arg(&points_view)
            .arg(&mut out_view);
        unsafe { launch.launch(launch_1d(point_count_u32, BLOCK)) }.ok()?;
        let mut partials = vec![0f32; partial_count * 15];
        stream.memcpy_dtoh(&out_view, &mut partials).ok()?;
        let mut min = [f64::INFINITY; 3];
        let mut max = [f64::NEG_INFINITY; 3];
        let mut accum = [0.; 9];
        for chunk in partials.as_chunks::<15>().0 {
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
        Some(crate::PointCloudStats::from_parts(bounds, moments))
    }
}

thread_local! {
    static SHARED_POINT_CLOUD_STATS: std::cell::LazyCell<Option<&'static CudaPointCloudStats>> =
        std::cell::LazyCell::new(|| {
            CudaPointCloudStats::new().map(|kernel| Box::leak(Box::new(kernel)) as &'static CudaPointCloudStats)
        });
}

/// Fused point-cloud bounds + moments reduction on the CUDA device; `None` without a device.
pub(crate) fn point_cloud_stats_cuda(points: &[V3]) -> Option<crate::PointCloudStats> {
    SHARED_POINT_CLOUD_STATS.with(|cell| {
        let shared: &Option<&CudaPointCloudStats> = cell;
        shared.and_then(|kernel| kernel.run(points))
    })
}
