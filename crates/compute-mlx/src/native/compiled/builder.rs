use super::*;
use tensor_core::{LowDtype, ReduceOp, mean_shape, reduction_shape};

impl MlxProgramBuilder {
    pub(super) fn require(&self, value: MlxValue, dtype: MlxDtype) -> Result<TensorSpec, MlxError> {
        let spec = self.spec(&value)?;
        if spec.dtype != dtype {
            return Err(MlxError::Dtype);
        }
        Ok(spec)
    }
    pub(super) fn low(&self, value: MlxValue) -> Result<(TensorSpec, LowDtype), MlxError> {
        let spec = self.spec(&value)?;
        let dtype = match spec.dtype {
            MlxDtype::F16 => LowDtype::F16,
            MlxDtype::Bf16 => LowDtype::Bf16,
            _ => return Err(MlxError::Dtype),
        };
        Ok((spec, dtype))
    }
    fn typed_input(&mut self, shape: Shape, dtype: MlxDtype) -> Result<MlxValue, MlxError> {
        let spec = TensorSpec { shape, dtype };
        let value = self.push(spec.clone(), Operation::Input(self.inputs.len()))?;
        self.inputs.push(spec);
        Ok(value)
    }
    pub fn input(&mut self, shape: Shape) -> Result<MlxValue, MlxError> {
        self.typed_input(shape, MlxDtype::F32)
    }
    pub fn input_u32(&mut self, shape: Shape) -> Result<MlxValue, MlxError> {
        self.typed_input(shape, MlxDtype::U32)
    }
    pub fn input_low(&mut self, dtype: LowDtype, shape: Shape) -> Result<MlxValue, MlxError> {
        self.backend.low_accessor(dtype)?;
        self.typed_input(shape, dtype.into())
    }
    pub fn unary(&mut self, value: MlxValue, op: UnaryOp) -> Result<MlxValue, MlxError> {
        let spec = self.require(value, MlxDtype::F32)?;
        self.native(NativeOp::Unary(op), &[value], spec)
    }
    pub fn binary(
        &mut self,
        left: MlxValue,
        right: MlxValue,
        op: BinaryOp,
    ) -> Result<MlxValue, MlxError> {
        self.binary_typed(left, right, op, MlxDtype::F32)
    }
    pub fn binary_u32(
        &mut self,
        left: MlxValue,
        right: MlxValue,
        op: BinaryOp,
    ) -> Result<MlxValue, MlxError> {
        self.require(left, MlxDtype::U32)?;
        self.require(right, MlxDtype::U32)?;
        if op == BinaryOp::Divide {
            return Err(MlxError::Dtype);
        }
        self.binary_typed(left, right, op, MlxDtype::U32)
    }
    fn binary_typed(
        &mut self,
        left: MlxValue,
        right: MlxValue,
        op: BinaryOp,
        dtype: MlxDtype,
    ) -> Result<MlxValue, MlxError> {
        let a = self.require(left, dtype)?;
        let b = self.require(right, dtype)?;
        self.native(
            NativeOp::Binary(op),
            &[left, right],
            TensorSpec {
                shape: a.shape.broadcast(&b.shape)?,
                dtype,
            },
        )
    }
    pub fn compare_u32(
        &mut self,
        left: MlxValue,
        right: MlxValue,
        op: CompareOp,
    ) -> Result<MlxValue, MlxError> {
        self.require(left, MlxDtype::U32)?;
        self.require(right, MlxDtype::U32)?;
        self.transaction(|graph| lowering::indexing::compare(graph, left, right, op))
    }
    /// F32 comparisons produce exact u32 masks without implicit input casts.
    pub fn compare(
        &mut self,
        left: MlxValue,
        right: MlxValue,
        op: CompareOp,
    ) -> Result<MlxValue, MlxError> {
        self.require(left, MlxDtype::F32)?;
        self.require(right, MlxDtype::F32)?;
        self.transaction(|graph| lowering::indexing::compare(graph, left, right, op))
    }
    pub fn select_u32(
        &mut self,
        mask: MlxValue,
        yes: MlxValue,
        no: MlxValue,
    ) -> Result<MlxValue, MlxError> {
        self.require(yes, MlxDtype::U32)?;
        self.require(no, MlxDtype::U32)?;
        self.transaction(|graph| lowering::indexing::select(graph, mask, yes, no))
    }
    pub fn reshape(&mut self, value: MlxValue, shape: Shape) -> Result<MlxValue, MlxError> {
        let spec = self.spec(&value)?;
        if spec.shape.numel() != shape.numel() {
            return Err(TensorError::ElementCountMismatch {
                expected: spec.shape.numel(),
                actual: shape.numel(),
            }
            .into());
        }
        self.native(
            NativeOp::Reshape,
            &[value],
            TensorSpec {
                shape,
                dtype: spec.dtype,
            },
        )
    }
    pub fn permute(&mut self, value: MlxValue, axes: &[usize]) -> Result<MlxValue, MlxError> {
        let spec = self.spec(&value)?;
        let shape = spec.shape.permute(axes)?;
        self.native(
            NativeOp::Permute(axes.iter().map(|&x| x as i32).collect()),
            &[value],
            TensorSpec {
                shape,
                dtype: spec.dtype,
            },
        )
    }
    pub fn broadcast_to(&mut self, value: MlxValue, shape: Shape) -> Result<MlxValue, MlxError> {
        let spec = self.spec(&value)?;
        if spec.shape.broadcast(&shape)? != shape {
            return Err(TensorError::IncompatibleBroadcast {
                left: spec.shape.dims().to_vec(),
                right: shape.dims().to_vec(),
            }
            .into());
        }
        self.native(
            NativeOp::Broadcast,
            &[value],
            TensorSpec {
                shape,
                dtype: spec.dtype,
            },
        )
    }
    fn reduce_native(
        &mut self,
        value: MlxValue,
        op: ReduceOp,
        axes: &[usize],
        keep: bool,
        dtype: MlxDtype,
        mean: bool,
    ) -> Result<MlxValue, MlxError> {
        let input = self.require(value, dtype)?;
        let shape = if mean {
            mean_shape(&input.shape, axes, keep)?
        } else {
            reduction_shape(op, &input.shape, axes, keep)?
        };
        if axes.is_empty() {
            return Ok(value);
        }
        let result = TensorSpec {
            shape: shape.clone(),
            dtype,
        };
        if input.shape.is_empty() || shape.is_empty() {
            return self.native(
                if !shape.is_empty() && op == ReduceOp::Product {
                    NativeOp::Ones
                } else {
                    NativeOp::Zeros
                },
                &[],
                result,
            );
        }
        let axes = axes.iter().map(|&x| x as i32).collect();
        self.native(
            if mean {
                NativeOp::Mean(axes, keep)
            } else {
                NativeOp::Reduce(op, axes, keep)
            },
            &[value],
            result,
        )
    }
    pub fn sum_axes(
        &mut self,
        value: MlxValue,
        axes: &[usize],
        keep: bool,
    ) -> Result<MlxValue, MlxError> {
        self.reduce_f32(value, ReduceOp::Sum, axes, keep)
    }
    pub fn reduce_f32(
        &mut self,
        value: MlxValue,
        op: ReduceOp,
        axes: &[usize],
        keep: bool,
    ) -> Result<MlxValue, MlxError> {
        self.reduce_native(value, op, axes, keep, MlxDtype::F32, false)
    }
    pub fn reduce_u32(
        &mut self,
        value: MlxValue,
        op: ReduceOp,
        axes: &[usize],
        keep: bool,
    ) -> Result<MlxValue, MlxError> {
        self.reduce_native(value, op, axes, keep, MlxDtype::U32, false)
    }
    pub fn mean_axes(
        &mut self,
        value: MlxValue,
        axes: &[usize],
        keep: bool,
    ) -> Result<MlxValue, MlxError> {
        self.reduce_native(value, ReduceOp::Sum, axes, keep, MlxDtype::F32, true)
    }
    pub fn matmul(&mut self, left: MlxValue, right: MlxValue) -> Result<MlxValue, MlxError> {
        let a = self.require(left, MlxDtype::F32)?;
        let b = self.require(right, MlxDtype::F32)?;
        let plan = tensor_core::MatmulPlan::new(&a.shape, &b.shape)?;
        let zero = plan.output.is_empty() || plan.left.dims()[plan.left.rank() - 1] == 0;
        let inputs = [left, right];
        self.native(
            if zero {
                NativeOp::Zeros
            } else {
                NativeOp::Matmul
            },
            if zero { &[] } else { &inputs },
            TensorSpec {
                shape: plan.output,
                dtype: MlxDtype::F32,
            },
        )
    }
}
