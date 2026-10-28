use crate::{CudaError, CudaMatmulPolicy, CudaRuntime, CudaTensor};
use gpu_compute::cuda::{
    CudaSlice, CudaStream,
    cudarc::{
        cublas::{CudaBlas, result, sys},
        driver::{DevicePtr, DevicePtrMut, DeviceRepr},
    },
};
use std::sync::Arc;
use tensor_core::{LowDtype, MatmulPlan, MatmulPrecision, TensorBackend};

mod capture;
mod plan;
pub(crate) use capture::{
    CaptureBlas, capture_default_workspace_bytes, capture_workspace_allocation_bytes,
};
pub(crate) use plan::GemmPlan;

/// A checked descriptor and numerical mode retained on its preparation stream.
/// Preparation initializes cuBLAS; replay only binds the current allocations.
#[derive(Clone, Debug)]
pub(crate) struct PreparedGemm {
    plan: GemmPlan,
    input_type: sys::cudaDataType_t,
    compute_type: sys::cublasComputeType_t,
    stream: Arc<CudaStream>,
    capture_blas: Option<Arc<CaptureBlas>>,
}
impl PreparedGemm {
    pub(crate) fn plan(&self) -> &GemmPlan {
        &self.plan
    }
    /// Attach the graph-private handle before warmup/capture. Precision and
    /// input storage modes stay those validated by ordinary preparation.
    pub(crate) fn attach_capture_blas(&mut self, owner: Arc<CaptureBlas>) -> Result<(), CudaError> {
        if owner.stream() != &self.stream {
            return Err(CudaError::InvalidInput(
                "capture cuBLAS stream differs from prepared GEMM",
            ));
        }
        self.capture_blas = Some(owner);
        Ok(())
    }
}

impl CudaRuntime {
    pub fn matmul_policy(&self, precision: MatmulPrecision) -> Result<CudaMatmulPolicy, CudaError> {
        static TF32_OVERRIDE: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
        let tf32_override = TF32_OVERRIDE
            .get_or_init(|| std::env::var("NVIDIA_TF32_OVERRIDE").ok())
            .as_deref();
        CudaMatmulPolicy::for_device(
            precision,
            self.capabilities.compute_capability,
            tf32_override,
        )
    }
    pub(crate) fn matmul_impl(
        &self,
        a: &CudaTensor,
        b: &CudaTensor,
        precision: MatmulPrecision,
    ) -> Result<CudaTensor, CudaError> {
        self.check(a)?;
        self.check(b)?;
        let shape = MatmulPlan::new(a.shape(), b.shape())?;
        // Preserve policy validation before empty shortcuts or materialization.
        self.matmul_policy(precision)?;
        if shape.matrix_output.is_empty() || *shape.left.dims().last().unwrap() == 0 {
            return self.zeros(shape.output);
        }
        let a = self.reshape(a, shape.left)?;
        let b = self.reshape(b, shape.right)?;
        let plan = GemmPlan::new(&a.layout, &b.layout)?;
        let prepared = self.prepare_gemm_f32(plan, precision)?;
        let mut output = self.zeros(prepared.plan().output().clone())?;
        self.launch_gemm_f32(
            &prepared,
            a.storage.as_ref(),
            b.storage.as_ref(),
            Arc::get_mut(&mut output.storage).unwrap(),
        )?;
        self.view(&output, output.layout.reshape(shape.output)?)
    }

    /// The executor materializes and rank-promotes operands in its own schedule.
    /// This step checks precision even for empty/K=0 plans. Nonempty work obtains
    /// the cuBLAS handle before any recorded replay can occur.
    pub(crate) fn prepare_gemm_f32(
        &self,
        plan: GemmPlan,
        precision: MatmulPrecision,
    ) -> Result<PreparedGemm, CudaError> {
        let policy = self.matmul_policy(precision)?;
        self.prepare_gemm(plan, sys::cudaDataType_t::CUDA_R_32F, policy.compute_type)
    }

    /// No explicit device allocation, metadata upload or cuBLAS initialization.
    /// The caller validates runtime ownership and output aliases before enqueue.
    /// A zero plan does not write: its nonempty output needs a scheduled fill.
    pub(crate) fn launch_gemm_f32(
        &self,
        prepared: &PreparedGemm,
        left: &CudaSlice<f32>,
        right: &CudaSlice<f32>,
        output: &mut CudaSlice<f32>,
    ) -> Result<(), CudaError> {
        self.launch_gemm(prepared, left, right, output)
    }

    /// Native low operands accumulate directly into f32. Capability validation
    /// also applies to empty and K=0 plans; no conversion buffers are created.
    pub(crate) fn prepare_gemm_low(
        &self,
        plan: GemmPlan,
        dtype: LowDtype,
    ) -> Result<PreparedGemm, CudaError> {
        self.validate_low_matmul(dtype)?;
        self.prepare_gemm(
            plan,
            low_input_type(dtype),
            sys::cublasComputeType_t::CUBLAS_COMPUTE_32F,
        )
    }

