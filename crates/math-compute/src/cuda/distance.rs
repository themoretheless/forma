use super::*;

struct CudaDistancePairs {
    device: CudaDevice,
    kernel: CudaFunction,
    buffers: std::cell::RefCell<Option<DistancePairBuffers>>,
}

struct DistancePairBuffers {
    capacity: usize,
    a: CudaSlice<f32>,
    b: CudaSlice<f32>,
    out: CudaSlice<f32>,
}

impl CudaDistancePairs {
    fn new() -> Option<Self> {
        let device = CudaDevice::new()?;
        let module = device.load_ptx(DISTANCE_PAIRS_PTX)?;
        let kernel = module.load_function("squared_distance_pairs").ok()?;
        Some(Self {
            device,
            kernel,
            buffers: std::cell::RefCell::new(None),
        })
    }

    fn ensure_buffers(&self, pair_count: usize) -> Option<()> {
        let stale = match &*self.buffers.borrow() {
            Some(buffers) => buffers.capacity < pair_count,
            None => true,
        };
        if !stale {
            return Some(());
        }
        let capacity = pair_count.max(1);
        let stream = &self.device.stream;
        let a = stream.alloc_zeros::<f32>(capacity * 3).ok()?;
        let b = stream.alloc_zeros::<f32>(capacity * 3).ok()?;
        let out = stream.alloc_zeros::<f32>(capacity).ok()?;
        *self.buffers.borrow_mut() = Some(DistancePairBuffers {
            capacity,
            a,
            b,
            out,
        });
        Some(())
    }

    fn run(&self, a: &[V3], b: &[V3]) -> Option<Vec<f64>> {
        if a.len() != b.len() {
            return None;
        }
        let pair_count = a.len();
        let stream = &self.device.stream;
        let flat_a: Vec<f32> = a.iter().flatten().map(|&v| v as f32).collect();
        let flat_b: Vec<f32> = b.iter().flatten().map(|&v| v as f32).collect();
        self.ensure_buffers(pair_count)?;
        let mut buffers = self.buffers.borrow_mut();
        let buffers = buffers.as_mut().expect("ensure_buffers was just called");
        {
            let mut a_view = buffers.a.slice_mut(0..(pair_count * 3).max(1));
            stream.memcpy_htod(&flat_a, &mut a_view).ok()?;
        }
        {
            let mut b_view = buffers.b.slice_mut(0..(pair_count * 3).max(1));
            stream.memcpy_htod(&flat_b, &mut b_view).ok()?;
        }
        let pair_count_u32 = pair_count as u32;
        let a_view = buffers.a.slice(0..(pair_count * 3).max(1));
        let b_view = buffers.b.slice(0..(pair_count * 3).max(1));
        let mut out_view = buffers.out.slice_mut(0..pair_count.max(1));
        let mut launch = stream.launch_builder(&self.kernel);
        launch
            .arg(&pair_count_u32)
            .arg(&a_view)
            .arg(&b_view)
            .arg(&mut out_view);
        unsafe { launch.launch(launch_1d(pair_count_u32, BLOCK)) }.ok()?;
        let mut host = vec![0f32; pair_count];
        stream.memcpy_dtoh(&out_view, &mut host).ok()?;
        Some(host.into_iter().map(|value| value as f64).collect())
    }
}

