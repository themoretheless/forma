use crate::{
    CudaError,
    libraries::{require_cublas, require_nvrtc},
};
use gpu_compute::cuda::{
    CudaDevice, CudaFunction, CudaSlice, CudaStreamMode, LaunchConfig, cudarc,
};
use std::{cell::OnceCell, sync::Arc};
use tensor_core::{
    BackendKind, BinaryOp, HasShape, Layout, MatmulPrecision, ReduceOp, Shape, TensorBackend,
    TensorError, TensorReduceBackend, UnaryOp,
};

pub const CUDA_KERNEL_SOURCE: &str = concat!(
    include_str!("kernels.cu"),
    include_str!("indexing.cu"),
    include_str!("reductions.cu"),
    include_str!("scatter.cu"),
    include_str!("low_precision.cu"),
    include_str!("statistics.cu"),
    include_str!("attention.cu"),
    include_str!("low_ops.cu"),
    include_str!("low_indexing.cu"),
    include_str!("low_scatter.cu"),
    include_str!("low_statistics.cu"),
    include_str!("low_attention.cu"),
    include_str!("convolution.cu"),
    include_str!("float64.cu"),
    include_str!("float64_indexing.cu")
);

#[derive(Clone, Debug)]
pub struct CudaCapabilities {
    pub name: String,
    pub ordinal: usize,
    pub compute_capability: (i32, i32),
    pub multiprocessors: u32,
    pub nvrtc_available: bool,
    pub cublas_available: bool,
}

/// Cloning creates a view of the same device allocation, with its own layout.
#[derive(Clone)]
pub struct CudaTensor<T = f32> {
    pub(crate) storage: Arc<CudaSlice<T>>,
    pub(crate) layout: Layout,
    owner: Arc<()>,
}
impl<T> CudaTensor<T> {
    pub fn shape(&self) -> &Shape {
        self.layout.shape()
    }
    pub fn layout(&self) -> &Layout {
        &self.layout
    }
}
impl<T> HasShape for CudaTensor<T> {
    fn shape(&self) -> &Shape {
        self.shape()
    }
}

