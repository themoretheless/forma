use super::*;

struct PointReduceBuffers {
    point_capacity: usize,
    partial_capacity: usize,
    points: CudaSlice<f32>,
    out: CudaSlice<f32>,
}

fn run_point_reduce(
    session: &CudaSession,
    kernel: &CudaFunction,
    slot: &RefCell<Option<PointReduceBuffers>>,
    points: &[V3],
    floats_per_partial: usize,
) -> Option<(usize, Vec<f32>)> {
    if points.is_empty() {
        return None;
    }
    let point_count = points.len();
    let point_count_u32 = u32::try_from(point_count).ok()?;
    let partial_count = point_count.div_ceil(BLOCK as usize).max(1);
    let stream = &session.device.stream;
    let flat = wire::pack_points_f32(points);
    let mut cache = slot.borrow_mut();
    if cache
        .as_ref()
        .is_none_or(|b| b.point_capacity < point_count || b.partial_capacity < partial_count)
    {
        let pc = point_count.max(1);
        let rc = partial_count.max(1);
        *cache = Some(PointReduceBuffers {
            point_capacity: pc,
            partial_capacity: rc,
            points: stream.alloc_zeros::<f32>(pc * 3).ok()?,
            out: stream.alloc_zeros::<f32>(rc * floats_per_partial).ok()?,
        });
    }
    let buffers = cache.as_mut()?;
    {
        let mut points_view = buffers.points.slice_mut(0..point_count * 3);
        stream.memcpy_htod(&flat, &mut points_view).ok()?;
    }
    let points_view = buffers.points.slice(0..point_count * 3);
    let mut out_view = buffers.out.slice_mut(0..partial_count * floats_per_partial);
    let mut launch = stream.launch_builder(kernel);
    launch
        .arg(&point_count_u32)
        .arg(&points_view)
        .arg(&mut out_view);
    // SAFETY: Both `point_moments_reduce` (stride 9) and `point_cloud_stats_reduce` (stride 15)
    // accept `(u32, *const f32, *mut f32)`; `points_view` has `point_count * 3` elements and
    // `out_view` has `partial_count * floats_per_partial` elements on `stream`.
    unsafe { launch.launch(launch_1d(point_count_u32, BLOCK)) }.ok()?;
    let mut partials = vec![0f32; partial_count * floats_per_partial];
    stream.memcpy_dtoh(&out_view, &mut partials).ok()?;
    Some((partial_count, partials))
}

pub(super) struct PointMomentsKernel {
    kernel: CudaFunction,
    buffers: RefCell<Option<PointReduceBuffers>>,
}

impl PointMomentsKernel {
    fn new(session: &CudaSession) -> Option<Self> {
        Some(Self {
            kernel: session.load(POINT_MOMENTS_PTX, "point_moments_reduce")?,
            buffers: RefCell::new(None),
        })
    }
}

/// Point-cloud centroid/covariance reduction on the CUDA device; `None` without a device.
pub(crate) fn point_moments_cuda(points: &[V3]) -> Option<crate::PointMoments> {
    with_session(|session| {
        let state = session
            .point_moments
            .get_or_init(|| PointMomentsKernel::new(session))
            .as_ref()?;
        let (partial_count, partials) =
            run_point_reduce(session, &state.kernel, &state.buffers, points, 9)?;
        Some(wire::fold_point_moments(
            points.len(),
            &partials,
            partial_count,
        ))
    })
}

pub(super) struct PointCloudStatsKernel {
    kernel: CudaFunction,
    buffers: RefCell<Option<PointReduceBuffers>>,
}

impl PointCloudStatsKernel {
    fn new(session: &CudaSession) -> Option<Self> {
        Some(Self {
            kernel: session.load(POINT_CLOUD_STATS_PTX, "point_cloud_stats_reduce")?,
            buffers: RefCell::new(None),
        })
    }
}

/// Fused point-cloud bounds + moments reduction on the CUDA device; `None` without a device.
pub(crate) fn point_cloud_stats_cuda(points: &[V3]) -> Option<crate::PointCloudStats> {
    with_session(|session| {
        let state = session
            .point_cloud_stats
            .get_or_init(|| PointCloudStatsKernel::new(session))
            .as_ref()?;
        let (partial_count, partials) =
            run_point_reduce(session, &state.kernel, &state.buffers, points, 15)?;
        Some(wire::fold_point_cloud_stats(
            points.len(),
            &partials,
            partial_count,
        ))
    })
}
