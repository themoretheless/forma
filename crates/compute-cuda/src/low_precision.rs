use crate::{
    CudaError, CudaRuntime, CudaTensor,
    indexing::{dispatch::CopyDispatch, output},
    low_dispatch::LowCastDispatch,
};
use gpu_compute::cuda::{CudaFunction, CudaModule};
use std::sync::Arc;
use tensor_core::{
    HasLowDtype, HasShape, Layout, LowDtype, LowPrecisionSupport, LowStorage, MatmulPlan, Shape,
    TensorError, TensorLowBackend,
};

/// Native two-byte CUDA allocation with an explicit floating-point format.
/// Clones and layout transformations share its storage; raw reads preserve bits.
#[derive(Clone)]
pub struct CudaLowTensor {
    pub(crate) tensor: CudaTensor<u16>,
    pub(crate) dtype: LowDtype,
}
impl CudaLowTensor {
    pub fn shape(&self) -> &Shape {
        self.tensor.shape()
    }
    pub fn layout(&self) -> &Layout {
        self.tensor.layout()
    }
    pub fn low_dtype(&self) -> LowDtype {
        self.dtype
    }
    /// Size of the shared allocation, including empty sentinels and any trailing
    /// alignment padding used by raw scatter's halfword-preserving CAS.
    pub fn storage_bytes(&self) -> usize {
        self.tensor.storage.len() * 2
    }
}
impl HasShape for CudaLowTensor {
    fn shape(&self) -> &Shape {
        self.shape()
    }
}
impl HasLowDtype for CudaLowTensor {
    fn low_dtype(&self) -> LowDtype {
        self.dtype
    }
}

#[derive(Clone)]
pub(crate) struct LowKernels {
    pub(crate) copy: CudaFunction,
    pub(crate) encode: CudaFunction,
    pub(crate) decode: CudaFunction,
    pub(crate) unary: CudaFunction,
    pub(crate) binary: CudaFunction,
    pub(crate) reduce_axes: CudaFunction,
    pub(crate) reduce_all: CudaFunction,
    pub(crate) compare: CudaFunction,
    pub(crate) select: CudaFunction,
    pub(crate) gather: CudaFunction,
    pub(crate) compact: CudaFunction,
    pub(crate) scan: CudaFunction,
    pub(crate) scatter_raw: CudaFunction,
    pub(crate) scatter_f32: CudaFunction,
}
impl LowKernels {
    pub(crate) fn load(module: &Arc<CudaModule>) -> Result<Self, CudaError> {
        Ok(Self {
            copy: module.load_function("copy_low")?,
            encode: module.load_function("cast_to_low")?,
            decode: module.load_function("cast_from_low")?,
            unary: module.load_function("unary_low")?,
            binary: module.load_function("binary_low")?,
            reduce_axes: module.load_function("reduce_axes_low")?,
            reduce_all: module.load_function("reduce_all_low")?,
            compare: module.load_function("compare_low")?,
            select: module.load_function("where_low")?,
            gather: module.load_function("gather_low")?,
            compact: module.load_function("compact_low")?,
            scan: module.load_function("scan_low_f32")?,
            scatter_raw: module.load_function("scatter_low_raw")?,
            scatter_f32: module.load_function("scatter_low_f32")?,
        })
    }
}

fn matmul_supported(dtype: LowDtype, capability: (i32, i32), cublas: bool) -> bool {
    cublas
        && capability.0
            >= match dtype {
                LowDtype::F16 => 5,
                LowDtype::Bf16 => 8,
            }
}

