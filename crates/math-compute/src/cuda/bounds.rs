use super::*;

pub(super) struct PointBoundsKernel {
    kernel: CudaFunction,
    buffers: RefCell<Option<PointBoundsBuffers>>,
}

struct PointBoundsBuffers {
    point_capacity: usize,
    partial_capacity: usize,
    points: CudaSlice<f32>,
    out_min: CudaSlice<f32>,
    out_max: CudaSlice<f32>,
}

impl PointBoundsKernel {
    fn new(session: &CudaSession) -> Option<Self> {
        Some(Self {
            kernel: session.load(POINT_BOUNDS_PTX, "point_bounds_reduce")?,
            buffers: RefCell::new(None),
        })
    }

    fn run(&self, session: &CudaSession, points: &[V3]) -> Option<crate::PointBounds> {
        if points.is_empty() {
            return None;
        }
        let point_count = points.len();
        let point_count_u32 = u32::try_from(point_count).ok()?;
        let partial_count = point_count.div_ceil(BLOCK as usize).max(1);
        let stream = &session.device.stream;
        let flat = wire::pack_points_f32(points);
        let mut slot = self.buffers.borrow_mut();
        if slot
            .as_ref()
            .is_none_or(|b| b.point_capacity < point_count || b.partial_capacity < partial_count)
        {
            let pc = point_count.max(1);
            let rc = partial_count.max(1);
            *slot = Some(PointBoundsBuffers {
                point_capacity: pc,
                partial_capacity: rc,
                points: stream.alloc_zeros::<f32>(pc * 3).ok()?,
                out_min: stream.alloc_zeros::<f32>(rc * 3).ok()?,
                out_max: stream.alloc_zeros::<f32>(rc * 3).ok()?,
            });
        }
        let buffers = slot.as_mut()?;
        {
            let mut points_view = buffers.points.slice_mut(0..point_count * 3);
            stream.memcpy_htod(&flat, &mut points_view).ok()?;
        }
        let points_view = buffers.points.slice(0..point_count * 3);
        let mut min_view = buffers.out_min.slice_mut(0..partial_count * 3);
        let mut max_view = buffers.out_max.slice_mut(0..partial_count * 3);
        let mut launch = stream.launch_builder(&self.kernel);
        launch
            .arg(&point_count_u32)
            .arg(&points_view)
            .arg(&mut min_view)
            .arg(&mut max_view);
        // SAFETY: `point_bounds_reduce` takes `(u32, *const f32, *mut f32, *mut f32)`;
        // `points_view` holds `point_count * 3` floats and `min_view`/`max_view` hold
        // `partial_count * 3` floats on the owning stream for the 1D grid covering `point_count_u32`.
        unsafe { launch.launch(launch_1d(point_count_u32, BLOCK)) }.ok()?;
        let mut mins = vec![0f32; partial_count * 3];
        let mut maxes = vec![0f32; partial_count * 3];
        stream.memcpy_dtoh(&min_view, &mut mins).ok()?;
        stream.memcpy_dtoh(&max_view, &mut maxes).ok()?;
        Some(wire::fold_point_bounds(
            point_count,
            &mins,
            &maxes,
            partial_count,
        ))
    }
}

/// Point-cloud axis-aligned bounds on the CUDA device; `None` without a device.
pub(crate) fn point_bounds_cuda(points: &[V3]) -> Option<crate::PointBounds> {
    with_session(|session| {
        session
            .point_bounds
            .get_or_init(|| PointBoundsKernel::new(session))
            .as_ref()?
            .run(session, points)
    })
}

pub(super) struct TransformedPointBoundsKernel {
    kernel: CudaFunction,
    buffers: RefCell<Option<TransformedPointBoundsBuffers>>,
}

struct TransformedPointBoundsBuffers {
    point_capacity: usize,
    partial_capacity: usize,
    points: CudaSlice<f32>,
    transform: CudaSlice<f32>,
    out_min: CudaSlice<f32>,
    out_max: CudaSlice<f32>,
}

impl TransformedPointBoundsKernel {
    fn new(session: &CudaSession) -> Option<Self> {
        Some(Self {
            kernel: session.load(
                TRANSFORMED_POINT_BOUNDS_PTX,
                "transformed_point_bounds_reduce",
            )?,
            buffers: RefCell::new(None),
        })
    }

    fn run(
        &self,
        session: &CudaSession,
        points: &[V3],
        m: M3,
        t: V3,
    ) -> Option<crate::PointBounds> {
        if points.is_empty() {
            return None;
        }
        let point_count = points.len();
        let point_count_u32 = u32::try_from(point_count).ok()?;
        let partial_count = point_count.div_ceil(BLOCK as usize).max(1);
        let stream = &session.device.stream;
        let flat = wire::pack_points_f32(points);
        let transform = wire::pack_transform_f32(m, t);
        let mut slot = self.buffers.borrow_mut();
        if slot
            .as_ref()
            .is_none_or(|b| b.point_capacity < point_count || b.partial_capacity < partial_count)
        {
            let pc = point_count.max(1);
            let rc = partial_count.max(1);
            *slot = Some(TransformedPointBoundsBuffers {
                point_capacity: pc,
                partial_capacity: rc,
                points: stream.alloc_zeros::<f32>(pc * 3).ok()?,
                transform: stream.alloc_zeros::<f32>(12).ok()?,
                out_min: stream.alloc_zeros::<f32>(rc * 3).ok()?,
                out_max: stream.alloc_zeros::<f32>(rc * 3).ok()?,
            });
        }
        let buffers = slot.as_mut()?;
        {
            let mut points_view = buffers.points.slice_mut(0..point_count * 3);
            stream.memcpy_htod(&flat, &mut points_view).ok()?;
            let mut transform_view = buffers.transform.slice_mut(0..12);
            stream.memcpy_htod(&transform, &mut transform_view).ok()?;
        }
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
        // SAFETY: `transformed_point_bounds_reduce` takes `(u32, *const f32, *const f32, *mut f32, *mut f32)`;
        // `points_view` (`point_count * 3`), `transform_view` (`12`), and `min_view`/`max_view` (`partial_count * 3`)
        // remain valid on the stream across the 1D launch covering `point_count_u32`.
        unsafe { launch.launch(launch_1d(point_count_u32, BLOCK)) }.ok()?;
        let mut mins = vec![0f32; partial_count * 3];
        let mut maxes = vec![0f32; partial_count * 3];
        stream.memcpy_dtoh(&min_view, &mut mins).ok()?;
        stream.memcpy_dtoh(&max_view, &mut maxes).ok()?;
        Some(wire::fold_point_bounds(
            point_count,
            &mins,
            &maxes,
            partial_count,
        ))
    }
}

/// Transformed point-cloud axis-aligned bounds on CUDA; `None` without a device.
pub(crate) fn transformed_point_bounds_cuda(
    points: &[V3],
    m: M3,
    t: V3,
) -> Option<crate::PointBounds> {
    with_session(|session| {
        session
            .transformed_point_bounds
            .get_or_init(|| TransformedPointBoundsKernel::new(session))
            .as_ref()?
            .run(session, points, m, t)
    })
}
