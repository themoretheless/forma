use super::*;

struct Buffers {
    query_capacity: usize,
    target_capacity: usize,
    queries: CudaSlice<f32>,
    targets: CudaSlice<f32>,
    out_index: CudaSlice<u32>,
    out_dist: CudaSlice<f32>,
}

struct CudaNearestNeighbor {
    device: CudaDevice,
    kernel: CudaFunction,
    buffers: std::cell::RefCell<Option<Buffers>>,
}

impl CudaNearestNeighbor {
    fn new() -> Option<Self> {
        let device = CudaDevice::new()?;
        let module = device.load_ptx(NEAREST_NEIGHBOR_PTX)?;
        let kernel = module.load_function("nearest_neighbor").ok()?;
        Some(Self {
            device,
            kernel,
            buffers: std::cell::RefCell::new(None),
        })
    }

    /// (Re)allocates the device slices when either capacity is exceeded; a
    /// no-op otherwise, so repeated calls at a stable or shrinking size
    /// reuse the same device allocations.
    fn ensure_buffers(&self, query_count: usize, target_count: usize) -> Option<()> {
        let stale = match &*self.buffers.borrow() {
            Some(b) => b.query_capacity < query_count || b.target_capacity < target_count,
            None => true,
        };
        if !stale {
            return Some(());
        }
        let query_capacity = query_count.max(1);
        let target_capacity = target_count.max(1);
        let stream = &self.device.stream;
        let queries = stream.alloc_zeros::<f32>(query_capacity * 3).ok()?;
        let targets = stream.alloc_zeros::<f32>(target_capacity * 3).ok()?;
        let out_index = stream.alloc_zeros::<u32>(query_capacity).ok()?;
        let out_dist = stream.alloc_zeros::<f32>(query_capacity).ok()?;
        *self.buffers.borrow_mut() = Some(Buffers {
            query_capacity,
            target_capacity,
            queries,
            targets,
            out_index,
            out_dist,
        });
        Some(())
    }

    fn run(&self, queries: &[V3], targets: &[V3]) -> Option<Vec<(u32, f64)>> {
        let query_count = queries.len();
        let target_count = targets.len();
        self.ensure_buffers(query_count, target_count)?;
        let stream = &self.device.stream;
        let flat_q: Vec<f32> = queries.iter().flatten().map(|&v| v as f32).collect();
        let flat_t: Vec<f32> = targets.iter().flatten().map(|&v| v as f32).collect();
        let mut buffers = self.buffers.borrow_mut();
        let b = buffers.as_mut().expect("ensure_buffers was just called");
        if !flat_q.is_empty() {
            let mut view = b.queries.slice_mut(0..flat_q.len());
            stream.memcpy_htod(&flat_q, &mut view).ok()?;
        }
        if !flat_t.is_empty() {
            let mut view = b.targets.slice_mut(0..flat_t.len());
            stream.memcpy_htod(&flat_t, &mut view).ok()?;
        }
        let query_count_u32 = query_count as u32;
        let target_count_u32 = target_count as u32;
        let q_elems = (query_count * 3).max(1);
        let t_elems = (target_count * 3).max(1);
        let out_elems = query_count.max(1);
        let queries_view = b.queries.slice(0..q_elems);
        let targets_view = b.targets.slice(0..t_elems);
        let mut out_index_view = b.out_index.slice_mut(0..out_elems);
        let mut out_dist_view = b.out_dist.slice_mut(0..out_elems);
        let mut launch = stream.launch_builder(&self.kernel);
        launch
            .arg(&query_count_u32)
            .arg(&target_count_u32)
            .arg(&queries_view)
            .arg(&targets_view)
            .arg(&mut out_index_view)
            .arg(&mut out_dist_view);
        unsafe { launch.launch(launch_1d(query_count_u32, BLOCK)) }.ok()?;
        if query_count == 0 {
            return Some(Vec::new());
        }
        let mut out_index = vec![0u32; query_count];
        let mut out_dist = vec![0f32; query_count];
        stream.memcpy_dtoh(&out_index_view, &mut out_index).ok()?;
        stream.memcpy_dtoh(&out_dist_view, &mut out_dist).ok()?;
        Some(
            out_index
                .into_iter()
                .zip(out_dist)
                .map(|(i, d)| (i, d as f64))
                .collect(),
        )
    }
}