thread_local! {
    static SHARED_DISTANCE_PAIRS: std::cell::LazyCell<Option<&'static CudaDistancePairs>> =
        std::cell::LazyCell::new(|| {
            CudaDistancePairs::new().map(|kernel| Box::leak(Box::new(kernel)) as &'static CudaDistancePairs)
        });
}

/// One-to-one squared distances on the CUDA device; `None` without a device.
pub(crate) fn squared_distance_pairs_cuda(a: &[V3], b: &[V3]) -> Option<Vec<f64>> {
    SHARED_DISTANCE_PAIRS.with(|cell| {
        let shared: &Option<&CudaDistancePairs> = cell;
        shared.and_then(|kernel| kernel.run(a, b))
    })
}

struct CudaDistancePairSum {
    device: CudaDevice,
    kernel: CudaFunction,
    buffers: std::cell::RefCell<Option<DistancePairSumBuffers>>,
}

struct DistancePairSumBuffers {
    pair_capacity: usize,
    partial_capacity: usize,
    a: CudaSlice<f32>,
    b: CudaSlice<f32>,
    out: CudaSlice<f32>,
}

impl CudaDistancePairSum {
    fn new() -> Option<Self> {
        let device = CudaDevice::new()?;
        let module = device.load_ptx(DISTANCE_PAIR_SUM_PTX)?;
        let kernel = module.load_function("squared_distance_pair_sum").ok()?;
        Some(Self {
            device,
            kernel,
            buffers: std::cell::RefCell::new(None),
        })
    }

    fn ensure_buffers(&self, pair_count: usize, partial_count: usize) -> Option<()> {
        let stale = match &*self.buffers.borrow() {
            Some(b) => b.pair_capacity < pair_count || b.partial_capacity < partial_count,
            None => true,
        };
        if !stale {
            return Some(());
        }
        let pair_capacity = pair_count.max(1);
        let partial_capacity = partial_count.max(1);
        let stream = &self.device.stream;
        let a = stream.alloc_zeros::<f32>(pair_capacity * 3).ok()?;
        let b = stream.alloc_zeros::<f32>(pair_capacity * 3).ok()?;
        let out = stream.alloc_zeros::<f32>(partial_capacity).ok()?;
        *self.buffers.borrow_mut() = Some(DistancePairSumBuffers {
            pair_capacity,
            partial_capacity,
            a,
            b,
            out,
        });
        Some(())
    }

    fn run(&self, a: &[V3], b: &[V3]) -> Option<f64> {
        if a.len() != b.len() {
            return None;
        }
        let pair_count = a.len();
        let partial_count = pair_count.div_ceil(BLOCK as usize).max(1);
        let stream = &self.device.stream;
        let flat_a: Vec<f32> = a.iter().flatten().map(|&v| v as f32).collect();
        let flat_b: Vec<f32> = b.iter().flatten().map(|&v| v as f32).collect();
        self.ensure_buffers(pair_count, partial_count)?;
        let mut buffers = self.buffers.borrow_mut();
        let buffers = buffers.as_mut().expect("ensure_buffers was just called");
        {
            let mut a_view = buffers.a.slice_mut(0..(pair_count * 3).max(1));
            stream.memcpy_htod(&flat_a, &mut a_view).ok()?;
        }
        {
            let mut b_view = buffers.b.slice_mut(0..(pair_count * 3).max(1));
            stream.memcpy_htod(&flat_b, &mut b_view).ok()?;
        }
        let pair_count_u32 = pair_count as u32;
        let a_view = buffers.a.slice(0..(pair_count * 3).max(1));
        let b_view = buffers.b.slice(0..(pair_count * 3).max(1));
        let mut out_view = buffers.out.slice_mut(0..partial_count);
        let mut launch = stream.launch_builder(&self.kernel);
        launch
            .arg(&pair_count_u32)
            .arg(&a_view)
            .arg(&b_view)
            .arg(&mut out_view);
        unsafe { launch.launch(launch_1d(pair_count_u32, BLOCK)) }.ok()?;
        let mut partials = vec![0f32; partial_count];
        stream.memcpy_dtoh(&out_view, &mut partials).ok()?;
        Some(partials.into_iter().map(|value| value as f64).sum())
    }
}

thread_local! {
    static SHARED_DISTANCE_PAIR_SUM: std::cell::LazyCell<Option<&'static CudaDistancePairSum>> =
        std::cell::LazyCell::new(|| {
            CudaDistancePairSum::new().map(|kernel| Box::leak(Box::new(kernel)) as &'static CudaDistancePairSum)
        });
}

/// Sum of one-to-one squared distances on the CUDA device; `None` without a device.
pub(crate) fn squared_distance_pair_sum_cuda(a: &[V3], b: &[V3]) -> Option<f64> {
    SHARED_DISTANCE_PAIR_SUM.with(|cell| {
        let shared: &Option<&CudaDistancePairSum> = cell;
        shared.and_then(|kernel| kernel.run(a, b))
    })
}

struct CudaTransformedDistancePairSum {
    device: CudaDevice,
    kernel: CudaFunction,
    buffers: std::cell::RefCell<Option<TransformedDistancePairSumBuffers>>,
}

struct TransformedDistancePairSumBuffers {
    pair_capacity: usize,
    partial_capacity: usize,
    source: CudaSlice<f32>,
    target: CudaSlice<f32>,
    transform: CudaSlice<f32>,
    out: CudaSlice<f32>,
}

impl CudaTransformedDistancePairSum {
    fn new() -> Option<Self> {
        let device = CudaDevice::new()?;
        let module = device.load_ptx(TRANSFORMED_DISTANCE_PAIR_SUM_PTX)?;
        let kernel = module
            .load_function("transformed_squared_distance_pair_sum")
            .ok()?;
        Some(Self {
            device,
            kernel,
            buffers: std::cell::RefCell::new(None),
        })
    }

    fn ensure_buffers(&self, pair_count: usize, partial_count: usize) -> Option<()> {
        let stale = match &*self.buffers.borrow() {
            Some(b) => b.pair_capacity < pair_count || b.partial_capacity < partial_count,
            None => true,
        };
        if !stale {
            return Some(());
        }
        let pair_capacity = pair_count.max(1);
        let partial_capacity = partial_count.max(1);
        let stream = &self.device.stream;
        let source = stream.alloc_zeros::<f32>(pair_capacity * 3).ok()?;
        let target = stream.alloc_zeros::<f32>(pair_capacity * 3).ok()?;
        let transform = stream.alloc_zeros::<f32>(12).ok()?;
        let out = stream.alloc_zeros::<f32>(partial_capacity).ok()?;
        *self.buffers.borrow_mut() = Some(TransformedDistancePairSumBuffers {
            pair_capacity,
            partial_capacity,
            source,
            target,
            transform,
            out,
        });
        Some(())
    }

    fn run(&self, source: &[V3], target: &[V3], m: M3, t: V3) -> Option<f64> {
        if source.len() != target.len() {
            return None;
        }
        let pair_count = source.len();
        let partial_count = pair_count.div_ceil(BLOCK as usize).max(1);
        let stream = &self.device.stream;
        let flat_source: Vec<f32> = source.iter().flatten().map(|&v| v as f32).collect();
        let flat_target: Vec<f32> = target.iter().flatten().map(|&v| v as f32).collect();
        let mut transform = Vec::with_capacity(12);
        for row in 0..3 {
            transform.extend(m[row].map(|v| v as f32));
            transform.push(t[row] as f32);
        }
        self.ensure_buffers(pair_count, partial_count)?;
        let mut buffers = self.buffers.borrow_mut();
        let buffers = buffers.as_mut().expect("ensure_buffers was just called");
        {
            let mut source_view = buffers.source.slice_mut(0..(pair_count * 3).max(1));
            stream.memcpy_htod(&flat_source, &mut source_view).ok()?;
        }
        {
            let mut target_view = buffers.target.slice_mut(0..(pair_count * 3).max(1));
            stream.memcpy_htod(&flat_target, &mut target_view).ok()?;
        }
        {
            let mut transform_view = buffers.transform.slice_mut(0..12);
            stream.memcpy_htod(&transform, &mut transform_view).ok()?;
        }
        let pair_count_u32 = pair_count as u32;
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
        unsafe { launch.launch(launch_1d(pair_count_u32, BLOCK)) }.ok()?;
        let mut partials = vec![0f32; partial_count];
        stream.memcpy_dtoh(&out_view, &mut partials).ok()?;
        Some(partials.into_iter().map(|value| value as f64).sum())
    }
}

thread_local! {
    static SHARED_TRANSFORMED_DISTANCE_PAIR_SUM: std::cell::LazyCell<Option<&'static CudaTransformedDistancePairSum>> =
        std::cell::LazyCell::new(|| {
            CudaTransformedDistancePairSum::new().map(|kernel| Box::leak(Box::new(kernel)) as &'static CudaTransformedDistancePairSum)
        });
}

/// Fused transform-and-sum of one-to-one squared distances on CUDA; `None` without a device.
pub(crate) fn transformed_squared_distance_pair_sum_cuda(
    source: &[V3],
    target: &[V3],
    m: M3,
    t: V3,
) -> Option<f64> {
    SHARED_TRANSFORMED_DISTANCE_PAIR_SUM.with(|cell| {
        let shared: &Option<&CudaTransformedDistancePairSum> = cell;
        shared.and_then(|kernel| kernel.run(source, target, m, t))
    })
}
