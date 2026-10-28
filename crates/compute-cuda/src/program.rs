//! Prepared resident typed schedules with opt-in CUDA Graph capture.
use crate::{CudaError, CudaRuntime};
use tensor_core::{BinaryOp, Layout, MatmulPrecision, ReduceOp, Shape, UnaryOp};
mod api_typed;
mod attention_api;
mod builder;
mod execution;
mod expansion;
mod gate;
mod graph;
mod graph_plan;
mod indexing_api;
mod indexing_launch;
mod indexing_plan;
mod launch;
mod operation;
mod plan;
mod preparation;
mod reduction_plan;
mod scratch;
mod statistics_api;
mod statistics_launch;
mod statistics_plan;
mod storage;
mod typed;
mod validation;
use builder::CudaProgramPlanBuilder;
pub use builder::CudaValue;
pub use execution::CudaPreparedProgram;
pub use graph::{CudaCaptureOptions, CudaGraphProgram, CudaGraphStats};
pub use plan::CudaDtype;
pub use typed::{CudaProgramInput, CudaProgramOutput, CudaProgramOutputMut};

/// Limits adapter-owned device allocations made during preparation. Caller
/// outputs and cuBLAS's internal allocations are not included.
#[derive(Clone, Copy, Debug)]
pub struct CudaPrepareOptions {
    pub max_scratch_bytes: usize,
    pub max_metadata_bytes: usize,
}
impl Default for CudaPrepareOptions {
    fn default() -> Self {
        Self {
            max_scratch_bytes: usize::MAX,
            max_metadata_bytes: usize::MAX,
        }
    }
}
/// Scheduled calls and reserved adapter tensor storage, not measured device
/// memory or the number of kernels selected internally by cuBLAS.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CudaProgramStats {
    pub kernel_launches: usize,
    /// Explicit asynchronous clears issued during replay, excluding preparation.
    pub memset_calls: usize,
    pub gemm_calls: usize,
    pub scratch_bytes: usize,
    pub metadata_bytes: usize,
}

/// A fixed-layout typed program tied to one runtime. Creating/validating nodes
/// allocates host descriptors only; preparation uploads metadata and scratch.
pub struct CudaProgramBuilder<'rt> {
    runtime: &'rt CudaRuntime,
    plan: CudaProgramPlanBuilder,
}
impl CudaRuntime {
    pub fn program(&self) -> CudaProgramBuilder<'_> {
        CudaProgramBuilder {
            runtime: self,
            plan: CudaProgramPlanBuilder::new(),
        }
    }
}
impl<'rt> CudaProgramBuilder<'rt> {
    pub fn input(&mut self, layout: Layout) -> Result<CudaValue, CudaError> {
        self.plan.input(layout)
    }
    pub fn unary(&mut self, value: CudaValue, op: UnaryOp) -> Result<CudaValue, CudaError> {
        self.plan.unary(value, op)
    }
    pub fn affine(
        &mut self,
        value: CudaValue,
        scale: f32,
        bias: f32,
    ) -> Result<CudaValue, CudaError> {
        self.plan.affine(value, scale, bias)
    }
    pub fn binary(
        &mut self,
        left: CudaValue,
        right: CudaValue,
        op: BinaryOp,
    ) -> Result<CudaValue, CudaError> {
        self.plan.binary(left, right, op)
    }
    pub fn reduce(
        &mut self,
        value: CudaValue,
        op: ReduceOp,
        axes: &[usize],
        keep_dims: bool,
    ) -> Result<CudaValue, CudaError> {
        self.plan.reduce(value, op, axes, keep_dims)
    }
    pub fn sum_axes(
        &mut self,
        value: CudaValue,
        axes: &[usize],
        keep_dims: bool,
    ) -> Result<CudaValue, CudaError> {
        self.plan.sum_axes(value, axes, keep_dims)
    }
    pub fn mean(
        &mut self,
        value: CudaValue,
        axes: &[usize],
        keep_dims: bool,
    ) -> Result<CudaValue, CudaError> {
        self.plan.mean(value, axes, keep_dims)
    }
    pub fn materialize(&mut self, value: CudaValue) -> Result<CudaValue, CudaError> {
        self.plan.materialize(value)
    }
    pub fn reshape(&mut self, value: CudaValue, shape: Shape) -> Result<CudaValue, CudaError> {
        self.plan.reshape(value, shape)
    }
    pub fn permute(&mut self, value: CudaValue, axes: &[usize]) -> Result<CudaValue, CudaError> {
        self.plan.permute(value, axes)
    }
    pub fn broadcast_to(&mut self, value: CudaValue, shape: Shape) -> Result<CudaValue, CudaError> {
        self.plan.broadcast_to(value, shape)
    }
    pub fn matmul(
        &mut self,
        left: CudaValue,
        right: CudaValue,
        precision: MatmulPrecision,
    ) -> Result<CudaValue, CudaError> {
        self.plan.matmul(left, right, precision)
    }
    /// Checks the complete expanded budget before device allocation or library
    /// handle initialization, then uploads/allocates once for subsequent runs.
    pub fn prepare(
        self,
        outputs: &[CudaValue],
        options: CudaPrepareOptions,
    ) -> Result<CudaPreparedProgram<'rt>, CudaError> {
        let plan = self.plan.finish(outputs)?;
        preparation::prepare(self.runtime, plan, options)
    }
}

#[cfg(test)]
mod preparation_tests;

#[cfg(test)]
mod typed_preparation_tests;

#[cfg(test)]
mod indexing_preparation_tests;

#[cfg(test)]
mod statistics_preparation_tests;
