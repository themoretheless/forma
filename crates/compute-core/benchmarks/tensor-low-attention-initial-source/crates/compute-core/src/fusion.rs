//! A typed scalar DAG compiled into one grid-strided WGSL dispatch. Building an
//! expression allocates no GPU arrays; only final outputs become storage buffers.
mod graph;
mod program;
use crate::{ComputeError, Kernel, KernelBindingError, KernelError, gpu_compute::GpuContext};
pub use graph::{Expression, FusionGraph, Predicate};
use std::sync::Arc;

#[derive(Debug)]
pub enum FusionError {
    ForeignExpression,
    InputSlot { slot: usize, count: usize },
    NonfiniteConstant,
    NoOutputs,
    BindingLimit { requested: usize, limit: u32 },
    InputCount { expected: usize, actual: usize },
    OutputCount { expected: usize, actual: usize },
    ForeignDevice,
    Kernel(KernelError),
    Binding(KernelBindingError),
    Compute(ComputeError),
}
impl std::fmt::Display for FusionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ForeignExpression => f.write_str("expression belongs to another fusion graph"),
            Self::InputSlot { slot, count } => write!(
                f,
                "input slot {slot} exceeds the declared input count {count}"
            ),
            Self::NonfiniteConstant => f.write_str("fusion constants must be finite f32 values"),
            Self::NoOutputs => f.write_str("a fused kernel needs at least one output"),
            Self::BindingLimit { requested, limit } => write!(
                f,
                "fused kernel needs {requested} storage bindings; device limit is {limit}"
            ),
            Self::InputCount { expected, actual } => write!(
                f,
                "fused kernel expects {expected} input arrays, received {actual}"
            ),
            Self::OutputCount { expected, actual } => write!(
                f,
                "fused kernel expects {expected} output arrays, received {actual}"
            ),
            Self::ForeignDevice => f.write_str("fused kernel belongs to another GPU device"),
            Self::Kernel(e) => e.fmt(f),
            Self::Binding(e) => e.fmt(f),
            Self::Compute(e) => e.fmt(f),
        }
    }
}
impl std::error::Error for FusionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Kernel(e) => Some(e),
            Self::Binding(e) => Some(e),
            Self::Compute(e) => Some(e),
            _ => None,
        }
    }
}
impl From<ComputeError> for FusionError {
    fn from(e: ComputeError) -> Self {
        Self::Compute(e)
    }
}
impl From<KernelError> for FusionError {
    fn from(e: KernelError) -> Self {
        Self::Kernel(e)
    }
}
impl From<KernelBindingError> for FusionError {
    fn from(e: KernelBindingError) -> Self {
        Self::Binding(e)
    }
}

/// Reusable compiled expression. Inputs may be arrays or broadcast scalars;
/// output length is chosen when recording. Intermediate expressions stay in
/// shader locals; arithmetic follows WGSL f32 semantics and may contract FMAs.
pub struct FusedKernel {
    context: GpuContext,
    kernel: Arc<Kernel>,
    inputs: usize,
    used_inputs: Vec<usize>,
    outputs: usize,
    operations: usize,
}
impl FusedKernel {
    pub fn input_count(&self) -> usize {
        self.inputs
    }
    pub fn output_count(&self) -> usize {
        self.outputs
    }
    /// Reachable, distinct computation nodes after common-expression reuse.
    pub fn operation_count(&self) -> usize {
        self.operations
    }
    /// The generated source is inspectable and can be exported for diagnostics.
    pub fn source(&self) -> &str {
        self.kernel.source()
    }
}

/// Compiled elementwise expression followed by a sum. The first pass evaluates
/// and reduces directly to workgroup partials; subsequent passes fold them to
/// one f32. No full-sized mapped output is allocated. Empty input sums to zero.
pub struct FusedSumKernel(FusedKernel);
impl FusedSumKernel {
    pub(crate) fn dot(context: &GpuContext) -> Result<Self, KernelError> {
        let mut graph = FusionGraph::new(2);
        let a = graph.input(0).expect("built-in graph declares input zero");
        let b = graph.input(1).expect("built-in graph declares input one");
        let product = graph
            .binary(crate::BinaryOp::Multiply, &a, &b)
            .expect("built-in operands belong to the same graph");
        graph
            .compile_sum(context, &product)
            .map_err(|error| KernelError {
                message: format!("built-in dot pipeline: {error}"),
            })
    }
    pub fn input_count(&self) -> usize {
        self.0.input_count()
    }
    pub fn operation_count(&self) -> usize {
        self.0.operation_count()
    }
    pub fn source(&self) -> &str {
        self.0.source()
    }
}
