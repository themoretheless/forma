use super::plan::{
    BufferRef, CudaDtype, CudaProgramPlan, PlannedValue, Step, TensorSpec, validate_layout,
};
use crate::CudaError;
use std::sync::atomic::{AtomicU64, Ordering};
use tensor_core::{
    BinaryOp, Layout, MatmulPlan, MatmulPrecision, ReduceOp, Shape, TensorError, UnaryOp,
    mean_shape, reduction_shape,
};

/// A value belongs to exactly one CUDA program builder.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CudaValue {
    owner: u64,
    index: usize,
}

/// CPU-only recording used by the runtime-borrowing public builder. No CUDA
/// handles, tensor owners, storage allocations or metadata uploads are retained.
pub(crate) struct CudaProgramPlanBuilder {
    owner: u64,
    values: Vec<PlannedValue>,
    plan: CudaProgramPlan,
}

impl CudaProgramPlanBuilder {
    pub fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let owner = NEXT
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |v| v.checked_add(1))
            .expect("CUDA program identity space exhausted");
        Self {
            owner,
            values: Vec::new(),
            plan: CudaProgramPlan::default(),
        }
    }

    fn value(&self, value: CudaValue) -> Result<PlannedValue, CudaError> {
        if value.owner != self.owner {
            return Err(CudaError::InvalidInput(
                "value belongs to a different CUDA program builder",
            ));
        }
        self.values
            .get(value.index)
            .cloned()
            .ok_or(CudaError::InvalidInput("invalid CUDA program value"))
    }

    fn require(&self, value: CudaValue, dtype: CudaDtype) -> Result<PlannedValue, CudaError> {
        let source = self.value(value)?;
        if source.dtype != dtype {
            return Err(CudaError::Dtype);
        }
        Ok(source)
    }

    fn push(&mut self, value: PlannedValue) -> CudaValue {
        let index = self.values.len();
        self.values.push(value);
        CudaValue {
            owner: self.owner,
            index,
        }
    }

    fn transaction<T>(
        &mut self,
        record: impl FnOnce(&mut Self) -> Result<T, CudaError>,
    ) -> Result<T, CudaError> {
        let checkpoint = (
            self.values.len(),
            self.plan.inputs.len(),
            self.plan.scratch.len(),
            self.plan.steps.len(),
        );
        let result = record(self);
        if result.is_err() {
            self.values.truncate(checkpoint.0);
            self.plan.inputs.truncate(checkpoint.1);
            self.plan.scratch.truncate(checkpoint.2);
            self.plan.steps.truncate(checkpoint.3);
        }
        result
    }

    fn output(
        &mut self,
        shape: Shape,
        dtype: CudaDtype,
        step: impl FnOnce(usize) -> Step,
    ) -> Result<CudaValue, CudaError> {
        let layout = Layout::contiguous(shape.clone())?;
        validate_layout(&layout, dtype)?;
        let output = self.plan.scratch.len();
        self.plan.scratch.push(TensorSpec {
            layout: layout.clone(),
            dtype,
        });
        self.plan.steps.push(step(output));
        Ok(self.push(PlannedValue {
            input_views: Vec::new(),
            buffer: BufferRef::Scratch(output),
            layout,
            dtype,
        }))
    }

    pub fn input(&mut self, layout: Layout) -> Result<CudaValue, CudaError> {
        self.input_typed(layout, CudaDtype::F32)
    }

    fn input_typed(&mut self, layout: Layout, dtype: CudaDtype) -> Result<CudaValue, CudaError> {
        validate_layout(&layout, dtype)?;
        let input = self.plan.inputs.len();
        self.plan.inputs.push(TensorSpec {
            layout: layout.clone(),
            dtype,
        });
        Ok(self.push(PlannedValue {
            input_views: Vec::new(),
            buffer: BufferRef::Input(input),
            layout,
            dtype,
        }))
    }

    fn unary_op(
        &mut self,
        value: CudaValue,
        op: u32,
        scale: f32,
        bias: f32,
    ) -> Result<CudaValue, CudaError> {
        let source = self.value(value)?;
        self.output(source.layout.shape().clone(), source.dtype, |output| {
            Step::Unary {
                source,
                output,
                op,
                scale,
                bias,
            }
        })
    }

    pub fn unary(&mut self, value: CudaValue, op: UnaryOp) -> Result<CudaValue, CudaError> {
        self.require(value, CudaDtype::F32)?;
        self.unary_op(value, op as u32, 1., 0.)
    }

    pub fn affine(
        &mut self,
        value: CudaValue,
        scale: f32,
        bias: f32,
    ) -> Result<CudaValue, CudaError> {
        self.require(value, CudaDtype::F32)?;
        self.unary_op(value, 9, scale, bias)
    }

    pub fn binary(
        &mut self,
        left: CudaValue,
        right: CudaValue,
        op: BinaryOp,
    ) -> Result<CudaValue, CudaError> {
        let left = self.require(left, CudaDtype::F32)?;
        let right = self.require(right, CudaDtype::F32)?;
        self.binary_values(left, right, op)
    }

    fn binary_values(
        &mut self,
        mut left: PlannedValue,
        mut right: PlannedValue,
        op: BinaryOp,
    ) -> Result<CudaValue, CudaError> {
        let shape = left.layout.shape().broadcast(right.layout.shape())?;
        left.broadcast_to(shape.clone())?;
        right.broadcast_to(shape.clone())?;
        validate_layout(&left.layout, left.dtype)?;
        validate_layout(&right.layout, right.dtype)?;
        self.output(shape, left.dtype, |output| Step::Binary {
            left,
            right,
            output,
            op,
        })
    }

    pub fn materialize(&mut self, value: CudaValue) -> Result<CudaValue, CudaError> {
        if self.value(value)?.layout.is_contiguous() {
            Ok(value)
        } else {
            self.unary_op(value, 10, 1., 0.)
        }
    }

    pub fn reshape(&mut self, value: CudaValue, shape: Shape) -> Result<CudaValue, CudaError> {
        let source = self.value(value)?;
        if source.layout.shape().numel() != shape.numel() {
            return Err(TensorError::ElementCountMismatch {
                expected: source.layout.shape().numel(),
                actual: shape.numel(),
            }
            .into());
        }
        self.transaction(|this| {
            let contiguous = this.materialize(value)?;
            let mut source = this.value(contiguous)?;
            source.reshape(shape)?;
            validate_layout(&source.layout, source.dtype)?;
            Ok(this.push(source))
        })
    }

    pub fn permute(&mut self, value: CudaValue, axes: &[usize]) -> Result<CudaValue, CudaError> {
        let mut source = self.value(value)?;
        source.permute(axes)?;
        validate_layout(&source.layout, source.dtype)?;
        Ok(self.push(source))
    }

    pub fn broadcast_to(&mut self, value: CudaValue, shape: Shape) -> Result<CudaValue, CudaError> {
        let mut source = self.value(value)?;
        source.broadcast_to(shape)?;
        validate_layout(&source.layout, source.dtype)?;
        Ok(self.push(source))
    }

    pub fn reduce(
        &mut self,
        value: CudaValue,
        op: ReduceOp,
        axes: &[usize],
        keep_dims: bool,
    ) -> Result<CudaValue, CudaError> {
        self.require(value, CudaDtype::F32)?;
        self.reduction(value, op, axes, keep_dims, false)
    }

    pub fn sum_axes(
        &mut self,
        value: CudaValue,
        axes: &[usize],
        keep_dims: bool,
    ) -> Result<CudaValue, CudaError> {
        self.reduce(value, ReduceOp::Sum, axes, keep_dims)
    }

    pub fn mean(
        &mut self,
        value: CudaValue,
        axes: &[usize],
        keep_dims: bool,
    ) -> Result<CudaValue, CudaError> {
        self.require(value, CudaDtype::F32)?;
        self.reduction(value, ReduceOp::Sum, axes, keep_dims, true)
    }

    fn reduction(
        &mut self,
        value: CudaValue,
        op: ReduceOp,
        axes: &[usize],
        keep_dims: bool,
        mean: bool,
    ) -> Result<CudaValue, CudaError> {
        let source = self.value(value)?;
        let shape = if mean {
            mean_shape(source.layout.shape(), axes, keep_dims)?
        } else {
            reduction_shape(op, source.layout.shape(), axes, keep_dims)?
        };
        if axes.is_empty() {
            return if source.dtype.low_dtype().is_some() {
                self.cast_to_f32(value)
            } else {
                self.materialize(value)
            };
        }
        let dtype = if source.dtype.low_dtype().is_some() {
            CudaDtype::F32
        } else {
            source.dtype
        };
        self.output(shape, dtype, |output| Step::Reduce {
            source,
            output,
            op,
            axes: axes.to_vec(),
            keep_dims,
            mean,
        })
    }

    pub fn matmul(
        &mut self,
        left: CudaValue,
        right: CudaValue,
        precision: MatmulPrecision,
    ) -> Result<CudaValue, CudaError> {
        self.require(left, CudaDtype::F32)?;
        self.require(right, CudaDtype::F32)?;
        self.matmul_values(left, right, precision)
    }

    fn matmul_values(
        &mut self,
        left: CudaValue,
        right: CudaValue,
        precision: MatmulPrecision,
    ) -> Result<CudaValue, CudaError> {
        let a = self.value(left)?;
        let b = self.value(right)?;
        let plan = MatmulPlan::new(a.layout.shape(), b.layout.shape())?;
        let active = !plan.matrix_output.is_empty() && *plan.left.dims().last().unwrap() != 0;
        // Reject an impossible BLAS call before materializing either operand.
        if active {
            let dims = plan.matrix_output.dims();
            crate::policy::row_major_gemm(
                dims[dims.len() - 2],
                *plan.left.dims().last().unwrap(),
                dims[dims.len() - 1],
            )?;
        }
        self.transaction(|this| {
            let (mut a, mut b) = if active {
                let left = this.materialize(left)?;
                let right = this.materialize(right)?;
                (this.value(left)?, this.value(right)?)
            } else {
                (a, b)
            };
            a.promote(plan.left, true)?;
            b.promote(plan.right, false)?;
            // Retain even empty/K=0 operations so preparation still validates
            // requested precision and emits a fresh zero fill where required.
            this.output(plan.output, CudaDtype::F32, |output| Step::Matmul {
                left: a,
                right: b,
                output,
                precision,
            })
        })
    }

    pub(crate) fn finish(mut self, outputs: &[CudaValue]) -> Result<CudaProgramPlan, CudaError> {
        self.plan.outputs = outputs
            .iter()
            .map(|&value| self.value(value))
            .collect::<Result<_, _>>()?;
        Ok(self.plan)
    }
}

#[path = "builder_typed.rs"]
mod typed;

#[path = "builder_indexing.rs"]
mod indexing;

#[cfg(test)]
#[path = "builder_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "builder_typed_tests.rs"]
mod typed_tests;

#[cfg(test)]
#[path = "builder_indexing_tests.rs"]
mod indexing_tests;

#[path = "builder_attention.rs"]
mod attention;
#[cfg(test)]
#[path = "builder_attention_tests.rs"]
mod attention_tests;
#[path = "builder_statistics.rs"]
mod statistics;
#[cfg(test)]
#[path = "builder_statistics_tests.rs"]
mod statistics_tests;
