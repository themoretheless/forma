use super::*;

pub(super) struct DistancePairsKernel {
    kernel: CudaFunction,
    buffers: RefCell<Option<DistancePairBuffers>>,
}

struct DistancePairBuffers {
    capacity: usize,
    a: CudaSlice<f32>,
    b: CudaSlice<f32>,
    out: CudaSlice<f32>,
}

impl DistancePairsKernel {
    fn new(session: &CudaSession) -> Option<Self> {
        Some(Self {
            kernel: session.load(DISTANCE_PAIRS_PTX, "squared_distance_pairs")?,
            buffers: RefCell::new(None),
        })
    }

    fn run(&self, session: &CudaSession, a: &[V3], b: &[V3]) -> Option<Vec<f64>> {
        if a.len() != b.len() {
            return None;
        }
        let pair_count = a.len();
        let pair_count_u32 = u32::try_from(pair_count).ok()?;
        let stream = &session.device.stream;
        let flat_a = wire::pack_points_f32(a);
        let flat_b = wire::pack_points_f32(b);
        let mut slot = self.buffers.borrow_mut();
        if slot.as_ref().is_none_or(|buf| buf.capacity < pair_count) {
            let cap = pair_count.max(1);
            *slot = Some(DistancePairBuffers {
                capacity: cap,
                a: stream.alloc_zeros::<f32>(cap * 3).ok()?,
                b: stream.alloc_zeros::<f32>(cap * 3).ok()?,
                out: stream.alloc_zeros::<f32>(cap).ok()?,
            });
        }
        let buffers = slot.as_mut()?;
        {
            let mut a_view = buffers.a.slice_mut(0..(pair_count * 3).max(1));
            stream.memcpy_htod(&flat_a, &mut a_view).ok()?;
            let mut b_view = buffers.b.slice_mut(0..(pair_count * 3).max(1));
            stream.memcpy_htod(&flat_b, &mut b_view).ok()?;
        }
        let a_view = buffers.a.slice(0..(pair_count * 3).max(1));
        let b_view = buffers.b.slice(0..(pair_count * 3).max(1));
        let mut out_view = buffers.out.slice_mut(0..pair_count.max(1));
        let mut launch = stream.launch_builder(&self.kernel);
        launch
            .arg(&pair_count_u32)
            .arg(&a_view)
            .arg(&b_view)
            .arg(&mut out_view);
        // SAFETY: `squared_distance_pairs` takes `(u32, *const f32, *const f32, *mut f32)` with
        // input slices sized `pair_count * 3` and output sized `pair_count` on `stream`.
        unsafe { launch.launch(launch_1d(pair_count_u32, BLOCK)) }.ok()?;
        let mut host = vec![0f32; pair_count];
        stream.memcpy_dtoh(&out_view, &mut host).ok()?;
        Some(wire::fold_f64_vec(&host))
    }
}

/// One-to-one squared distances on the CUDA device; `None` without a device.
pub(crate) fn squared_distance_pairs_cuda(a: &[V3], b: &[V3]) -> Option<Vec<f64>> {
    with_session(|session| {
        session
            .distance_pairs
            .get_or_init(|| DistancePairsKernel::new(session))
            .as_ref()?
            .run(session, a, b)
    })
}

pub(super) struct DistancePairSumKernel {
    kernel: CudaFunction,
    buffers: RefCell<Option<DistancePairSumBuffers>>,
}

struct DistancePairSumBuffers {
    pair_capacity: usize,
    partial_capacity: usize,
    a: CudaSlice<f32>,
    b: CudaSlice<f32>,
    out: CudaSlice<f32>,
}

impl DistancePairSumKernel {
    fn new(session: &CudaSession) -> Option<Self> {
        Some(Self {
            kernel: session.load(DISTANCE_PAIR_SUM_PTX, "squared_distance_pair_sum")?,
            buffers: RefCell::new(None),
        })
    }

    fn run(&self, session: &CudaSession, a: &[V3], b: &[V3]) -> Option<f64> {
        if a.len() != b.len() {
            return None;
        }
        let pair_count = a.len();
        let pair_count_u32 = u32::try_from(pair_count).ok()?;
        let partial_count = pair_count.div_ceil(BLOCK as usize).max(1);
        let stream = &session.device.stream;
        let flat_a = wire::pack_points_f32(a);
        let flat_b = wire::pack_points_f32(b);
        let mut slot = self.buffers.borrow_mut();
        if slot.as_ref().is_none_or(|buf| {
            buf.pair_capacity < pair_count || buf.partial_capacity < partial_count
        }) {
            let pc = pair_count.max(1);
            let rc = partial_count.max(1);
            *slot = Some(DistancePairSumBuffers {
                pair_capacity: pc,
                partial_capacity: rc,
                a: stream.alloc_zeros::<f32>(pc * 3).ok()?,
                b: stream.alloc_zeros::<f32>(pc * 3).ok()?,
                out: stream.alloc_zeros::<f32>(rc).ok()?,
            });
        }
        let buffers = slot.as_mut()?;
        {
            let mut a_view = buffers.a.slice_mut(0..(pair_count * 3).max(1));
            stream.memcpy_htod(&flat_a, &mut a_view).ok()?;
            let mut b_view = buffers.b.slice_mut(0..(pair_count * 3).max(1));
            stream.memcpy_htod(&flat_b, &mut b_view).ok()?;
        }
        let a_view = buffers.a.slice(0..(pair_count * 3).max(1));
        let b_view = buffers.b.slice(0..(pair_count * 3).max(1));
        let mut out_view = buffers.out.slice_mut(0..partial_count);
        let mut launch = stream.launch_builder(&self.kernel);
        launch
            .arg(&pair_count_u32)
            .arg(&a_view)
            .arg(&b_view)
            .arg(&mut out_view);
        // SAFETY: `squared_distance_pair_sum` takes `(u32, *const f32, *const f32, *mut f32)`;
        // device slices cover `pair_count * 3` inputs and `partial_count` block sums on `stream`.
        unsafe { launch.launch(launch_1d(pair_count_u32, BLOCK)) }.ok()?;
        let mut partials = vec![0f32; partial_count];
        stream.memcpy_dtoh(&out_view, &mut partials).ok()?;
        Some(wire::fold_f64_sum(&partials))
    }
}

