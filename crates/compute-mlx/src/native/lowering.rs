//! One set of low-storage graph recipes for eager tensors and compiled values.
use super::{custom_metal::KernelKey, *};
use tensor_core::{ReduceOp, ScatterOp};

pub(super) mod attention;
pub(super) mod casts;
pub(super) mod compaction;
pub(super) mod indexing;
pub(super) mod ops;
pub(super) mod raw;
pub(super) mod scan;
pub(super) mod scatter;
pub(super) mod statistics;
pub(super) mod statistics_low;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct TensorSpec {
    pub shape: Shape,
    pub dtype: MlxDtype,
}

#[derive(Clone, Debug)]
pub(super) enum NativeOp {
    Unary(UnaryOp),
    Binary(BinaryOp),
    Cast(MlxDtype),
    ViewU32,
    ViewF32,
    ArangeU32(usize),
    Softmax(Vec<i32>),
    Logsumexp(Vec<i32>, bool),
    FastAttention {
        scale: f32,
        causal: bool,
        masked: bool,
    },
    PackBf16,
    RightShift,
    BitwiseAnd,
    Compare(CompareOp),
    Select,
    Zeros,
    Ones,
    Reshape,
    Permute(Vec<i32>),
    Broadcast,
    Reduce(ReduceOp, Vec<i32>, bool),
    Mean(Vec<i32>, bool),
    Matmul,
    Scan {
        axis: i32,
        inclusive: bool,
        reverse: bool,
    },
    TakeAxis(i32),
    PutAlongAxis(i32),
    Scatter {
        op: ScatterOp,
        axis: i32,
    },
}

pub(super) trait Lowering {
    type Value: Clone;
    fn spec(&self, value: &Self::Value) -> Result<TensorSpec, MlxError>;
    fn native(
        &mut self,
        op: NativeOp,
        inputs: &[Self::Value],
        output: TensorSpec,
    ) -> Result<Self::Value, MlxError>;
    fn constant_u32(&mut self, shape: Shape, words: &[u32]) -> Result<Self::Value, MlxError>;
    fn metal(
        &mut self,
        key: KernelKey,
        inputs: &[Self::Value],
        output: TensorSpec,
        bf16: bool,
        launch: (usize, usize),
    ) -> Result<Self::Value, MlxError>;
}

pub(super) struct NativeLowerer<'a> {
    backend: &'a MlxBackend,
}
impl<'a> NativeLowerer<'a> {
    pub fn new(backend: &'a MlxBackend) -> Self {
        Self { backend }
    }
}
impl Lowering for NativeLowerer<'_> {
    type Value = MlxTensor;
    fn spec(&self, value: &MlxTensor) -> Result<TensorSpec, MlxError> {
        self.backend.check(value, None)?;
        Ok(TensorSpec {
            shape: value.shape.clone(),
            dtype: value.dtype,
        })
    }
    fn native(
        &mut self,
        op: NativeOp,
        inputs: &[MlxTensor],
        output: TensorSpec,
    ) -> Result<MlxTensor, MlxError> {
        for input in inputs {
            self.backend.check(input, None)?;
        }
        let handles: Vec<_> = inputs.iter().map(|t| t.array.raw).collect();
        self.backend.output(
            "prepared native operation",
            output.shape.clone(),
            output.dtype,
            |api, out| {
                i32::from(
                    raw::apply(
                        api,
                        self.backend.context.stream,
                        &op,
                        &handles,
                        &output,
                        out,
                    )
                    .is_err(),
                )
            },
        )
    }
    fn constant_u32(&mut self, shape: Shape, words: &[u32]) -> Result<MlxTensor, MlxError> {
        self.backend.upload_u32(shape, words)
    }
    fn metal(
        &mut self,
        key: KernelKey,
        inputs: &[MlxTensor],
        output: TensorSpec,
        bf16: bool,
        launch: (usize, usize),
    ) -> Result<MlxTensor, MlxError> {
        let refs: Vec<_> = inputs.iter().collect();
        self.backend
            .custom_metal(key, &refs, output.shape, output.dtype, bf16, launch)
    }
}
