use super::*;

struct PairOutputsBuffers {
    query_capacity: usize,
    target_capacity: usize,
    queries: CudaSlice<f32>,
    targets: CudaSlice<f32>,
    out_index: CudaSlice<u32>,
    out_dist: CudaSlice<f32>,
}

fn upload_query_target(
    stream: &std::sync::Arc<gpu_compute::cuda::CudaStream>,
    queries_buf: &mut CudaSlice<f32>,
    targets_buf: &mut CudaSlice<f32>,
    flat_q: &[f32],
    flat_t: &[f32],
) -> Option<()> {
    if !flat_q.is_empty() {
        let mut view = queries_buf.slice_mut(0..flat_q.len());
        stream.memcpy_htod(flat_q, &mut view).ok()?;
    }
    if !flat_t.is_empty() {
        let mut view = targets_buf.slice_mut(0..flat_t.len());
        stream.memcpy_htod(flat_t, &mut view).ok()?;
    }
    Some(())
}

pub(super) struct NearestNeighborKernel {
    kernel: CudaFunction,
    buffers: RefCell<Option<PairOutputsBuffers>>,
}

impl NearestNeighborKernel {
    fn new(session: &CudaSession) -> Option<Self> {
        Some(Self {
            kernel: session.load(NEAREST_NEIGHBOR_PTX, "nearest_neighbor")?,
            buffers: RefCell::new(None),
        })
    }

    fn run(
        &self,
        session: &CudaSession,
        queries: &[V3],
        targets: &[V3],
    ) -> Option<Vec<(u32, f64)>> {
        let query_count = queries.len();
        let target_count = targets.len();
        let query_count_u32 = u32::try_from(query_count).ok()?;
        let target_count_u32 = u32::try_from(target_count).ok()?;
        let stream = &session.device.stream;
        let flat_q = wire::pack_points_f32(queries);
        let flat_t = wire::pack_points_f32(targets);
        let mut slot = self.buffers.borrow_mut();
        if slot
            .as_ref()
            .is_none_or(|b| b.query_capacity < query_count || b.target_capacity < target_count)
        {
            let qc = query_count.max(1);
            let tc = target_count.max(1);
            *slot = Some(PairOutputsBuffers {
                query_capacity: qc,
                target_capacity: tc,
                queries: stream.alloc_zeros::<f32>(qc * 3).ok()?,
                targets: stream.alloc_zeros::<f32>(tc * 3).ok()?,
                out_index: stream.alloc_zeros::<u32>(qc).ok()?,
                out_dist: stream.alloc_zeros::<f32>(qc).ok()?,
            });
        }
        let b = slot.as_mut()?;
        upload_query_target(stream, &mut b.queries, &mut b.targets, &flat_q, &flat_t)?;
        let q_view = b.queries.slice(0..(query_count * 3).max(1));
        let t_view = b.targets.slice(0..(target_count * 3).max(1));
        let mut out_index_view = b.out_index.slice_mut(0..query_count.max(1));
        let mut out_dist_view = b.out_dist.slice_mut(0..query_count.max(1));
        let mut launch = stream.launch_builder(&self.kernel);
        launch
            .arg(&query_count_u32)
            .arg(&target_count_u32)
            .arg(&q_view)
            .arg(&t_view)
            .arg(&mut out_index_view)
            .arg(&mut out_dist_view);
        // SAFETY: `nearest_neighbor` takes `(u32, u32, *const f32, *const f32, *mut u32, *mut f32)`;
        // query/target slices hold `count * 3` floats and outputs hold `query_count` entries.
        unsafe { launch.launch(launch_1d(query_count_u32, BLOCK)) }.ok()?;
        if query_count == 0 {
            return Some(Vec::new());
        }
        let mut out_index = vec![0u32; query_count];
        let mut out_dist = vec![0f32; query_count];
        stream.memcpy_dtoh(&out_index_view, &mut out_index).ok()?;
        stream.memcpy_dtoh(&out_dist_view, &mut out_dist).ok()?;
        Some(wire::zip_nearest_one(out_index, out_dist))
    }
}