    pub(crate) fn launch_gemm_low(
        &self,
        prepared: &PreparedGemm,
        left: &CudaSlice<u16>,
        right: &CudaSlice<u16>,
        output: &mut CudaSlice<f32>,
    ) -> Result<(), CudaError> {
        let dtype = match prepared.input_type {
            sys::cudaDataType_t::CUDA_R_16F => LowDtype::F16,
            sys::cudaDataType_t::CUDA_R_16BF => LowDtype::Bf16,
            _ => return Err(CudaError::InvalidInput("GEMM descriptor has no low dtype")),
        };
        self.validate_low_matmul(dtype)?;
        self.launch_gemm(prepared, left, right, output)
    }

    /// Eager native two-byte storage uses the same preparation and cuBLAS
    /// launch implementation as prepared low programs, accumulating into f32.
    pub(crate) fn gemm_dense_low(
        &self,
        a: &CudaTensor<u16>,
        b: &CudaTensor<u16>,
        dtype: LowDtype,
    ) -> Result<CudaTensor, CudaError> {
        self.check(a)?;
        self.check(b)?;
        let plan = GemmPlan::new(&a.layout, &b.layout)?;
        let prepared = self.prepare_gemm_low(plan, dtype)?;
        let mut output = self.zeros(prepared.plan().output().clone())?;
        self.launch_gemm_low(
            &prepared,
            a.storage.as_ref(),
            b.storage.as_ref(),
            Arc::get_mut(&mut output.storage).unwrap(),
        )?;
        Ok(output)
    }

    pub(crate) fn prepare_gemm(
        &self,
        plan: GemmPlan,
        input_type: sys::cudaDataType_t,
        compute_type: sys::cublasComputeType_t,
    ) -> Result<PreparedGemm, CudaError> {
        if !plan.is_zero() && self.blas.get().is_none() {
            crate::libraries::require_cublas()?;
            let blas = CudaBlas::new(self.device.stream.clone())
                .map_err(|e| CudaError::Blas(e.to_string()))?;
            let _ = self.blas.set(blas);
        }
        Ok(PreparedGemm {
            plan,
            input_type,
            compute_type,
            stream: self.device.stream.clone(),
            capture_blas: None,
        })
    }

    pub(crate) fn launch_gemm<T: DeviceRepr, O: GemmOutput>(
        &self,
        prepared: &PreparedGemm,
        left: &CudaSlice<T>,
        right: &CudaSlice<T>,
        output: &mut CudaSlice<O>,
    ) -> Result<(), CudaError> {
        validate_input_width::<T>(prepared.input_type)?;
        O::validate(prepared.input_type, prepared.compute_type)?;
        crate::runtime::validate_logical_size::<O>(prepared.plan.output())?;
        if prepared.stream != self.device.stream
            || left.context() != &self.device.context
            || right.context() != &self.device.context
            || output.context() != &self.device.context
        {
            return Err(CudaError::InvalidInput(
                "prepared GEMM stream and storage context must match",
            ));
        }
        prepared
            .plan
            .validate_storage(left.len(), right.len(), output.len())?;
        let Some(dimensions) = prepared.plan.dimensions() else {
            return Ok(());
        };
        let capture_handle = prepared
            .capture_blas
            .as_ref()
            .map(|owner| owner.lock())
            .transpose()?;
        let handle = match &capture_handle {
            Some(guard) => guard.raw(),
            None => *self
                .blas
                .get()
                .ok_or(CudaError::InvalidInput(
                    "GEMM was not prepared on this runtime",
                ))?
                .handle(),
        };
        self.device.context.bind_to_thread()?;
        let alpha = O::ONE;
        let beta = O::ZERO;
        for index in 0..prepared.plan.calls() {
            let batch = prepared.plan.batch(index)?;
            let a = left.slice(batch.left);
            let b = right.slice(batch.right);
            let mut out = output.slice_mut(batch.output);
            let (a_ptr, _a_guard) = a.device_ptr(&self.device.stream);
            let (b_ptr, _b_guard) = b.device_ptr(&self.device.stream);
            let (out_ptr, _out_guard) = out.device_ptr_mut(&self.device.stream);
            // The plan proves every view bound before the first call. Row-major
            // C=A*B is C^T=B^T*A^T in cuBLAS's column-major convention. Retained
            // guards register accesses on the same stream as eager execution.
            unsafe {
                result::gemm_ex(
                    handle,
                    sys::cublasOperation_t::CUBLAS_OP_N,
                    sys::cublasOperation_t::CUBLAS_OP_N,
                    dimensions.m,
                    dimensions.n,
                    dimensions.k,
                    (&alpha as *const O).cast(),
                    b_ptr as *const _,
                    prepared.input_type,
                    dimensions.lda,
                    a_ptr as *const _,
                    prepared.input_type,
                    dimensions.ldb,
                    (&beta as *const O).cast(),
                    out_ptr as *mut _,
                    O::DTYPE,
                    dimensions.ldc,
                    prepared.compute_type,
                    sys::cublasGemmAlgo_t::CUBLAS_GEMM_DEFAULT,
                )
                .map_err(|e| CudaError::Blas(e.to_string()))?;
            }
        }
        Ok(())
    }
}
fn low_input_type(dtype: LowDtype) -> sys::cudaDataType_t {
    match dtype {
        LowDtype::F16 => sys::cudaDataType_t::CUDA_R_16F,
        LowDtype::Bf16 => sys::cudaDataType_t::CUDA_R_16BF,
    }
}
fn validate_input_width<T>(input_type: sys::cudaDataType_t) -> Result<(), CudaError> {
    let width = match input_type {
        sys::cudaDataType_t::CUDA_R_32F => 4,
        sys::cudaDataType_t::CUDA_R_64F => 8,
        sys::cudaDataType_t::CUDA_R_16F | sys::cudaDataType_t::CUDA_R_16BF => 2,
        _ => return Err(CudaError::InvalidInput("unsupported GEMM input type")),
    };
    if std::mem::size_of::<T>() != width {
        return Err(CudaError::InvalidInput(
            "GEMM storage width does not match its declared type",
        ));
    }
    Ok(())
}

