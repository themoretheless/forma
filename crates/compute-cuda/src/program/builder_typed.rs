use super::*;
use tensor_core::{CompareOp, LowDtype, select_shape};

impl CudaProgramPlanBuilder {
    pub(super) fn low(&self, value: CudaValue) -> Result<PlannedValue, CudaError> {
        let source = self.value(value)?;
        source.dtype.low_dtype().ok_or(CudaError::Dtype)?;
        Ok(source)
    }

    pub(super) fn low_pair(
        &self,
        left: CudaValue,
        right: CudaValue,
    ) -> Result<(PlannedValue, PlannedValue), CudaError> {
        let a = self.value(left)?;
        let b = self.value(right)?;
        let left = a.dtype.low_dtype().ok_or(CudaError::Dtype)?;
        let right = b.dtype.low_dtype().ok_or(CudaError::Dtype)?;
        if left != right {
            return Err(TensorError::LowDtypeMismatch { left, right }.into());
        }
        Ok((a, b))
    }

    pub fn input_u32(&mut self, layout: Layout) -> Result<CudaValue, CudaError> {
        self.input_typed(layout, CudaDtype::U32)
    }
    pub fn input_low(&mut self, dtype: LowDtype, layout: Layout) -> Result<CudaValue, CudaError> {
        self.input_typed(layout, dtype.into())
    }

    pub fn cast_to_low(
        &mut self,
        value: CudaValue,
        dtype: LowDtype,
    ) -> Result<CudaValue, CudaError> {
        let source = self.require(value, CudaDtype::F32)?;
        self.cast(source, dtype.into())
    }
    pub fn cast_to_f32(&mut self, value: CudaValue) -> Result<CudaValue, CudaError> {
        let source = self.low(value)?;
        self.cast(source, CudaDtype::F32)
    }
    fn cast(&mut self, source: PlannedValue, to: CudaDtype) -> Result<CudaValue, CudaError> {
        self.output(source.layout.shape().clone(), to, |output| Step::Cast {
            source,
            output,
            to,
        })
    }

    pub fn unary_low(&mut self, value: CudaValue, op: UnaryOp) -> Result<CudaValue, CudaError> {
        self.low(value)?;
        self.unary_op(value, op as u32, 1., 0.)
    }
    pub fn binary_low(
        &mut self,
        left: CudaValue,
        right: CudaValue,
        op: BinaryOp,
    ) -> Result<CudaValue, CudaError> {
        let (left, right) = self.low_pair(left, right)?;
        self.binary_values(left, right, op)
    }
    pub fn binary_u32(
        &mut self,
        left: CudaValue,
        right: CudaValue,
        op: BinaryOp,
    ) -> Result<CudaValue, CudaError> {
        let left = self.require(left, CudaDtype::U32)?;
        let right = self.require(right, CudaDtype::U32)?;
        if op == BinaryOp::Divide {
            return Err(CudaError::InvalidInput("u32 division is not supported"));
        }
        self.binary_values(left, right, op)
    }

    pub fn compare(
        &mut self,
        left: CudaValue,
        right: CudaValue,
        op: CompareOp,
    ) -> Result<CudaValue, CudaError> {
        let left = self.require(left, CudaDtype::F32)?;
        let right = self.require(right, CudaDtype::F32)?;
        self.comparison(left, right, op)
    }
    pub fn compare_u32(
        &mut self,
        left: CudaValue,
        right: CudaValue,
        op: CompareOp,
    ) -> Result<CudaValue, CudaError> {
        let left = self.require(left, CudaDtype::U32)?;
        let right = self.require(right, CudaDtype::U32)?;
        self.comparison(left, right, op)
    }
    pub fn compare_low(
        &mut self,
        left: CudaValue,
        right: CudaValue,
        op: CompareOp,
    ) -> Result<CudaValue, CudaError> {
        let (left, right) = self.low_pair(left, right)?;
        self.comparison(left, right, op)
    }
    fn comparison(
        &mut self,
        mut left: PlannedValue,
        mut right: PlannedValue,
        op: CompareOp,
    ) -> Result<CudaValue, CudaError> {
        let shape = left.layout.shape().broadcast(right.layout.shape())?;
        left.broadcast_to(shape.clone())?;
        right.broadcast_to(shape.clone())?;
        validate_layout(&left.layout, left.dtype)?;
        validate_layout(&right.layout, right.dtype)?;
        self.output(shape, CudaDtype::U32, |output| Step::Compare {
            left,
            right,
            output,
            op,
        })
    }