/// Sum of one-to-one squared distances on the CUDA device; `None` without a device.
pub(crate) fn squared_distance_pair_sum_cuda(a: &[V3], b: &[V3]) -> Option<f64> {
    with_session(|session| {
        session
            .distance_pair_sum
            .get_or_init(|| DistancePairSumKernel::new(session))
            .as_ref()?
            .run(session, a, b)
    })
}

pub(super) struct TransformedDistancePairSumKernel {
    kernel: CudaFunction,
    buffers: RefCell<Option<TransformedDistancePairSumBuffers>>,
}

struct TransformedDistancePairSumBuffers {
    pair_capacity: usize,
    partial_capacity: usize,
    source: CudaSlice<f32>,
    target: CudaSlice<f32>,
    transform: CudaSlice<f32>,
    out: CudaSlice<f32>,
}

impl TransformedDistancePairSumKernel {
    fn new(session: &CudaSession) -> Option<Self> {
        Some(Self {
            kernel: session.load(
                TRANSFORMED_DISTANCE_PAIR_SUM_PTX,
                "transformed_squared_distance_pair_sum",
            )?,
            buffers: RefCell::new(None),
        })
    }

    fn run(
        &self,
        session: &CudaSession,
        source: &[V3],
        target: &[V3],
        m: M3,
        t: V3,
    ) -> Option<f64> {
        if source.len() != target.len() {
            return None;
        }
        let pair_count = source.len();
        let pair_count_u32 = u32::try_from(pair_count).ok()?;
        let partial_count = pair_count.div_ceil(BLOCK as usize).max(1);
        let stream = &session.device.stream;
        let flat_source = wire::pack_points_f32(source);
        let flat_target = wire::pack_points_f32(target);
        let transform = wire::pack_transform_f32(m, t);
        let mut slot = self.buffers.borrow_mut();
        if slot.as_ref().is_none_or(|buf| {
            buf.pair_capacity < pair_count || buf.partial_capacity < partial_count
        }) {
            let pc = pair_count.max(1);
            let rc = partial_count.max(1);
            *slot = Some(TransformedDistancePairSumBuffers {
                pair_capacity: pc,
                partial_capacity: rc,
                source: stream.alloc_zeros::<f32>(pc * 3).ok()?,
                target: stream.alloc_zeros::<f32>(pc * 3).ok()?,
                transform: stream.alloc_zeros::<f32>(12).ok()?,
                out: stream.alloc_zeros::<f32>(rc).ok()?,
            });
        }
        let buffers = slot.as_mut()?;
        {
            let mut source_view = buffers.source.slice_mut(0..(pair_count * 3).max(1));
            stream.memcpy_htod(&flat_source, &mut source_view).ok()?;
            let mut target_view = buffers.target.slice_mut(0..(pair_count * 3).max(1));
            stream.memcpy_htod(&flat_target, &mut target_view).ok()?;
            let mut transform_view = buffers.transform.slice_mut(0..12);
            stream.memcpy_htod(&transform, &mut transform_view).ok()?;
        }
        let source_view = buffers.source.slice(0..(pair_count * 3).max(1));
        let target_view = buffers.target.slice(0..(pair_count * 3).max(1));
        let transform_view = buffers.transform.slice(0..12);
        let mut out_view = buffers.out.slice_mut(0..partial_count);
        let mut launch = stream.launch_builder(&self.kernel);
        launch
            .arg(&pair_count_u32)
            .arg(&source_view)
            .arg(&target_view)
            .arg(&transform_view)
            .arg(&mut out_view);
        // SAFETY: `transformed_squared_distance_pair_sum` takes `(u32, *const f32, *const f32, *const f32, *mut f32)`;
        // `source_view`/`target_view` (`pair_count * 3`), `transform_view` (`12`), and `out_view` (`partial_count`)
        // remain live on `stream` for the 1D grid covering `pair_count_u32`.
        unsafe { launch.launch(launch_1d(pair_count_u32, BLOCK)) }.ok()?;
        let mut partials = vec![0f32; partial_count];
        stream.memcpy_dtoh(&out_view, &mut partials).ok()?;
        Some(wire::fold_f64_sum(&partials))
    }
}

/// Fused transform-and-sum of one-to-one squared distances on CUDA; `None` without a device.
pub(crate) fn transformed_squared_distance_pair_sum_cuda(
    source: &[V3],
    target: &[V3],
    m: M3,
    t: V3,
) -> Option<f64> {
    with_session(|session| {
        session
            .transformed_distance_pair_sum
            .get_or_init(|| TransformedDistancePairSumKernel::new(session))
            .as_ref()?
            .run(session, source, target, m, t)
    })
}
