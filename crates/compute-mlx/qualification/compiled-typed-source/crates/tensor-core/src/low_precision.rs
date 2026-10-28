use crate::{HasShape, Shape, TensorBackend, TensorError};

/// Two-byte floating-point storage. This is independent of policies that permit
/// reduced multiplication precision for tensors stored as f32.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum LowDtype {
    F16,
    Bf16,
}

pub trait HasLowDtype: HasShape {
    fn low_dtype(&self) -> LowDtype;
}

pub(crate) fn check_low_dtypes(
    left: &impl HasLowDtype,
    right: &impl HasLowDtype,
) -> Result<(), TensorError> {
    if left.low_dtype() != right.low_dtype() {
        return Err(TensorError::LowDtypeMismatch {
            left: left.low_dtype(),
            right: right.low_dtype(),
        });
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LowStorage {
    /// Two logical 16-bit elements per u32, with one padding lane for odd sizes.
    Packed16x2,
    /// Native two-byte array elements.
    Native16,
}

/// Storage/casts are provided for both dtypes. Matrix support is reported per
/// dtype and device. A supported product consumes the low-precision inputs
/// directly and accumulates in f32; it does not expand whole inputs to f32.
/// These flags do not prove use of a particular hardware instruction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LowPrecisionSupport {
    pub storage: LowStorage,
    /// f32 accumulation with one final rounding into the input dtype.
    pub matmul: bool,
    /// Direct low-precision inputs and f32 output, without low-output rounding.
    pub matmul_f32: bool,
}

/// Device-resident f16/bf16 storage, views, conversions and matrix products.
///
/// Raw upload/read and view materialization preserve every 16-bit pattern,
/// including NaN payloads and signed zeros. Casts use round-to-nearest, ties-to-
/// even, preserve signed zero and subnormals, and overflow to signed infinity.
/// NaN payload/sign preservation is not required for casts, but NaNs remain NaNs.
/// Reshape follows logical row-major order and materializes when necessary.
///
/// Products require matching input dtypes and follow `TensorBackend::matmul`
/// shape rules. Accumulation is f32; order and fused arithmetic may differ.
/// Inputs and accumulated results must remain finite (rounding a low-format
/// output may overflow). Unsupported products return an error, without a CPU
/// fallback or silent whole-input expansion to f32.
pub trait TensorLowBackend: TensorBackend {
    type LowTensor: HasLowDtype;

    fn low_precision_support(&self, dtype: LowDtype) -> LowPrecisionSupport;
    fn upload_low(
        &self,
        dtype: LowDtype,
        shape: Shape,
        bits: &[u16],
    ) -> Result<Self::LowTensor, Self::Error>;
    fn read_low_bits(&self, tensor: &Self::LowTensor) -> Result<Vec<u16>, Self::Error>;
    fn materialize_low(&self, tensor: &Self::LowTensor) -> Result<Self::LowTensor, Self::Error>;
    fn reshape_low(
        &self,
        tensor: &Self::LowTensor,
        shape: Shape,
    ) -> Result<Self::LowTensor, Self::Error>;
    fn permute_low(
        &self,
        tensor: &Self::LowTensor,
        axes: &[usize],
    ) -> Result<Self::LowTensor, Self::Error>;
    fn broadcast_low(
        &self,
        tensor: &Self::LowTensor,
        shape: Shape,
    ) -> Result<Self::LowTensor, Self::Error>;
    fn cast_to_low(
        &self,
        tensor: &Self::Tensor,
        dtype: LowDtype,
    ) -> Result<Self::LowTensor, Self::Error>;
    fn cast_to_f32(&self, tensor: &Self::LowTensor) -> Result<Self::Tensor, Self::Error>;
    fn matmul_low(
        &self,
        left: &Self::LowTensor,
        right: &Self::LowTensor,
    ) -> Result<Self::LowTensor, Self::Error>;
    fn matmul_low_f32(
        &self,
        left: &Self::LowTensor,
        right: &Self::LowTensor,
    ) -> Result<Self::Tensor, Self::Error>;
}
