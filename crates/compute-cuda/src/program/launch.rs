//! Dispatch typed storage through the shared eager launch implementations.
use super::{
    execution::Instruction,
    operation::Operation,
    plan::{BufferRef, CudaDtype},
    storage::{Storage, StorageMut, StorageRef},
    typed::CudaProgramInput,
};
use crate::{
    CudaError, CudaRuntime, dispatch,
    indexing::dispatch::{LoadedCompare, SelectInputs},
    reduction::dispatch::LoadedReduction,
};

pub(super) fn source<'a>(
    buffer: BufferRef,
    inputs: &'a [CudaProgramInput<'_>],
    scratch: &'a [Option<Storage>],
) -> Result<StorageRef<'a>, CudaError> {
    match buffer {
        BufferRef::Input(i) => inputs.get(i).map(CudaProgramInput::storage),
        BufferRef::Scratch(i) => scratch.get(i).and_then(Option::as_ref).map(Storage::as_ref),
    }
    .ok_or(CudaError::InvalidInput("invalid internal prepared source"))
}
impl Instruction {
    pub(super) fn launch(
        &self,
        rt: &CudaRuntime,
        inputs: &[CudaProgramInput<'_>],
        scratch: &[Option<Storage>],
        output: StorageMut<'_>,
        auxiliary: Option<StorageMut<'_>>,
    ) -> Result<(), CudaError> {
        let metadata = || self.metadata.as_ref().expect("prepared metadata exists");
        let input = |id| source(id, inputs, scratch);
        match &self.operation {
            Operation::StatisticsPartial { .. }
            | Operation::StatisticsMerge { .. }
            | Operation::StatisticsEmit { .. }
            | Operation::StatisticsLse { .. }
            | Operation::StatisticsMoments { .. }
            | Operation::Attention { .. } => {
                self.launch_statistics(rt, inputs, scratch, output, auxiliary)
            }
            Operation::InvalidIndices { .. }
            | Operation::Gather { .. }
            | Operation::ScatterOwners { .. }
            | Operation::Scatter { .. }
            | Operation::Zero
            | Operation::Scan { .. }
            | Operation::ScanCarry { .. }
            | Operation::Normalize { .. }
            | Operation::Compact { .. } => {
                self.launch_indexing(rt, inputs, scratch, output, auxiliary)
            }
            Operation::Copy { source, pass } => match (input(*source)?, output) {
                (StorageRef::F32(a), StorageMut::F32(b)) => {
                    pass.launch(rt, &rt.indexing.copy[0], a, b, metadata())
                }
                (StorageRef::U32(a), StorageMut::U32(b)) => {
                    pass.launch(rt, &rt.indexing.copy[1], a, b, metadata())
                }
                (StorageRef::Low(a, d), StorageMut::Low(b, e)) if d == e => {
                    pass.launch(rt, &rt.low.copy, a, b, metadata())
                }
                _ => Err(CudaError::Dtype),
            },
            Operation::Unary { source, pass } => {
                pass.launch(rt, input(*source)?.f32()?, output.f32()?, metadata())
            }
            Operation::LowUnary { source, pass } => match (input(*source)?, output) {
                (StorageRef::Low(a, d), StorageMut::Low(b, e)) if d == e => {
                    pass.launch(rt, a, b, metadata())
                }
                _ => Err(CudaError::Dtype),
            },
            Operation::Binary { left, right, pass } => {
                match (input(*left)?, input(*right)?, output) {
                    (StorageRef::F32(a), StorageRef::F32(b), StorageMut::F32(c)) => {
                        pass.launch(rt, a, b, c, metadata())
                    }
                    (StorageRef::U32(a), StorageRef::U32(b), StorageMut::U32(c)) => {
                        pass.launch_u32(rt, a, b, c, metadata())
                    }
                    _ => Err(CudaError::Dtype),
                }
            }
            Operation::LowBinary { left, right, pass } => {
                match (input(*left)?, input(*right)?, output) {
                    (StorageRef::Low(a, d), StorageRef::Low(b, e), StorageMut::Low(c, f))
                        if d == e && e == f =>
                    {
                        pass.launch(rt, a, b, c, metadata())
                    }
                    _ => Err(CudaError::Dtype),
                }
            }
            Operation::Compare { left, right, pass } => {
                let out = output.u32()?;
                match (input(*left)?, input(*right)?) {
                    (StorageRef::F32(a), StorageRef::F32(b)) => pass.launch(
                        rt,
                        LoadedCompare {
                            function: &rt.indexing.compare[0],
                            dtype: None,
                        },
                        (a, b),
                        out,
                        metadata(),
                    ),
                    (StorageRef::U32(a), StorageRef::U32(b)) => pass.launch(
                        rt,
                        LoadedCompare {
                            function: &rt.indexing.compare[1],
                            dtype: None,
                        },
                        (a, b),
                        out,
                        metadata(),
                    ),
                    (StorageRef::Low(a, d), StorageRef::Low(b, e)) if d == e => pass.launch(
                        rt,
                        LoadedCompare {
                            function: &rt.low.compare,
                            dtype: Some(d as u32),
                        },
                        (a, b),
                        out,
                        metadata(),
                    ),
                    _ => Err(CudaError::Dtype),
                }
            }
            Operation::Select {
                mask,
                yes,
                no,
                pass,
            } => {
                let mask = input(*mask)?.u32()?;
                match (input(*yes)?, input(*no)?, output) {
                    (StorageRef::F32(yes), StorageRef::F32(no), StorageMut::F32(out)) => pass
                        .launch(
                            rt,
                            &rt.indexing.select[0],
                            SelectInputs { mask, yes, no },
                            out,
                            metadata(),
                        ),
                    (StorageRef::U32(yes), StorageRef::U32(no), StorageMut::U32(out)) => pass
                        .launch(
                            rt,
                            &rt.indexing.select[1],
                            SelectInputs { mask, yes, no },
                            out,
                            metadata(),
                        ),
                    (StorageRef::Low(yes, d), StorageRef::Low(no, e), StorageMut::Low(out, f))
                        if d == e && e == f =>
                    {
                        pass.launch(
                            rt,
                            &rt.low.select,
                            SelectInputs { mask, yes, no },
                            out,
                            metadata(),
                        )
                    }
                    _ => Err(CudaError::Dtype),
                }
            }
            Operation::Cast { source, pass, to } => match (to, input(*source)?, output) {
                (CudaDtype::F32, StorageRef::Low(a, _), StorageMut::F32(b)) => {
                    pass.launch_decode(rt, a, b, metadata())
                }
                (to, StorageRef::F32(a), StorageMut::Low(b, d)) if to.low_dtype() == Some(d) => {
                    pass.launch_encode(rt, a, b, metadata())
                }
                _ => Err(CudaError::Dtype),
            },
            Operation::Reduction { source, pass } => match (input(*source)?, output) {
                (StorageRef::F32(a), StorageMut::F32(b)) => pass.launch(
                    rt,
                    LoadedReduction {
                        function: if pass.is_all() {
                            &rt.reduction.all[0]
                        } else {
                            &rt.reduction.axes[0]
                        },
                        dtype: None,
                    },
                    a,
                    b,
                    metadata(),
                ),
                (StorageRef::U32(a), StorageMut::U32(b)) => pass.launch(
                    rt,
                    LoadedReduction {
                        function: if pass.is_all() {
                            &rt.reduction.all[1]
                        } else {
                            &rt.reduction.axes[1]
                        },
                        dtype: None,
                    },
                    a,
                    b,
                    metadata(),
                ),
                (StorageRef::Low(a, d), StorageMut::F32(b)) => pass.launch(
                    rt,
                    LoadedReduction {
                        function: if pass.is_all() {
                            &rt.low.reduce_all
                        } else {
                            &rt.low.reduce_axes
                        },
                        dtype: Some(d as u32),
                    },
                    a,
                    b,
                    metadata(),
                ),
                _ => Err(CudaError::Dtype),
            },
            Operation::Gemm { left, right, .. } => {
                let gemm = self.gemm.as_ref().expect("prepared GEMM exists");
                let out = output.f32()?;
                match (input(*left)?, input(*right)?) {
                    (StorageRef::F32(a), StorageRef::F32(b)) => rt.launch_gemm_f32(gemm, a, b, out),
                    (StorageRef::Low(a, d), StorageRef::Low(b, e)) if d == e => {
                        rt.launch_gemm_low(gemm, a, b, out)
                    }
                    _ => Err(CudaError::Dtype),
                }
            }
            Operation::Fill { count, value } => match output {
                StorageMut::F32(out) => dispatch::fill(rt, out, *count, *value as f32),
                StorageMut::U32(out) => {
                    dispatch::fill_typed(rt, &rt.reduction.fill[1], out, *count as u64, *value)
                }
                _ => Err(CudaError::Dtype),
            },
        }
    }
}