thread_local! {
    // Process-lifetime context, module and stream; see `sdf-core`'s `cuda.rs`.
    static SHARED: std::cell::LazyCell<Option<&'static CudaNearestNeighbor>> =
        std::cell::LazyCell::new(|| {
            CudaNearestNeighbor::new().map(|nn| Box::leak(Box::new(nn)) as &'static CudaNearestNeighbor)
        });
}

/// True when a CUDA device and the kernel module are available on this thread.
pub fn available() -> bool {
    SHARED.with(|cell| {
        let shared: &Option<&CudaNearestNeighbor> = cell;
        shared.is_some()
    })
}

/// Device name for diagnostics/benchmarks; `None` without a CUDA device.
pub fn device_name() -> Option<String> {
    device_report().map(|report| report.name)
}

pub fn device_report() -> Option<CudaDeviceReport> {
    SHARED.with(|cell| {
        let shared: &Option<&CudaNearestNeighbor> = cell;
        shared.map(|nn| nn.device.report())
    })
}

/// Finds each query's nearest target on the CUDA device; `None` without a
/// device or on any driver error (the caller then tries the wgpu path and
/// the CPU reference).
pub(crate) fn nearest_neighbor_cuda(queries: &[V3], targets: &[V3]) -> Option<Vec<(u32, f64)>> {
    SHARED.with(|cell| {
        let shared: &Option<&CudaNearestNeighbor> = cell;
        shared.and_then(|nn| nn.run(queries, targets))
    })
}

struct CudaNearestTwo {
    device: CudaDevice,
    kernel: CudaFunction,
    buffers: std::cell::RefCell<Option<NearestTwoBuffers>>,
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

impl CudaNearestTwo {
    fn new() -> Option<Self> {
        let device = CudaDevice::new()?;
        let module = device.load_ptx(NEAREST_TWO_PTX)?;
        let kernel = module.load_function("nearest_two").ok()?;
        Some(Self {
            device,
            kernel,
            buffers: std::cell::RefCell::new(None),
        })
    }

    fn ensure_buffers(&self, query_count: usize, target_count: usize) -> Option<()> {
        let stale = match &*self.buffers.borrow() {
            Some(b) => b.query_capacity < query_count || b.target_capacity < target_count,
            None => true,
        };
        if !stale {
            return Some(());
        }
        let query_capacity = query_count.max(1);
        let target_capacity = target_count.max(1);
        let stream = &self.device.stream;
        let queries = stream.alloc_zeros::<f32>(query_capacity * 3).ok()?;
        let targets = stream.alloc_zeros::<f32>(target_capacity * 3).ok()?;
        let out_i0 = stream.alloc_zeros::<u32>(query_capacity).ok()?;
        let out_d0 = stream.alloc_zeros::<f32>(query_capacity).ok()?;
        let out_i1 = stream.alloc_zeros::<u32>(query_capacity).ok()?;
        let out_d1 = stream.alloc_zeros::<f32>(query_capacity).ok()?;
        *self.buffers.borrow_mut() = Some(NearestTwoBuffers {
            query_capacity,
            target_capacity,
            queries,
            targets,
            out_i0,
            out_d0,
            out_i1,
            out_d1,
        });
        Some(())
    }

    fn run(&self, queries: &[V3], targets: &[V3]) -> Option<Vec<crate::TwoNearest>> {
        let query_count = queries.len();
        let target_count = targets.len();
        let stream = &self.device.stream;
        let flat_q: Vec<f32> = queries.iter().flatten().map(|&v| v as f32).collect();
        let flat_t: Vec<f32> = targets.iter().flatten().map(|&v| v as f32).collect();
        self.ensure_buffers(query_count, target_count)?;
        let mut buffers = self.buffers.borrow_mut();
        let b = buffers.as_mut().expect("ensure_buffers was just called");
        if !flat_q.is_empty() {
            let mut view = b.queries.slice_mut(0..flat_q.len());
            stream.memcpy_htod(&flat_q, &mut view).ok()?;
        }
        if !flat_t.is_empty() {
            let mut view = b.targets.slice_mut(0..flat_t.len());
            stream.memcpy_htod(&flat_t, &mut view).ok()?;
        }
        let query_count_u32 = query_count as u32;
        let target_count_u32 = target_count as u32;
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
        Some(
            (0..query_count)
                .map(|idx| [(i0[idx], d0[idx] as f64), (i1[idx], d1[idx] as f64)])
                .collect(),
        )
    }
}

thread_local! {
    static SHARED_NEAREST_TWO: std::cell::LazyCell<Option<&'static CudaNearestTwo>> =
        std::cell::LazyCell::new(|| {
            CudaNearestTwo::new().map(|nn| Box::leak(Box::new(nn)) as &'static CudaNearestTwo)
        });
}

pub(crate) fn nearest_two_cuda(queries: &[V3], targets: &[V3]) -> Option<Vec<crate::TwoNearest>> {
    SHARED_NEAREST_TWO.with(|cell| {
        let shared: &Option<&CudaNearestTwo> = cell;
        shared.and_then(|nn| nn.run(queries, targets))
    })
}

struct CudaNearestFour {
    device: CudaDevice,
    kernel: CudaFunction,
    buffers: std::cell::RefCell<Option<NearestFourBuffers>>,
}

struct NearestFourBuffers {
    query_capacity: usize,
    target_capacity: usize,
    queries: CudaSlice<f32>,
    targets: CudaSlice<f32>,
    out_indices: CudaSlice<u32>,
    out_distances: CudaSlice<f32>,
}

impl CudaNearestFour {
    fn new() -> Option<Self> {
        let device = CudaDevice::new()?;
        let module = device.load_ptx(NEAREST_FOUR_PTX)?;
        let kernel = module.load_function("nearest_four").ok()?;
        Some(Self {
            device,
            kernel,
            buffers: std::cell::RefCell::new(None),
        })
    }

    fn ensure_buffers(&self, query_count: usize, target_count: usize) -> Option<()> {
        let stale = match &*self.buffers.borrow() {
            Some(b) => b.query_capacity < query_count || b.target_capacity < target_count,
            None => true,
        };
        if !stale {
            return Some(());
        }
        let query_capacity = query_count.max(1);
        let target_capacity = target_count.max(1);
        let stream = &self.device.stream;
        let queries = stream.alloc_zeros::<f32>(query_capacity * 3).ok()?;
        let targets = stream.alloc_zeros::<f32>(target_capacity * 3).ok()?;
        let out_indices = stream.alloc_zeros::<u32>(query_capacity * 4).ok()?;
        let out_distances = stream.alloc_zeros::<f32>(query_capacity * 4).ok()?;
        *self.buffers.borrow_mut() = Some(NearestFourBuffers {
            query_capacity,
            target_capacity,
            queries,
            targets,
            out_indices,
            out_distances,
        });
        Some(())
    }

    fn run(&self, queries: &[V3], targets: &[V3]) -> Option<Vec<crate::FourNearest>> {
        let query_count = queries.len();
        let target_count = targets.len();
        let stream = &self.device.stream;
        let flat_q: Vec<f32> = queries.iter().flatten().map(|&v| v as f32).collect();
        let flat_t: Vec<f32> = targets.iter().flatten().map(|&v| v as f32).collect();
        self.ensure_buffers(query_count, target_count)?;
        let mut buffers = self.buffers.borrow_mut();
        let b = buffers.as_mut().expect("ensure_buffers was just called");
        if !flat_q.is_empty() {
            let mut view = b.queries.slice_mut(0..flat_q.len());
            stream.memcpy_htod(&flat_q, &mut view).ok()?;
        }
        if !flat_t.is_empty() {
            let mut view = b.targets.slice_mut(0..flat_t.len());
            stream.memcpy_htod(&flat_t, &mut view).ok()?;
        }
        let query_count_u32 = query_count as u32;
        let target_count_u32 = target_count as u32;
        let q_view = b.queries.slice(0..(query_count * 3).max(1));
        let t_view = b.targets.slice(0..(target_count * 3).max(1));
        let mut indices_view = b.out_indices.slice_mut(0..(query_count * 4).max(1));
        let mut distances_view = b.out_distances.slice_mut(0..(query_count * 4).max(1));
        let mut launch = stream.launch_builder(&self.kernel);
        launch
            .arg(&query_count_u32)
            .arg(&target_count_u32)
            .arg(&q_view)
            .arg(&t_view)
            .arg(&mut indices_view)
            .arg(&mut distances_view);
        unsafe { launch.launch(launch_1d(query_count_u32, BLOCK)) }.ok()?;
        let mut indices = vec![0u32; query_count * 4];
        let mut distances = vec![0f32; query_count * 4];
        if query_count > 0 {
            stream.memcpy_dtoh(&indices_view, &mut indices).ok()?;
            stream.memcpy_dtoh(&distances_view, &mut distances).ok()?;
        }
        Some(
            (0..query_count)
                .map(|idx| {
                    std::array::from_fn(|k| {
                        let off = idx * 4 + k;
                        (indices[off], distances[off] as f64)
                    })
                })
                .collect(),
        )
    }
}

thread_local! {
    static SHARED_NEAREST_FOUR: std::cell::LazyCell<Option<&'static CudaNearestFour>> =
        std::cell::LazyCell::new(|| {
            CudaNearestFour::new().map(|nn| Box::leak(Box::new(nn)) as &'static CudaNearestFour)
        });
}

pub(crate) fn nearest_four_cuda(queries: &[V3], targets: &[V3]) -> Option<Vec<crate::FourNearest>> {
    SHARED_NEAREST_FOUR.with(|cell| {
        let shared: &Option<&CudaNearestFour> = cell;
        shared.and_then(|nn| nn.run(queries, targets))
    })
}

struct CudaChamfer {
    device: CudaDevice,
    kernel: CudaFunction,
    buffers: std::cell::RefCell<Option<ChamferBuffers>>,
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

impl CudaChamfer {
    fn new() -> Option<Self> {
        let device = CudaDevice::new()?;
        let module = device.load_ptx(CHAMFER_PTX)?;
        let kernel = module.load_function("directed_chamfer_reduce").ok()?;
        Some(Self {
            device,
            kernel,
            buffers: std::cell::RefCell::new(None),
        })
    }

    fn ensure_buffers(
        &self,
        query_count: usize,
        target_count: usize,
        partial_count: usize,
    ) -> Option<()> {
        let stale = match &*self.buffers.borrow() {
            Some(b) => {
                b.query_capacity < query_count
                    || b.target_capacity < target_count
                    || b.partial_capacity < partial_count
            }
            None => true,
        };
        if !stale {
            return Some(());
        }
        let query_capacity = query_count.max(1);
        let target_capacity = target_count.max(1);
        let partial_capacity = partial_count.max(1);
        let stream = &self.device.stream;
        let queries = stream.alloc_zeros::<f32>(query_capacity * 3).ok()?;
        let targets = stream.alloc_zeros::<f32>(target_capacity * 3).ok()?;
        let out_sum = stream.alloc_zeros::<f32>(partial_capacity).ok()?;
        let out_max = stream.alloc_zeros::<f32>(partial_capacity).ok()?;
        *self.buffers.borrow_mut() = Some(ChamferBuffers {
            query_capacity,
            target_capacity,
            partial_capacity,
            queries,
            targets,
            out_sum,
            out_max,
        });
        Some(())
    }

    fn run(&self, queries: &[V3], targets: &[V3]) -> Option<crate::DirectedChamfer> {
        if queries.is_empty() || targets.is_empty() {
            return None;
        }
        let query_count = queries.len();
        let target_count = targets.len();
        let partial_count = query_count.div_ceil(BLOCK as usize).max(1);
        let stream = &self.device.stream;
        let flat_q: Vec<f32> = queries.iter().flatten().map(|&v| v as f32).collect();
        let flat_t: Vec<f32> = targets.iter().flatten().map(|&v| v as f32).collect();
        self.ensure_buffers(query_count, target_count, partial_count)?;
        let mut buffers = self.buffers.borrow_mut();
        let buffers = buffers.as_mut().expect("ensure_buffers was just called");
        {
            let mut q_view = buffers.queries.slice_mut(0..query_count * 3);
            stream.memcpy_htod(&flat_q, &mut q_view).ok()?;
        }
        {
            let mut t_view = buffers.targets.slice_mut(0..target_count * 3);
            stream.memcpy_htod(&flat_t, &mut t_view).ok()?;
        }
        let query_count_u32 = query_count as u32;
        let target_count_u32 = target_count as u32;
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
        unsafe { launch.launch(launch_1d(query_count_u32, BLOCK)) }.ok()?;
        let mut sums = vec![0f32; partial_count];
        let mut maxes = vec![0f32; partial_count];
        stream.memcpy_dtoh(&sum_view, &mut sums).ok()?;
        stream.memcpy_dtoh(&max_view, &mut maxes).ok()?;
        let sum: f64 = sums.into_iter().map(|value| value as f64).sum();
        let max_squared_distance = maxes
            .into_iter()
            .map(|value| value as f64)
            .fold(0., f64::max);
        let mean_squared_distance = sum / query_count as f64;
        Some(crate::DirectedChamfer {
            samples: query_count,
            mean_squared_distance,
            rms_distance: mean_squared_distance.sqrt(),
            max_squared_distance,
        })
    }
}

thread_local! {
    static SHARED_CHAMFER: std::cell::LazyCell<Option<&'static CudaChamfer>> =
        std::cell::LazyCell::new(|| {
            CudaChamfer::new().map(|kernel| Box::leak(Box::new(kernel)) as &'static CudaChamfer)
        });
}

/// Directed Chamfer reduction on the CUDA device; `None` without a device.
pub(crate) fn directed_chamfer_cuda(
    queries: &[V3],
    targets: &[V3],
) -> Option<crate::DirectedChamfer> {
    SHARED_CHAMFER.with(|cell| {
        let shared: &Option<&CudaChamfer> = cell;
        shared.and_then(|kernel| kernel.run(queries, targets))
    })
}