pub(crate) fn nearest_neighbor_cuda(queries: &[V3], targets: &[V3]) -> Option<Vec<(u32, f64)>> {
    with_session(|session| {
        session
            .nearest_neighbor
            .get_or_init(|| NearestNeighborKernel::new(session))
            .as_ref()?
            .run(session, queries, targets)
    })
}

pub(super) struct NearestTwoKernel {
    kernel: CudaFunction,
    buffers: RefCell<Option<NearestTwoBuffers>>,
}

struct NearestTwoBuffers {
    query_capacity: usize,
    target_capacity: usize,
    queries: CudaSlice<f32>,
    targets: CudaSlice<f32>,
    out_i0: CudaSlice<u32>,
    out_d0: CudaSlice<f32>,
    out_i1: CudaSlice<u32>,
    out_d1: CudaSlice<f32>,
}

impl NearestTwoKernel {
    fn new(session: &CudaSession) -> Option<Self> {
        Some(Self {
            kernel: session.load(NEAREST_TWO_PTX, "nearest_two")?,
            buffers: RefCell::new(None),
        })
    }

    fn run(
        &self,
        session: &CudaSession,
        queries: &[V3],
        targets: &[V3],
    ) -> Option<Vec<crate::TwoNearest>> {
        let query_count = queries.len();
        let target_count = targets.len();
        let query_count_u32 = u32::try_from(query_count).ok()?;
        let target_count_u32 = u32::try_from(target_count).ok()?;
        let stream = &session.device.stream;
        let flat_q = wire::pack_points_f32(queries);
        let flat_t = wire::pack_points_f32(targets);
        let mut slot = self.buffers.borrow_mut();
        if slot
            .as_ref()
            .is_none_or(|b| b.query_capacity < query_count || b.target_capacity < target_count)
        {
            let qc = query_count.max(1);
            let tc = target_count.max(1);
            *slot = Some(NearestTwoBuffers {
                query_capacity: qc,
                target_capacity: tc,
                queries: stream.alloc_zeros::<f32>(qc * 3).ok()?,
                targets: stream.alloc_zeros::<f32>(tc * 3).ok()?,
                out_i0: stream.alloc_zeros::<u32>(qc).ok()?,
                out_d0: stream.alloc_zeros::<f32>(qc).ok()?,
                out_i1: stream.alloc_zeros::<u32>(qc).ok()?,
                out_d1: stream.alloc_zeros::<f32>(qc).ok()?,
            });
        }
        let b = slot.as_mut()?;
        upload_query_target(stream, &mut b.queries, &mut b.targets, &flat_q, &flat_t)?;
        let q_view = b.queries.slice(0..(query_count * 3).max(1));
        let t_view = b.targets.slice(0..(target_count * 3).max(1));
        let mut i0_view = b.out_i0.slice_mut(0..query_count.max(1));
        let mut d0_view = b.out_d0.slice_mut(0..query_count.max(1));
        let mut i1_view = b.out_i1.slice_mut(0..query_count.max(1));
        let mut d1_view = b.out_d1.slice_mut(0..query_count.max(1));
        let mut launch = stream.launch_builder(&self.kernel);
        launch
            .arg(&query_count_u32)
            .arg(&target_count_u32)
            .arg(&q_view)
            .arg(&t_view)
            .arg(&mut i0_view)
            .arg(&mut d0_view)
            .arg(&mut i1_view)
            .arg(&mut d1_view);
        // SAFETY: `nearest_two` takes `(u32, u32, *const f32, *const f32, *mut u32, *mut f32, *mut u32, *mut f32)`;
        // input/output views are allocated for `query_count` queries and `target_count` targets.
        unsafe { launch.launch(launch_1d(query_count_u32, BLOCK)) }.ok()?;
        let mut i0 = vec![0u32; query_count];
        let mut d0 = vec![0f32; query_count];
        let mut i1 = vec![0u32; query_count];
        let mut d1 = vec![0f32; query_count];
        if query_count > 0 {
            stream.memcpy_dtoh(&i0_view, &mut i0).ok()?;
            stream.memcpy_dtoh(&d0_view, &mut d0).ok()?;
            stream.memcpy_dtoh(&i1_view, &mut i1).ok()?;
            stream.memcpy_dtoh(&d1_view, &mut d1).ok()?;
        }
        Some(wire::zip_nearest_two(&i0, &d0, &i1, &d1))
    }
}

