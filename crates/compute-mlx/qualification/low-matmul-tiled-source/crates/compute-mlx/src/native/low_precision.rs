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
    fn low_accessor(&self, dtype: LowDtype) -> Result<ffi::HalfData, MlxError> {
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

    fn cast_values(&self, input: &MlxTensor, dtype: MlxDtype) -> Result<MlxTensor, MlxError> {
        self.check(input, None)?;
        self.output("cast", input.shape.clone(), dtype, |a, out| unsafe {
            (a.astype)(out, input.array.raw, dtype.raw(), self.context.stream)
        })
    }

    fn encode_bf16(&self, input: &MlxTensor) -> Result<MlxTensor, MlxError> {
        // Metal's native float-to-bfloat conversion flushes f32 subnormals in
        // MLX 0.32.1. Integer rounding preserves them and implements ties-to-even
        // across the full format. Every operation remains in the device graph.
        let bits = self.output(
            "f32 bit view",
            input.shape.clone(),
            MlxDtype::U32,
            |a, out| unsafe { (a.view)(out, input.array.raw, ffi::U32, self.context.stream) },
        )?;
        let scalar = |value| self.upload_u32(Shape::new(vec![])?, &[value]);
        let shift = scalar(16)?;
        let one = scalar(1)?;
        let half_minus_one = scalar(0x7fff)?;
        let magnitude_mask = scalar(0x7fff_ffff)?;
        let infinity = scalar(0x7f80_0000)?;
        let canonical_nan = scalar(0x7fc0)?;
        let upper = self.binary_u32(self.context.api.right_shift, &bits, &shift)?;
        let lsb = self.binary_u32(self.context.api.bitwise_and, &upper, &one)?;
        let rounded = self.binary_u32(self.context.api.add, &bits, &half_minus_one)?;
        let rounded = self.binary_u32(self.context.api.add, &rounded, &lsb)?;
        let rounded = self.binary_u32(self.context.api.right_shift, &rounded, &shift)?;
        let magnitude = self.binary_u32(self.context.api.bitwise_and, &bits, &magnitude_mask)?;
        let is_nan = self.compare_values(CompareOp::Greater, &magnitude, &infinity)?;
        let encoded = self.select_values(&is_nan, &canonical_nan, &rounded)?;
        self.output(
            "bf16 bit encoding",
            input.shape.clone(),
            MlxDtype::Bf16,
            |a, out| unsafe {
                let mut words = (a.array_new)();
                let code = (a.astype)(&mut words, encoded.array.raw, ffi::U16, self.context.stream);
                let code = if code == 0 {
                    (a.view)(out, words, ffi::BF16, self.context.stream)
                } else {
                    code
                };
                (a.array_free)(words);
                code
            },
        )
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
            tensor: match dtype {
                LowDtype::F16 => self.cast_values(input, dtype.into())?,
                LowDtype::Bf16 => self.encode_bf16(input)?,
            },
            dtype,
        })
    }

    fn cast_to_f32(&self, input: &MlxLowTensor) -> Result<MlxTensor, MlxError> {
        self.check_low(input)?;
        self.cast_values(&input.tensor, MlxDtype::F32)
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