impl CudaRuntime {
    pub(crate) fn validate_low_matmul(&self, dtype: LowDtype) -> Result<(), CudaError> {
        validate_matmul_support(
            dtype,
            self.capabilities.compute_capability,
            self.capabilities.cublas_available,
        )
    }
    fn low_view(&self, input: &CudaLowTensor, layout: Layout) -> Result<CudaLowTensor, CudaError> {
        Ok(CudaLowTensor {
            tensor: self.view(&input.tensor, layout)?,
            dtype: input.dtype,
        })
    }
    /// Zero-copy low-precision slice, using the same checked layout rules.
    pub fn narrow_low(
        &self,
        input: &CudaLowTensor,
        axis: usize,
        start: usize,
        len: usize,
    ) -> Result<CudaLowTensor, CudaError> {
        self.low_view(input, input.layout().narrow(axis, start, len)?)
    }
}

impl TensorLowBackend for CudaRuntime {
    type LowTensor = CudaLowTensor;
    fn low_precision_support(&self, dtype: LowDtype) -> LowPrecisionSupport {
        let supported = matmul_supported(
            dtype,
            self.capabilities.compute_capability,
            self.capabilities.cublas_available,
        );
        LowPrecisionSupport {
            storage: LowStorage::Native16,
            matmul: supported,
            matmul_f32: supported,
        }
    }
    fn upload_low(
        &self,
        dtype: LowDtype,
        shape: Shape,
        bits: &[u16],
    ) -> Result<CudaLowTensor, CudaError> {
        if shape.numel() != bits.len() {
            return Err(TensorError::ElementCountMismatch {
                expected: shape.numel(),
                actual: bits.len(),
            }
            .into());
        }
        let mut tensor = self.zeros_typed(shape)?;
        if !bits.is_empty() {
            self.device.stream.memcpy_htod(bits, output(&mut tensor))?;
        }
        Ok(CudaLowTensor { tensor, dtype })
    }
    fn read_low_bits(&self, input: &CudaLowTensor) -> Result<Vec<u16>, CudaError> {
        self.check(&input.tensor)?;
        if input.shape().is_empty() {
            return Ok(Vec::new());
        }
        let input = self.materialize_low(input)?;
        let start = input.layout().offset();
        Ok(self.device.stream.clone_dtoh(
            &input
                .tensor
                .storage
                .slice(start..start + input.shape().numel()),
        )?)
    }
    fn materialize_low(&self, input: &CudaLowTensor) -> Result<CudaLowTensor, CudaError> {
        self.check(&input.tensor)?;
        if input.layout().is_contiguous() {
            return Ok(input.clone());
        }
        let pass = CopyDispatch::new::<u16>(input.layout())?;
        let mut tensor = self.zeros_typed(input.shape().clone())?;
        if !input.shape().is_empty() {
            let metadata = self.metadata(&pass.metadata)?;
            pass.launch(
                self,
                &self.low.copy,
                input.tensor.storage.as_ref(),
                output(&mut tensor),
                &metadata,
            )?;
        }
        Ok(CudaLowTensor {
            tensor,
            dtype: input.dtype,
        })
    }
    fn reshape_low(&self, input: &CudaLowTensor, shape: Shape) -> Result<CudaLowTensor, CudaError> {
        self.check(&input.tensor)?;
        if shape.numel() != input.shape().numel() {
            return Err(TensorError::ElementCountMismatch {
                expected: input.shape().numel(),
                actual: shape.numel(),
            }
            .into());
        }
        let input = self.materialize_low(input)?;
        self.low_view(&input, input.layout().reshape(shape)?)
    }
    fn permute_low(
        &self,
        input: &CudaLowTensor,
        axes: &[usize],
    ) -> Result<CudaLowTensor, CudaError> {
        self.low_view(input, input.layout().permute(axes)?)
    }
    fn broadcast_low(
        &self,
        input: &CudaLowTensor,
        shape: Shape,
    ) -> Result<CudaLowTensor, CudaError> {
        self.low_view(input, input.layout().broadcast_to(shape)?)
    }
    fn cast_to_low(&self, input: &CudaTensor, dtype: LowDtype) -> Result<CudaLowTensor, CudaError> {
        self.check(input)?;
        let pass = LowCastDispatch::new(input.layout(), dtype)?;
        let mut tensor = self.zeros_typed(input.shape().clone())?;
        if !input.shape().is_empty() {
            let metadata = self.metadata(&pass.geometry.metadata)?;
            pass.launch_encode(self, input.storage.as_ref(), output(&mut tensor), &metadata)?;
        }
        Ok(CudaLowTensor { tensor, dtype })
    }
    fn cast_to_f32(&self, input: &CudaLowTensor) -> Result<CudaTensor, CudaError> {
        self.check(&input.tensor)?;
        let pass = LowCastDispatch::new(input.layout(), input.dtype)?;
        let mut tensor = self.zeros(input.shape().clone())?;
        if !input.shape().is_empty() {
            let metadata = self.metadata(&pass.geometry.metadata)?;
            pass.launch_decode(
                self,
                input.tensor.storage.as_ref(),
                output(&mut tensor),
                &metadata,
            )?;
        }
        Ok(tensor)
    }
    fn matmul_low(
        &self,
        left: &CudaLowTensor,
        right: &CudaLowTensor,
    ) -> Result<CudaLowTensor, CudaError> {
        // Exactly one low-format rounding, after the entire f32 accumulation.
        // Inputs remain native16; only the result has a temporary f32 allocation.
        let product = self.matmul_low_f32(left, right)?;
        self.cast_to_low(&product, left.dtype)
    }
    fn matmul_low_f32(
        &self,
        left: &CudaLowTensor,
        right: &CudaLowTensor,
    ) -> Result<CudaTensor, CudaError> {
        self.check(&left.tensor)?;
        self.check(&right.tensor)?;
        if left.dtype != right.dtype {
            return Err(CudaError::InvalidInput(
                "low GEMM inputs must have the same dtype",
            ));
        }
        let plan = MatmulPlan::new(left.shape(), right.shape())?;
        self.validate_low_matmul(left.dtype)?;
        if plan.matrix_output.is_empty() || *plan.left.dims().last().unwrap() == 0 {
            return self.zeros(plan.output);
        }
        let left = self.reshape_low(left, plan.left)?;
        let right = self.reshape_low(right, plan.right)?;
        let product = self.gemm_dense_low(&left.tensor, &right.tensor, left.dtype)?;
        self.view(&product, product.layout.reshape(plan.output)?)
    }
}