pub(crate) fn nearest_two_cuda(queries: &[V3], targets: &[V3]) -> Option<Vec<crate::TwoNearest>> {
    with_session(|session| {
        session
            .nearest_two
            .get_or_init(|| NearestTwoKernel::new(session))
            .as_ref()?
            .run(session, queries, targets)
    })
}

pub(super) struct NearestFourKernel {
    kernel: CudaFunction,
    buffers: RefCell<Option<PairOutputsBuffers>>,
}

impl NearestFourKernel {
    fn new(session: &CudaSession) -> Option<Self> {
        Some(Self {
            kernel: session.load(NEAREST_FOUR_PTX, "nearest_four")?,
            buffers: RefCell::new(None),
        })
    }

    fn run(
        &self,
        session: &CudaSession,
        queries: &[V3],
        targets: &[V3],
    ) -> Option<Vec<crate::FourNearest>> {
        let query_count = queries.len();
        let target_count = targets.len();
        let query_count_u32 = u32::try_from(query_count).ok()?;
        let target_count_u32 = u32::try_from(target_count).ok()?;
        let stream = &session.device.stream;
        let flat_q = wire::pack_points_f32(queries);
        let flat_t = wire::pack_points_f32(targets);
        let mut slot = self.buffers.borrow_mut();
        if slot
            .as_ref()
            .is_none_or(|b| b.query_capacity < query_count || b.target_capacity < target_count)
        {
            let qc = query_count.max(1);
            let tc = target_count.max(1);
            *slot = Some(PairOutputsBuffers {
                query_capacity: qc,
                target_capacity: tc,
                queries: stream.alloc_zeros::<f32>(qc * 3).ok()?,
                targets: stream.alloc_zeros::<f32>(tc * 3).ok()?,
                out_index: stream.alloc_zeros::<u32>(qc * 4).ok()?,
                out_dist: stream.alloc_zeros::<f32>(qc * 4).ok()?,
            });
        }
        let b = slot.as_mut()?;
        upload_query_target(stream, &mut b.queries, &mut b.targets, &flat_q, &flat_t)?;
        let q_view = b.queries.slice(0..(query_count * 3).max(1));
        let t_view = b.targets.slice(0..(target_count * 3).max(1));
        let mut indices_view = b.out_index.slice_mut(0..(query_count * 4).max(1));
        let mut distances_view = b.out_dist.slice_mut(0..(query_count * 4).max(1));
        let mut launch = stream.launch_builder(&self.kernel);
        launch
            .arg(&query_count_u32)
            .arg(&target_count_u32)
            .arg(&q_view)
            .arg(&t_view)
            .arg(&mut indices_view)
            .arg(&mut distances_view);
        // SAFETY: `nearest_four` takes `(u32, u32, *const f32, *const f32, *mut u32, *mut f32)`;
        // outputs are sized `query_count * 4` on `stream`.
        unsafe { launch.launch(launch_1d(query_count_u32, BLOCK)) }.ok()?;
        let mut indices = vec![0u32; query_count * 4];
        let mut distances = vec![0f32; query_count * 4];
        if query_count > 0 {
            stream.memcpy_dtoh(&indices_view, &mut indices).ok()?;
            stream.memcpy_dtoh(&distances_view, &mut distances).ok()?;
        }
        Some(wire::zip_nearest_four(&indices, &distances, query_count))
    }
}

pub(crate) fn nearest_four_cuda(queries: &[V3], targets: &[V3]) -> Option<Vec<crate::FourNearest>> {
    with_session(|session| {
        session
            .nearest_four
            .get_or_init(|| NearestFourKernel::new(session))
            .as_ref()?
            .run(session, queries, targets)
    })
}

