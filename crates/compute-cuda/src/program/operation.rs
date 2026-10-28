use super::plan::{BufferRef, CudaDtype};
use crate::{
    CudaError, CudaRuntime,
    dispatch::{BinaryDispatch, UnaryDispatch},
    indexing::{
        compact_dispatch::{CompactDispatch, NormalizeMaskDispatch},
        dispatch::{CompareDispatch, CopyDispatch, SelectDispatch},
        gather_dispatch::{GatherDispatch, InvalidIndicesDispatch},
        scan_dispatch::{ScanCarryDispatch, ScanPass},
        scatter_dispatch::{ScatterDispatch, ScatterOwnersDispatch},
    },
    low_dispatch::{LowBinaryDispatch, LowCastDispatch, LowUnaryDispatch},
    matmul::{GemmPlan, PreparedGemm},
    reduction::dispatch::ReductionPass,
};
use crate::{attention_dispatch::AttentionDispatch, statistics_dispatch::RowPlan};
use tensor_core::{LowDtype, MatmulPrecision};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum MatmulRequest {
    F32(MatmulPrecision),
    Low(LowDtype),
}
impl MatmulRequest {
    pub fn validate(self, rt: &CudaRuntime) -> Result<(), CudaError> {
        match self {
            Self::F32(p) => rt.matmul_policy(p).map(|_| ()),
            Self::Low(d) => rt.validate_low_matmul(d),
        }
    }
    pub fn prepare(self, rt: &CudaRuntime, plan: GemmPlan) -> Result<PreparedGemm, CudaError> {
        match self {
            Self::F32(p) => rt.prepare_gemm_f32(plan, p),
            Self::Low(d) => rt.prepare_gemm_low(plan, d),
        }
    }
}
#[derive(Clone, Copy, Debug)]
pub(super) enum Destination {
    Scratch(usize),
    Output(usize),
}
#[derive(Debug)]
pub(super) enum Operation {
    StatisticsPartial {
        source: BufferRef,
        center: BufferRef,
        pass: RowPlan,
        op: u32,
    },
    StatisticsMerge {
        partials: BufferRef,
        pass: RowPlan,
        op: u32,
    },
    StatisticsEmit {
        source: BufferRef,
        first: BufferRef,
        second: BufferRef,
        pass: RowPlan,
        mode: (u32, f32),
    },
    StatisticsLse {
        first: BufferRef,
        second: BufferRef,
        pass: RowPlan,
    },
    StatisticsMoments {
        source: BufferRef,
        first: BufferRef,
        second: BufferRef,
        variance: usize,
        pass: RowPlan,
    },
    Attention {
        query: BufferRef,
        key: BufferRef,
        value: BufferRef,
        mask: Option<(BufferRef, bool)>,
        accumulator: usize,
        pass: AttentionDispatch,
    },
    Zero,
    InvalidIndices {
        indices: BufferRef,
        pass: InvalidIndicesDispatch,
    },
    Gather {
        source: BufferRef,
        indices: BufferRef,
        pass: GatherDispatch,
    },
    ScatterOwners {
        indices: BufferRef,
        pass: ScatterOwnersDispatch,
    },
    Scatter {
        indices: BufferRef,
        updates: BufferRef,
        owners: BufferRef,
        pass: ScatterDispatch,
    },
    Scan {
        source: BufferRef,
        totals: usize,
        pass: ScanPass,
    },
    ScanCarry {
        offsets: BufferRef,
        pass: ScanCarryDispatch,
    },
    Normalize {
        mask: BufferRef,
        pass: NormalizeMaskDispatch,
    },
    Compact {
        source: BufferRef,
        flags: BufferRef,
        prefix: BufferRef,
        count: usize,
        pass: CompactDispatch,
    },
    Copy {
        source: BufferRef,
        pass: CopyDispatch,
    },
    Unary {
        source: BufferRef,
        pass: UnaryDispatch,
    },
    LowUnary {
        source: BufferRef,
        pass: LowUnaryDispatch,
    },
    Binary {
        left: BufferRef,
        right: BufferRef,
        pass: BinaryDispatch,
    },
    LowBinary {
        left: BufferRef,
        right: BufferRef,
        pass: LowBinaryDispatch,
    },
    Compare {
        left: BufferRef,
        right: BufferRef,
        pass: CompareDispatch,
    },
    Select {
        mask: BufferRef,
        yes: BufferRef,
        no: BufferRef,
        pass: SelectDispatch,
    },
    Cast {
        source: BufferRef,
        pass: LowCastDispatch,
        to: CudaDtype,
    },
    Reduction {
        source: BufferRef,
        pass: ReductionPass,
    },
    Gemm {
        left: BufferRef,
        right: BufferRef,
        plan: GemmPlan,
        request: MatmulRequest,
    },
    Fill {
        count: usize,
        value: u32,
    },
}
impl Operation {
    pub fn auxiliary_destination(&self) -> Option<usize> {
        match self {
            Self::StatisticsMoments { variance, .. } => Some(*variance),
            Self::Attention { accumulator, .. } => Some(*accumulator),
            Self::Scan { totals, .. } => Some(*totals),
            Self::Compact { count, .. } => Some(*count),
            _ => None,
        }
    }
    pub fn metadata(&self) -> Option<&[u64]> {
        match self {
            Self::StatisticsPartial { pass, .. }
            | Self::StatisticsEmit { pass, .. }
            | Self::StatisticsMoments { pass, .. } => Some(&pass.metadata),
            Self::StatisticsMerge { .. } | Self::StatisticsLse { .. } => None,
            Self::Attention { pass, .. } => Some(&pass.metadata),
            Self::InvalidIndices { pass, .. } => Some(&pass.metadata),
            Self::Gather { pass, .. } => Some(&pass.metadata),
            Self::ScatterOwners { pass, .. } => Some(&pass.geometry.metadata),
            Self::Scatter { pass, .. } => Some(&pass.metadata),
            Self::Scan { pass, .. } => Some(&pass.metadata),
            Self::Normalize { pass, .. } => Some(&pass.geometry.metadata),
            Self::Compact { pass, .. } => Some(&pass.metadata),
            Self::Zero | Self::ScanCarry { .. } => None,
            Self::Copy { pass, .. } => Some(&pass.metadata),
            Self::Unary { pass, .. } => Some(&pass.metadata),
            Self::LowUnary { pass, .. } => Some(&pass.geometry.metadata),
            Self::Binary { pass, .. } => Some(&pass.metadata),
            Self::LowBinary { pass, .. } => Some(&pass.geometry.metadata),
            Self::Compare { pass, .. } => Some(&pass.metadata),
            Self::Select { pass, .. } => Some(&pass.metadata),
            Self::Cast { pass, .. } => Some(&pass.geometry.metadata),
            Self::Reduction { pass, .. } => Some(&pass.metadata),
            Self::Gemm { .. } | Self::Fill { .. } => None,
        }
    }
}
#[derive(Debug)]
pub(super) struct Scheduled {
    pub destination: Destination,
    pub operation: Operation,
}