fn validate_matmul_support(
    dtype: LowDtype,
    capability: (i32, i32),
    cublas: bool,
) -> Result<(), CudaError> {
    if !matmul_supported(dtype, capability, cublas) {
        return Err(CudaError::UnsupportedPrecision(
            "native low GEMM requires cuBLAS and SM50 for f16 / SM80 for bf16",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_low_gemm_support_is_explicit_and_storage_is_two_bytes() {
        assert_eq!(std::mem::size_of::<u16>(), 2);
        assert!(!matmul_supported(LowDtype::F16, (4, 0), true));
        assert!(matmul_supported(LowDtype::F16, (5, 0), true));
        assert!(!matmul_supported(LowDtype::Bf16, (7, 5), true));
        assert!(matmul_supported(LowDtype::Bf16, (8, 0), true));
        assert!(!matmul_supported(LowDtype::Bf16, (12, 0), false));
        // The replay preflight uses this same check even when an empty/K=0
        // plan has no GEMM instruction left in its launch schedule.
        for (dtype, capability, available, supported) in [
            (LowDtype::F16, (4, 9), true, false),
            (LowDtype::F16, (5, 0), true, true),
            (LowDtype::Bf16, (7, 9), true, false),
            (LowDtype::Bf16, (8, 0), true, true),
            (LowDtype::F16, (9, 0), false, false),
            (LowDtype::Bf16, (9, 0), false, false),
        ] {
            assert_eq!(
                validate_matmul_support(dtype, capability, available).is_ok(),
                supported
            );
        }
    }
}