pub(super) struct ChamferKernel {
    kernel: CudaFunction,
    buffers: RefCell<Option<ChamferBuffers>>,
}

struct ChamferBuffers {
    query_capacity: usize,
    target_capacity: usize,
    partial_capacity: usize,
    queries: CudaSlice<f32>,
    targets: CudaSlice<f32>,
    out_sum: CudaSlice<f32>,
    out_max: CudaSlice<f32>,
}

impl ChamferKernel {
    fn new(session: &CudaSession) -> Option<Self> {
        Some(Self {
            kernel: session.load(CHAMFER_PTX, "directed_chamfer_reduce")?,
            buffers: RefCell::new(None),
        })
    }

    fn run(
        &self,
        session: &CudaSession,
        queries: &[V3],
        targets: &[V3],
    ) -> Option<crate::DirectedChamfer> {
        if queries.is_empty() || targets.is_empty() {
            return None;
        }
        let query_count = queries.len();
        let target_count = targets.len();
        let query_count_u32 = u32::try_from(query_count).ok()?;
        let target_count_u32 = u32::try_from(target_count).ok()?;
        let partial_count = query_count.div_ceil(BLOCK as usize).max(1);
        let stream = &session.device.stream;
        let flat_q = wire::pack_points_f32(queries);
        let flat_t = wire::pack_points_f32(targets);
        let mut slot = self.buffers.borrow_mut();
        if slot.as_ref().is_none_or(|b| {
            b.query_capacity < query_count
                || b.target_capacity < target_count
                || b.partial_capacity < partial_count
        }) {
            let qc = query_count.max(1);
            let tc = target_count.max(1);
            let rc = partial_count.max(1);
            *slot = Some(ChamferBuffers {
                query_capacity: qc,
                target_capacity: tc,
                partial_capacity: rc,
                queries: stream.alloc_zeros::<f32>(qc * 3).ok()?,
                targets: stream.alloc_zeros::<f32>(tc * 3).ok()?,
                out_sum: stream.alloc_zeros::<f32>(rc).ok()?,
                out_max: stream.alloc_zeros::<f32>(rc).ok()?,
            });
        }
        let buffers = slot.as_mut()?;
        upload_query_target(
            stream,
            &mut buffers.queries,
            &mut buffers.targets,
            &flat_q,
            &flat_t,
        )?;
        let q_view = buffers.queries.slice(0..query_count * 3);
        let t_view = buffers.targets.slice(0..target_count * 3);
        let mut sum_view = buffers.out_sum.slice_mut(0..partial_count);
        let mut max_view = buffers.out_max.slice_mut(0..partial_count);
        let mut launch = stream.launch_builder(&self.kernel);
        launch
            .arg(&query_count_u32)
            .arg(&target_count_u32)
            .arg(&q_view)
            .arg(&t_view)
            .arg(&mut sum_view)
            .arg(&mut max_view);
        // SAFETY: `directed_chamfer_reduce` takes `(u32, u32, *const f32, *const f32, *mut f32, *mut f32)`;
        // `out_sum`/`out_max` hold `partial_count` block partial sums and maxima on `stream`.
        unsafe { launch.launch(launch_1d(query_count_u32, BLOCK)) }.ok()?;
        let mut sums = vec![0f32; partial_count];
        let mut maxes = vec![0f32; partial_count];
        stream.memcpy_dtoh(&sum_view, &mut sums).ok()?;
        stream.memcpy_dtoh(&max_view, &mut maxes).ok()?;
        Some(wire::fold_chamfer(&sums, &maxes, query_count))
    }
}

/// Directed Chamfer reduction on the CUDA device; `None` without a device.
pub(crate) fn directed_chamfer_cuda(
    queries: &[V3],
    targets: &[V3],
) -> Option<crate::DirectedChamfer> {
    with_session(|session| {
        session
            .chamfer
            .get_or_init(|| ChamferKernel::new(session))
            .as_ref()?
            .run(session, queries, targets)
    })
}