/// Private implementations keep cuBLAS scalar pointers, output storage and
/// compute mode coupled. A 64-bit input cannot accidentally use f32 alpha/beta.
pub(crate) trait GemmOutput: DeviceRepr {
    const ONE: Self;
    const ZERO: Self;
    const DTYPE: sys::cudaDataType_t;
    fn validate(
        input: sys::cudaDataType_t,
        compute: sys::cublasComputeType_t,
    ) -> Result<(), CudaError>;
}
impl GemmOutput for f32 {
    const ONE: Self = 1.;
    const ZERO: Self = 0.;
    const DTYPE: sys::cudaDataType_t = sys::cudaDataType_t::CUDA_R_32F;
    fn validate(
        input: sys::cudaDataType_t,
        compute: sys::cublasComputeType_t,
    ) -> Result<(), CudaError> {
        use sys::{cublasComputeType_t::*, cudaDataType_t::*};
        if matches!(input, CUDA_R_32F | CUDA_R_16F | CUDA_R_16BF)
            && matches!(
                compute,
                CUBLAS_COMPUTE_32F
                    | CUBLAS_COMPUTE_32F_PEDANTIC
                    | CUBLAS_COMPUTE_32F_FAST_TF32
                    | CUBLAS_COMPUTE_32F_FAST_16F
                    | CUBLAS_COMPUTE_32F_FAST_16BF
            )
        {
            Ok(())
        } else {
            Err(CudaError::InvalidInput(
                "GEMM f32 output requires f32 computation",
            ))
        }
    }
}
impl GemmOutput for f64 {
    const ONE: Self = 1.;
    const ZERO: Self = 0.;
    const DTYPE: sys::cudaDataType_t = sys::cudaDataType_t::CUDA_R_64F;
    fn validate(
        input: sys::cudaDataType_t,
        compute: sys::cublasComputeType_t,
    ) -> Result<(), CudaError> {
        if input == Self::DTYPE && compute == sys::cublasComputeType_t::CUBLAS_COMPUTE_64F_PEDANTIC
        {
            Ok(())
        } else {
            Err(CudaError::InvalidInput(
                "GEMM f64 output requires native f64 inputs and pedantic computation",
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn gemm_storage_types_match_the_cublas_abi() {
        use sys::cudaDataType_t::*;
        assert!(validate_input_width::<f32>(CUDA_R_32F).is_ok());
        assert!(validate_input_width::<f64>(CUDA_R_64F).is_ok());
        assert!(validate_input_width::<f32>(CUDA_R_64F).is_err());
        assert!(validate_input_width::<f64>(CUDA_R_32F).is_err());
        use sys::cublasComputeType_t::*;
        assert!(<f64 as GemmOutput>::validate(CUDA_R_64F, CUBLAS_COMPUTE_64F_PEDANTIC).is_ok());
        assert!(<f64 as GemmOutput>::validate(CUDA_R_64F, CUBLAS_COMPUTE_32F).is_err());
        assert!(<f64 as GemmOutput>::validate(CUDA_R_32F, CUBLAS_COMPUTE_64F_PEDANTIC).is_err());
        assert!(<f32 as GemmOutput>::validate(CUDA_R_64F, CUBLAS_COMPUTE_64F_PEDANTIC).is_err());
        assert!(validate_input_width::<u16>(CUDA_R_16F).is_ok());
        assert!(validate_input_width::<u16>(CUDA_R_16BF).is_ok());
        assert!(validate_input_width::<f32>(CUDA_R_16BF).is_err());
        assert!(validate_input_width::<u16>(CUDA_R_32F).is_err());
        assert!(validate_input_width::<f32>(CUDA_R_32I).is_err());
        assert_eq!(low_input_type(LowDtype::F16), CUDA_R_16F);
        assert_eq!(low_input_type(LowDtype::Bf16), CUDA_R_16BF);
    }
}
