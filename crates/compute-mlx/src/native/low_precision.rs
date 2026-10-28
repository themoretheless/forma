use super::lowering::{NativeLowerer, casts};
use super::*;
use tensor_core::{HasLowDtype, LowDtype, LowPrecisionSupport, LowStorage, TensorLowBackend};

/// Native two-byte floating-point storage. The private wrapper keeps
/// `HasLowDtype` total: an f32/u32 tensor cannot enter this type.
#[derive(Clone)]
pub struct MlxLowTensor {
    pub(super) tensor: MlxTensor,
    pub(super) dtype: LowDtype,
}

impl HasShape for MlxLowTensor {
    fn shape(&self) -> &Shape {
        &self.tensor.shape
    }
}

impl HasLowDtype for MlxLowTensor {
    fn low_dtype(&self) -> LowDtype {
        self.dtype
    }
}

impl From<LowDtype> for MlxDtype {
    fn from(dtype: LowDtype) -> Self {
        match dtype {
            LowDtype::F16 => Self::F16,
            LowDtype::Bf16 => Self::Bf16,
        }
    }
}

impl MlxBackend {
    pub(super) fn low_accessor(&self, dtype: LowDtype) -> Result<ffi::HalfData, MlxError> {
        let accessor = match dtype {
            LowDtype::F16 => self.context.api.array_data_float16,
            LowDtype::Bf16 => self.context.api.array_data_bfloat16,
        };
        accessor.ok_or(MlxError::UnsupportedLowPrecision {
            dtype,
            operation: "native half accessors",
        })
    }

    pub(super) fn check_low(&self, tensor: &MlxLowTensor) -> Result<(), MlxError> {
        self.check(&tensor.tensor, Some(tensor.dtype.into()))
    }
}

impl TensorLowBackend for MlxBackend {
    type LowTensor = MlxLowTensor;

    fn low_precision_support(&self, dtype: LowDtype) -> LowPrecisionSupport {
        LowPrecisionSupport {
            storage: LowStorage::Native16,
            matmul: self.low_accessor(dtype).is_ok(),
            matmul_f32: self.low_accessor(dtype).is_ok() && self.context.api.metal.is_some(),
        }
    }

    fn upload_low(
        &self,
        dtype: LowDtype,
        shape: Shape,
        bits: &[u16],
    ) -> Result<MlxLowTensor, MlxError> {
        self.low_accessor(dtype)?;
        let tensor = self.upload(shape, bits.as_ptr().cast(), bits.len(), dtype.into())?;
        Ok(MlxLowTensor { tensor, dtype })
    }

    fn read_low_bits(&self, tensor: &MlxLowTensor) -> Result<Vec<u16>, MlxError> {
        self.check_low(tensor)?;
        let accessor = self.low_accessor(tensor.dtype)?;
        let input = self.materialize(&tensor.tensor)?;
        native(
            "read_low_bits",
            self.context.api.call(|a| unsafe {
                Api::check((a.array_eval)(input.array.raw))?;
                if input.shape.is_empty() {
                    return Ok(Vec::new());
                }
                let ptr = accessor(input.array.raw);
                if ptr.is_null() {
                    return Err("evaluated low-precision array has no data".into());
                }
                Ok(std::slice::from_raw_parts(ptr, input.shape.numel()).to_vec())
            }),
        )
    }

    fn materialize_low(&self, input: &MlxLowTensor) -> Result<MlxLowTensor, MlxError> {
        self.check_low(input)?;
        Ok(MlxLowTensor {
            tensor: self.materialize(&input.tensor)?,
            dtype: input.dtype,
        })
    }

    fn reshape_low(&self, input: &MlxLowTensor, shape: Shape) -> Result<MlxLowTensor, MlxError> {
        self.check_low(input)?;
        Ok(MlxLowTensor {
            tensor: self.reshape(&input.tensor, shape)?,
            dtype: input.dtype,
        })
    }

    fn permute_low(&self, input: &MlxLowTensor, axes: &[usize]) -> Result<MlxLowTensor, MlxError> {
        self.check_low(input)?;
        Ok(MlxLowTensor {
            tensor: self.permute(&input.tensor, axes)?,
            dtype: input.dtype,
        })
    }

    fn broadcast_low(&self, input: &MlxLowTensor, shape: Shape) -> Result<MlxLowTensor, MlxError> {
        self.check_low(input)?;
        Ok(MlxLowTensor {
            tensor: self.broadcast_to(&input.tensor, shape)?,
            dtype: input.dtype,
        })
    }

    fn cast_to_low(&self, input: &MlxTensor, dtype: LowDtype) -> Result<MlxLowTensor, MlxError> {
        self.check(input, Some(MlxDtype::F32))?;
        self.low_accessor(dtype)?;
        Ok(MlxLowTensor {
            tensor: casts::cast_to_low(&mut NativeLowerer::new(self), input.clone(), dtype)?,
            dtype,
        })
    }

    fn cast_to_f32(&self, input: &MlxLowTensor) -> Result<MlxTensor, MlxError> {
        self.check_low(input)?;
        casts::cast_to_f32(&mut NativeLowerer::new(self), input.tensor.clone())
    }

    fn matmul_low(
        &self,
        left: &MlxLowTensor,
        right: &MlxLowTensor,
    ) -> Result<MlxLowTensor, MlxError> {
        self.check_low(left)?;
        self.check_low(right)?;
        self.check(&right.tensor, Some(left.dtype.into()))?;
        let shape = tensor_core::matmul_shape(left.shape(), right.shape())?;
        let dtype = left.dtype.into();
        let tensor = if shape.is_empty() {
            self.zeros(shape, dtype)?
        } else {
            self.output("low matmul", shape, dtype, |a, out| unsafe {
                (a.matmul)(
                    out,
                    left.tensor.array.raw,
                    right.tensor.array.raw,
                    self.context.stream,
                )
            })?
        };
        Ok(MlxLowTensor {
            tensor,
            dtype: left.dtype,
        })
    }

    fn matmul_low_f32(
        &self,
        left: &MlxLowTensor,
        right: &MlxLowTensor,
    ) -> Result<MlxTensor, MlxError> {
        self.check_low(left)?;
        self.check_low(right)?;
        self.check(&right.tensor, Some(left.dtype.into()))?;
        let plan = tensor_core::MatmulPlan::new(left.shape(), right.shape())?;
        if !self.low_precision_support(left.dtype).matmul_f32 {
            return Err(MlxError::UnsupportedLowPrecision {
                dtype: left.dtype,
                operation: "direct low-input matmul with f32 output",
            });
        }
        self.low_matmul_f32(left, right, plan)
    }
}