    pub fn select(
        &mut self,
        mask: CudaValue,
        yes: CudaValue,
        no: CudaValue,
    ) -> Result<CudaValue, CudaError> {
        let mask = self.require(mask, CudaDtype::U32)?;
        let yes = self.require(yes, CudaDtype::F32)?;
        let no = self.require(no, CudaDtype::F32)?;
        self.selection(mask, yes, no)
    }
    pub fn select_u32(
        &mut self,
        mask: CudaValue,
        yes: CudaValue,
        no: CudaValue,
    ) -> Result<CudaValue, CudaError> {
        let mask = self.require(mask, CudaDtype::U32)?;
        let yes = self.require(yes, CudaDtype::U32)?;
        let no = self.require(no, CudaDtype::U32)?;
        self.selection(mask, yes, no)
    }
    pub fn select_low(
        &mut self,
        mask: CudaValue,
        yes: CudaValue,
        no: CudaValue,
    ) -> Result<CudaValue, CudaError> {
        let mask = self.require(mask, CudaDtype::U32)?;
        let (yes, no) = self.low_pair(yes, no)?;
        self.selection(mask, yes, no)
    }
    fn selection(
        &mut self,
        mut mask: PlannedValue,
        mut yes: PlannedValue,
        mut no: PlannedValue,
    ) -> Result<CudaValue, CudaError> {
        // Joint three-way broadcasting preserves the shared zero-axis rule:
        // an empty mask can make an otherwise oversized value broadcast empty.
        let shape = select_shape(mask.layout.shape(), yes.layout.shape(), no.layout.shape())?;
        mask.broadcast_to(shape.clone())?;
        yes.broadcast_to(shape.clone())?;
        no.broadcast_to(shape.clone())?;
        // The mask remains its existing u32 allocation. A broadcast mask has
        // no result-sized u32 allocation, so only the value dtype limits the
        // logical traversal. Broadcasting preserves its checked storage span.
        for source in [&yes, &no] {
            validate_layout(&source.layout, source.dtype)?;
        }
        self.output(shape, yes.dtype, |output| Step::Select {
            mask,
            yes,
            no,
            output,
        })
    }

    pub fn reduce_u32(
        &mut self,
        value: CudaValue,
        op: ReduceOp,
        axes: &[usize],
        keep_dims: bool,
    ) -> Result<CudaValue, CudaError> {
        self.require(value, CudaDtype::U32)?;
        self.reduction(value, op, axes, keep_dims, false)
    }
    pub fn reduce_low_f32(
        &mut self,
        value: CudaValue,
        op: ReduceOp,
        axes: &[usize],
        keep_dims: bool,
    ) -> Result<CudaValue, CudaError> {
        self.low(value)?;
        self.reduction(value, op, axes, keep_dims, false)
    }
    pub fn reduce_low(
        &mut self,
        value: CudaValue,
        op: ReduceOp,
        axes: &[usize],
        keep_dims: bool,
    ) -> Result<CudaValue, CudaError> {
        let dtype = self.low(value)?.dtype.low_dtype().unwrap();
        self.transaction(|this| {
            let result = this.reduce_low_f32(value, op, axes, keep_dims)?;
            this.cast_to_low(result, dtype)
        })
    }
    pub fn mean_low_f32(
        &mut self,
        value: CudaValue,
        axes: &[usize],
        keep_dims: bool,
    ) -> Result<CudaValue, CudaError> {
        self.low(value)?;
        self.reduction(value, ReduceOp::Sum, axes, keep_dims, true)
    }
    pub fn mean_low(
        &mut self,
        value: CudaValue,
        axes: &[usize],
        keep_dims: bool,
    ) -> Result<CudaValue, CudaError> {
        let dtype = self.low(value)?.dtype.low_dtype().unwrap();
        self.transaction(|this| {
            let result = this.mean_low_f32(value, axes, keep_dims)?;
            this.cast_to_low(result, dtype)
        })
    }

    pub fn matmul_low_f32(
        &mut self,
        left: CudaValue,
        right: CudaValue,
    ) -> Result<CudaValue, CudaError> {
        self.low_pair(left, right)?;
        self.matmul_values(left, right, MatmulPrecision::F32)
    }
    pub fn matmul_low(
        &mut self,
        left: CudaValue,
        right: CudaValue,
    ) -> Result<CudaValue, CudaError> {
        let (source, _) = self.low_pair(left, right)?;
        let dtype = source.dtype.low_dtype().unwrap();
        self.transaction(|this| {
            let result = this.matmul_low_f32(left, right)?;
            this.cast_to_low(result, dtype)
        })
    }
}