/// An eager, ordered stream backend. Intermediates remain resident. Kernel
/// launches return without reading data; metadata uploads may synchronize the
/// stream through cudarc's host-memory lifetime protection.
pub struct CudaRuntime {
    pub(crate) device: CudaDevice,
    pub(crate) capabilities: CudaCapabilities,
    pub(crate) blas: OnceCell<cudarc::cublas::CudaBlas>,
    owner: Arc<()>,
    pub(crate) unary: CudaFunction,
    pub(crate) binary: CudaFunction,
    pub(crate) binary_u32: CudaFunction,
    pub(crate) reduction: crate::reduction::ReductionKernels,
    pub(crate) indexing: crate::indexing::IndexKernels,
    pub(crate) low: crate::low_precision::LowKernels,
    pub(crate) statistics: crate::statistics::StatisticsKernels,
    pub(crate) attention: crate::attention::AttentionKernel,
    pub(crate) convolution: crate::convolution::ConvKernels,
    pub(crate) float64: crate::float64::F64Kernels,
}
impl CudaRuntime {
    pub fn new() -> Result<Self, CudaError> {
        let device = CudaDevice::new().ok_or(CudaError::Unavailable(
            "CUDA driver/device is absent or initialization failed",
        ))?;
        Self::from_device(device)
    }
    /// Opt in to an explicit nonblocking execution stream.
    pub fn new_with_stream(mode: CudaStreamMode) -> Result<Self, CudaError> {
        let device = CudaDevice::new_with_stream(mode).ok_or(CudaError::Unavailable(
            "CUDA driver/device is absent or stream initialization failed",
        ))?;
        Self::from_device(device)
    }
    /// A private wrapper for the same primary CUDA context. Tracking is disabled
    /// only on this fresh Rust wrapper, before any private allocation exists.
    /// Graph storage never escapes; explicit event bridges order its use on the
    /// caller stream, and graph destruction synchronizes both streams.
    pub(crate) fn graph_runtime(&self) -> Result<Self, CudaError> {
        crate::libraries::require_graph_runtime()?;
        let context = cudarc::driver::CudaContext::new(self.capabilities.ordinal)?;
        if context != self.device.context {
            return Err(CudaError::InvalidInput(
                "CUDA Graph requires a primary context",
            ));
        }
        // SAFETY: no buffers/streams have been made with this private wrapper.
        // It is never exported or used outside the graph's owned lifetime.
        unsafe { context.disable_event_tracking() };
        let stream = context.new_stream()?;
        Ok(Self {
            device: CudaDevice {
                context,
                stream,
                name: self.device.name.clone(),
                multiprocessors: self.device.multiprocessors,
            },
            capabilities: self.capabilities.clone(),
            blas: OnceCell::new(),
            owner: Arc::new(()),
            unary: self.unary.clone(),
            binary: self.binary.clone(),
            binary_u32: self.binary_u32.clone(),
            reduction: self.reduction.clone(),
            indexing: self.indexing.clone(),
            low: self.low.clone(),
            statistics: self.statistics.clone(),
            attention: self.attention.clone(),
            convolution: self.convolution.clone(),
            float64: self.float64.clone(),
        })
    }
    pub fn from_device(device: CudaDevice) -> Result<Self, CudaError> {
        validate_device(&device)?;
        require_nvrtc()?;
        let (major, minor) = device.context.compute_capability()?;
        let mut count = 0;
        // The NVRTC library is present and the ABI receives live, sized outputs.
        unsafe { cudarc::nvrtc::sys::nvrtcGetNumSupportedArchs(&mut count) }
            .result()
            .map_err(|e| CudaError::Compilation(e.to_string()))?;
        if !(1..1024).contains(&count) {
            return Err(CudaError::Compilation(
                "invalid NVRTC architecture count".into(),
            ));
        }
        let mut architectures = vec![0i32; count as usize];
        unsafe { cudarc::nvrtc::sys::nvrtcGetSupportedArchs(architectures.as_mut_ptr()) }
            .result()
            .map_err(|e| CudaError::Compilation(e.to_string()))?;
        let arch = architectures
            .into_iter()
            .filter(|&a| a <= major * 10 + minor)
            .max()
            .ok_or(CudaError::Unavailable(
                "NVRTC has no PTX target compatible with this GPU",
            ))?;
        let ptx = cudarc::nvrtc::compile_ptx_with_opts(
            CUDA_KERNEL_SOURCE,
            cudarc::nvrtc::CompileOptions {
                fmad: Some(false),
                ftz: Some(false),
                prec_div: Some(true),
                prec_sqrt: Some(true),
                options: vec![format!("--gpu-architecture=compute_{arch}")],
                ..Default::default()
            },
        )
        .map_err(|error| CudaError::Compilation(error.to_string()))?;
        Self::load(device, ptx)
    }
    /// Loads precompiled kernels without using the NVRTC runtime library.
    ///
    /// # Safety
    /// PTX must implement every entry and parameter ABI in CUDA_KERNEL_SOURCE
    /// and obey the same memory bounds, stream and arithmetic contracts. An
    /// arbitrary PTX module can violate device memory safety.
    pub unsafe fn from_ptx(device: CudaDevice, ptx: &str) -> Result<Self, CudaError> {
        Self::load(device, cudarc::nvrtc::Ptx::from_src(ptx))
    }
    fn load(device: CudaDevice, ptx: cudarc::nvrtc::Ptx) -> Result<Self, CudaError> {
        validate_device(&device)?;
        let module = device.context.load_module(ptx)?;
        let capabilities = CudaCapabilities {
            name: device.name.clone(),
            ordinal: device.context.ordinal(),
            compute_capability: device.context.compute_capability()?,
            multiprocessors: device.multiprocessors,
            nvrtc_available: require_nvrtc().is_ok(),
            cublas_available: require_cublas().is_ok(),
        };
        Ok(Self {
            unary: module.load_function("unary")?,
            binary: module.load_function("binary")?,
            binary_u32: module.load_function("binary_u32")?,
            reduction: crate::reduction::ReductionKernels::load(&module)?,
            indexing: crate::indexing::IndexKernels::load(&module)?,
            low: crate::low_precision::LowKernels::load(&module)?,
            statistics: crate::statistics::StatisticsKernels::load(&module)?,
            attention: crate::attention::AttentionKernel::load(&module)?,
            convolution: crate::convolution::ConvKernels::load(&module)?,
            float64: crate::float64::F64Kernels::load(&module)?,
            owner: Arc::new(()),
            device,
            capabilities,
            blas: OnceCell::new(),
        })
    }
    pub fn capabilities(&self) -> &CudaCapabilities {
        &self.capabilities
    }
    pub fn synchronize(&self) -> Result<(), CudaError> {
        Ok(self.device.stream.synchronize()?)
    }
    pub(crate) fn check<T>(&self, tensor: &CudaTensor<T>) -> Result<(), CudaError> {
        if !Arc::ptr_eq(&self.owner, &tensor.owner) {
            return Err(CudaError::ForeignRuntime);
        }
        tensor.layout.validate_storage_len(tensor.storage.len())?;
        validate_logical_size::<T>(tensor.shape())?;
        Ok(())
    }
    pub(crate) fn zeros(&self, shape: Shape) -> Result<CudaTensor, CudaError> {
        self.zeros_typed(shape)
    }
    pub(crate) fn zeros_typed<T: cudarc::driver::DeviceRepr + cudarc::driver::ValidAsZeroBits>(
        &self,
        shape: Shape,
    ) -> Result<CudaTensor<T>, CudaError> {
        validate_logical_size::<T>(&shape)?;
        let storage = self.device.stream.alloc_zeros::<T>(shape.numel().max(1))?;
        Ok(CudaTensor {
            storage: Arc::new(storage),
            layout: Layout::contiguous(shape)?,
            owner: self.owner.clone(),
        })
    }
    pub(crate) fn view<T>(
        &self,
        tensor: &CudaTensor<T>,
        layout: Layout,
    ) -> Result<CudaTensor<T>, CudaError> {
        self.check(tensor)?;
        layout.validate_storage_len(tensor.storage.len())?;
        validate_logical_size::<T>(layout.shape())?;
        Ok(CudaTensor {
            storage: tensor.storage.clone(),
            layout,
            owner: self.owner.clone(),
        })
    }
    pub(crate) fn config(&self, count: usize) -> LaunchConfig {
        let groups = count
            .div_ceil(256)
            .min((self.device.multiprocessors as usize).saturating_mul(8))
            .max(1);
        LaunchConfig {
            grid_dim: (groups as u32, 1, 1),
            block_dim: (256, 1, 1),
            shared_mem_bytes: 0,
        }
    }
    pub(crate) fn metadata(&self, values: &[u64]) -> Result<CudaSlice<u64>, CudaError> {
        Ok(self.device.upload(values)?)
    }
    pub(crate) fn unary_op(
        &self,
        input: &CudaTensor,
        op: u32,
        scale: f32,
        bias: f32,
    ) -> Result<CudaTensor, CudaError> {
        self.check(input)?;
        let mut output = self.zeros(input.shape().clone())?;
        if input.shape().is_empty() {
            return Ok(output);
        }
        let pass = crate::dispatch::UnaryDispatch::new(&input.layout, op, scale, bias)?;
        let metadata = self.metadata(&pass.metadata)?;
        pass.launch(
            self,
            input.storage.as_ref(),
            Arc::get_mut(&mut output.storage).unwrap(),
            &metadata,
        )?;
        Ok(output)
    }
    /// Updates unique contiguous storage; live cloned views are
    /// rejected, so a mutable handle cannot silently overwrite another view.
    pub fn write_f32(&self, tensor: &mut CudaTensor, values: &[f32]) -> Result<(), CudaError> {
        self.check(tensor)?;
        if values.len() != tensor.shape().numel() {
            return Err(TensorError::ElementCountMismatch {
                expected: tensor.shape().numel(),
                actual: values.len(),
            }
            .into());
        }
        if !tensor.layout.is_contiguous() || tensor.layout.offset() != 0 {
            return Err(CudaError::SharedOutput);
        }
        let storage = Arc::get_mut(&mut tensor.storage).ok_or(CudaError::SharedOutput)?;
        if !values.is_empty() {
            self.device.stream.memcpy_htod(values, storage)?;
        }
        Ok(())
    }
    pub fn affine(
        &self,
        input: &CudaTensor,
        scale: f32,
        bias: f32,
    ) -> Result<CudaTensor, CudaError> {
        self.unary_op(input, 9, scale, bias)
    }
    /// A zero-copy slice along one axis. Views keep their allocation alive.
    pub fn narrow<T>(
        &self,
        tensor: &CudaTensor<T>,
        axis: usize,
        start: usize,
        len: usize,
    ) -> Result<CudaTensor<T>, CudaError> {
        self.view(tensor, tensor.layout.narrow(axis, start, len)?)
    }
    /// Resident composition; currently materializes the product before sum.
    pub fn dot(&self, left: &CudaTensor, right: &CudaTensor) -> Result<CudaTensor, CudaError> {
        if left.shape() != right.shape() {
            return Err(CudaError::InvalidInput("dot requires identical shapes"));
        }
        let product = self.binary(BinaryOp::Multiply, left, right)?;
        self.sum_axes(
            &product,
            &(0..product.shape().rank()).collect::<Vec<_>>(),
            false,
        )
    }
}
fn validate_device(device: &CudaDevice) -> Result<(), CudaError> {
    if &device.context != device.stream.context() {
        return Err(CudaError::InvalidInput(
            "CUDA stream and module context must match",
        ));
    }
    // Public CudaDevice fields can be constructed manually. Bound scheduling
    // metadata before multiplication and u32 conversion in every grid.
    if !(1..=u32::MAX / (256 * 8)).contains(&device.multiprocessors) {
        return Err(CudaError::InvalidInput("invalid CUDA multiprocessor count"));
    }
    Ok(())
}
pub(crate) fn validate_logical_size<T>(shape: &Shape) -> Result<(), CudaError> {
    // Also apply this to zero-stride broadcasts with tiny backing storage:
    // the limit ensures the u64 grid-stride loop cannot wrap after its tail.
    shape
        .numel()
        .max(1)
        .checked_mul(std::mem::size_of::<T>())
        .ok_or(CudaError::InvalidInput("CUDA tensor byte count overflows"))?;
    rank(shape)?;
    Ok(())
}
pub(crate) fn rank(shape: &Shape) -> Result<u32, CudaError> {
    u32::try_from(shape.rank())
        .map_err(|_| CudaError::InvalidInput("CUDA tensor rank exceeds u32::MAX"))
}
pub(crate) fn layout_metadata(layout: &Layout) -> Vec<u64> {
    let rank = layout.shape().rank();
    let mut meta = Vec::with_capacity(rank * 2);
    meta.extend(layout.shape().dims().iter().map(|&v| v as u64));
    meta.extend(layout.strides().iter().map(|&v| v as u64));
    meta
}
impl TensorBackend for CudaRuntime {
    type Tensor = CudaTensor;
    type Error = CudaError;
    fn kind(&self) -> BackendKind {
        BackendKind::Cuda
    }
    fn f64_support(&self) -> tensor_core::Float64Support {
        tensor_core::Float64Support::Native
    }
    fn upload_f32(&self, shape: Shape, values: &[f32]) -> Result<CudaTensor, CudaError> {
        if shape.numel() != values.len() {
            return Err(TensorError::ElementCountMismatch {
                expected: shape.numel(),
                actual: values.len(),
            }
            .into());
        }
        let mut tensor = self.zeros(shape)?;
        self.write_f32(&mut tensor, values)?;
        Ok(tensor)
    }
    fn read_f32(&self, tensor: &CudaTensor) -> Result<Vec<f32>, CudaError> {
        self.check(tensor)?;
        if tensor.shape().is_empty() {
            return Ok(Vec::new());
        }
        let contiguous = self.materialize(tensor)?;
        let start = contiguous.layout.offset();
        Ok(self.device.stream.clone_dtoh(
            &contiguous
                .storage
                .slice(start..start + contiguous.shape().numel()),
        )?)
    }
    fn materialize(&self, tensor: &CudaTensor) -> Result<CudaTensor, CudaError> {
        self.check(tensor)?;
        if tensor.layout.is_contiguous() {
            Ok(tensor.clone())
        } else {
            self.unary_op(tensor, 10, 1., 0.)
        }
    }
    fn reshape(&self, tensor: &CudaTensor, shape: Shape) -> Result<CudaTensor, CudaError> {
        self.check(tensor)?;
        if shape.numel() != tensor.shape().numel() {
            return Err(TensorError::ElementCountMismatch {
                expected: tensor.shape().numel(),
                actual: shape.numel(),
            }
            .into());
        }
        let contiguous = self.materialize(tensor)?;
        self.view(&contiguous, contiguous.layout.reshape(shape)?)
    }
    fn permute(&self, tensor: &CudaTensor, axes: &[usize]) -> Result<CudaTensor, CudaError> {
        self.view(tensor, tensor.layout.permute(axes)?)
    }
    fn broadcast_to(&self, tensor: &CudaTensor, shape: Shape) -> Result<CudaTensor, CudaError> {
        self.view(tensor, tensor.layout.broadcast_to(shape)?)
    }
    fn unary(&self, op: UnaryOp, input: &CudaTensor) -> Result<CudaTensor, CudaError> {
        self.unary_op(input, op as u32, 1., 0.)
    }
    fn binary(
        &self,
        op: BinaryOp,
        a: &CudaTensor,
        b: &CudaTensor,
    ) -> Result<CudaTensor, CudaError> {
        self.check(a)?;
        self.check(b)?;
        let pass = crate::dispatch::BinaryDispatch::new(&a.layout, &b.layout, op)?;
        let mut output = self.zeros(pass.shape.clone())?;
        if pass.shape.is_empty() {
            return Ok(output);
        }
        let metadata = self.metadata(&pass.metadata)?;
        pass.launch(
            self,
            a.storage.as_ref(),
            b.storage.as_ref(),
            Arc::get_mut(&mut output.storage).unwrap(),
            &metadata,
        )?;
        Ok(output)
    }
    fn sum_axes(
        &self,
        input: &CudaTensor,
        axes: &[usize],
        keep_dims: bool,
    ) -> Result<CudaTensor, CudaError> {
        self.reduce_f32(ReduceOp::Sum, input, axes, keep_dims)
    }
    fn matmul(
        &self,
        a: &CudaTensor,
        b: &CudaTensor,
        precision: MatmulPrecision,
    ) -> Result<CudaTensor, CudaError> {
        self.matmul_impl(a, b, precision)
    }
}

pub(crate) fn validate_storage(lengths: &[(usize, usize)]) -> Result<(), CudaError> {
    if lengths.iter().any(|&(actual, required)| actual < required) {
        return Err(CudaError::InvalidInput("dispatch storage is too small"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logical_broadcast_count_is_bounded_without_allocating() {
        assert!(validate_logical_size::<f32>(&Shape::new(vec![usize::MAX]).unwrap()).is_err());
        assert!(validate_logical_size::<f32>(&Shape::new(vec![0, usize::MAX]).unwrap()).is_ok());
        assert!(validate_logical_size::<f32>(&Shape::new(vec![]).unwrap()).is_ok());
        let fits_only_low = Shape::new(vec![usize::MAX / 2]).unwrap();
        assert!(validate_logical_size::<u16>(&fits_only_low).is_ok());
        assert!(validate_logical_size::<f32>(&fits_only_low).is_err());
    }
}
