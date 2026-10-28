use crate::CudaError;
use gpu_compute::cuda::cudarc::cublas::sys;
use tensor_core::MatmulPrecision;

/// Requested arithmetic and eligible hardware, not a claim that a selected
/// cuBLAS kernel actually issued Tensor Core instructions.
#[derive(Clone, Copy, Debug)]
pub struct CudaMatmulPolicy {
    pub precision: MatmulPrecision,
    pub tensor_cores_permitted: bool,
    pub compute_type: sys::cublasComputeType_t,
}
impl CudaMatmulPolicy {
    /// Checks eligibility without initializing CUDA, useful for deployment
    /// validation. `tf32_override` is NVIDIA_TF32_OVERRIDE from the environment.
    pub fn for_device(
        precision: MatmulPrecision,
        capability: (i32, i32),
        tf32_override: Option<&str>,
    ) -> Result<Self, CudaError> {
        use sys::cublasComputeType_t::*;
        let compute_type = match precision {
            MatmulPrecision::F32 => CUBLAS_COMPUTE_32F_PEDANTIC,
            MatmulPrecision::AllowTf32 => {
                if capability.0 < 8 {
                    return Err(CudaError::UnsupportedPrecision(
                        "TF32 requires compute capability 8.0 or newer",
                    ));
                }
                if tf32_override.is_some_and(|v| v.trim() == "0") {
                    return Err(CudaError::UnsupportedPrecision(
                        "NVIDIA_TF32_OVERRIDE=0 disables the requested TF32 mode",
                    ));
                }
                CUBLAS_COMPUTE_32F_FAST_TF32
            }
            MatmulPrecision::AllowF16 => {
                if capability.0 < 7 {
                    return Err(CudaError::UnsupportedPrecision(
                        "FP16 Tensor Core mode requires compute capability 7.0 or newer",
                    ));
                }
                CUBLAS_COMPUTE_32F_FAST_16F
            }
            MatmulPrecision::AllowBf16 => {
                if capability.0 < 8 {
                    return Err(CudaError::UnsupportedPrecision(
                        "BF16 Tensor Core mode requires compute capability 8.0 or newer",
                    ));
                }
                CUBLAS_COMPUTE_32F_FAST_16BF
            }
        };
        Ok(Self {
            precision,
            tensor_cores_permitted: !matches!(precision, MatmulPrecision::F32),
            compute_type,
        })
    }
}

/// cuBLAS is column-major. Row-major C=A*B is C^T=B^T*A^T,
/// so swap inputs and output dimensions without a device transpose.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct GemmDimensions {
    pub m: i32,
    pub n: i32,
    pub k: i32,
    pub lda: i32,
    pub ldb: i32,
    pub ldc: i32,
}
pub(crate) fn row_major_gemm(
    rows: usize,
    inner: usize,
    columns: usize,
) -> Result<GemmDimensions, CudaError> {
    let dim = |x| {
        i32::try_from(x).map_err(|_| CudaError::InvalidInput("cuBLAS dimension exceeds i32::MAX"))
    };
    Ok(GemmDimensions {
        m: dim(columns)?,
        n: dim(rows)?,
        k: dim(inner)?,
        lda: dim(columns.max(1))?,
        ldb: dim(inner.max(1))?,
        ldc: dim(columns.max(1))?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn precision_is_explicit_and_unsupported_modes_do_not_fall_back() {
        use sys::cublasComputeType_t::*;
        assert_eq!(
            CudaMatmulPolicy::for_device(MatmulPrecision::F32, (6, 1), None)
                .unwrap()
                .compute_type,
            CUBLAS_COMPUTE_32F_PEDANTIC
        );
        assert_eq!(
            CudaMatmulPolicy::for_device(MatmulPrecision::AllowTf32, (8, 0), None)
                .unwrap()
                .compute_type,
            CUBLAS_COMPUTE_32F_FAST_TF32
        );
        assert!(CudaMatmulPolicy::for_device(MatmulPrecision::AllowTf32, (7, 5), None).is_err());
        assert!(
            CudaMatmulPolicy::for_device(MatmulPrecision::AllowTf32, (9, 0), Some("0")).is_err()
        );
        assert!(CudaMatmulPolicy::for_device(MatmulPrecision::AllowF16, (6, 1), None).is_err());
        assert!(CudaMatmulPolicy::for_device(MatmulPrecision::AllowF16, (7, 0), None).is_ok());
        assert!(CudaMatmulPolicy::for_device(MatmulPrecision::AllowBf16, (7, 5), None).is_err());
        assert!(CudaMatmulPolicy::for_device(MatmulPrecision::AllowBf16, (8, 0), None).is_ok());
    }
    #[test]
    fn rectangular_row_major_layout_and_integer_limits() {
        assert_eq!(
            row_major_gemm(3, 5, 7).unwrap(),
            GemmDimensions {
                m: 7,
                n: 3,
                k: 5,
                lda: 7,
                ldb: 5,
                ldc: 7
            }
        );
        assert!(row_major_gemm(i32::MAX as usize + 1, 1, 1).is_err());
        assert_eq!(row_major_gemm(0, 0, 0).unwrap().ldc, 1);
    }
}
