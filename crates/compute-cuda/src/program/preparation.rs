pub(super) use super::expansion::expand;
use super::{
    CudaPrepareOptions, CudaProgramStats,
    execution::{CudaPreparedProgram, Instruction},
    operation::{Destination, MatmulRequest, Operation, Scheduled},
    plan::{CudaDtype, CudaProgramPlan, PlannedValue, TensorSpec, allocation_bytes},
    storage::{ScratchSpec, Storage},
};
use crate::{CudaError, CudaRuntime, indexing::dispatch::CopyDispatch};
use tensor_core::{Layout, Shape};

pub(super) struct Expanded {
    pub inputs: Vec<TensorSpec>,
    pub outputs: Vec<TensorSpec>,
    pub precisions: Vec<MatmulRequest>,
    pub scratch: Vec<ScratchSpec>,
    pub schedule: Vec<Scheduled>,
    pub stats: CudaProgramStats,
}
impl Expanded {
    pub(super) fn scratch_f64(&mut self, shape: Shape) -> Result<usize, CudaError> {
        let spec = ScratchSpec::f64(shape)?;
        let index = self.scratch.len();
        self.scratch.push(spec);
        Ok(index)
    }
    pub(super) fn scratch(&mut self, shape: Shape, dtype: CudaDtype) -> Result<usize, CudaError> {
        allocation_bytes(&shape, dtype)?;
        let index = self.scratch.len();
        self.scratch.push(ScratchSpec::new(TensorSpec {
            layout: Layout::contiguous(shape)?,
            dtype,
        }));
        Ok(index)
    }
    pub(super) fn add(&mut self, output: usize, operation: Operation) {
        self.schedule.push(Scheduled {
            destination: Destination::Scratch(output),
            operation,
        });
    }
    pub(super) fn copy(
        &mut self,
        source: PlannedValue,
        destination: Destination,
    ) -> Result<(), CudaError> {
        if source.layout.shape().is_empty() {
            return Ok(());
        }
        let pass = match source.dtype {
            CudaDtype::F32 => CopyDispatch::new::<f32>(&source.layout)?,
            CudaDtype::U32 => CopyDispatch::new::<u32>(&source.layout)?,
            _ => CopyDispatch::new::<u16>(&source.layout)?,
        };
        self.schedule.push(Scheduled {
            destination,
            operation: Operation::Copy {
                source: source.buffer,
                pass,
            },
        });
        Ok(())
    }
    pub(super) fn account(&mut self, options: CudaPrepareOptions) -> Result<(), CudaError> {
        let add = |a: usize, b: usize| {
            a.checked_add(b).ok_or(CudaError::InvalidInput(
                "CUDA preparation byte/count overflow",
            ))
        };
        for spec in &self.scratch {
            self.stats.scratch_bytes = add(self.stats.scratch_bytes, spec.bytes()?)?;
        }
        for step in &self.schedule {
            if let Some(metadata) = step.operation.metadata() {
                let bytes = metadata
                    .len()
                    .max(1)
                    .checked_mul(8)
                    .ok_or(CudaError::InvalidInput("CUDA metadata byte count overflow"))?;
                self.stats.metadata_bytes = add(self.stats.metadata_bytes, bytes)?;
            }
            match &step.operation {
                Operation::Gemm { plan, .. } => {
                    self.stats.gemm_calls = add(self.stats.gemm_calls, plan.calls())?
                }
                Operation::Zero => self.stats.memset_calls = add(self.stats.memset_calls, 1)?,
                _ => self.stats.kernel_launches = add(self.stats.kernel_launches, 1)?,
            }
        }
        for (kind, required, limit) in [
            (
                "scratch",
                self.stats.scratch_bytes,
                options.max_scratch_bytes,
            ),
            (
                "metadata",
                self.stats.metadata_bytes,
                options.max_metadata_bytes,
            ),
        ] {
            if required > limit {
                return Err(CudaError::PreparationBudget {
                    kind,
                    required,
                    limit,
                });
            }
        }
        Ok(())
    }
}
pub(super) fn prepare(
    runtime: &CudaRuntime,
    plan: CudaProgramPlan,
    options: CudaPrepareOptions,
) -> Result<CudaPreparedProgram<'_>, CudaError> {
    let expanded = expand(
        plan.clone(),
        runtime.capabilities.multiprocessors,
        options,
        |request| request.validate(runtime),
    )?;
    prepare_expanded(runtime, plan, expanded)
}
pub(super) fn prepare_expanded(
    runtime: &CudaRuntime,
    plan: CudaProgramPlan,
    expanded: Expanded,
) -> Result<CudaPreparedProgram<'_>, CudaError> {
    // All dtype/layout/policy/budget checks precede device allocation and BLAS setup.
    let mut instructions = Vec::with_capacity(expanded.schedule.len());
    for step in expanded.schedule {
        let gemm = match &step.operation {
            Operation::Gemm { plan, request, .. } => Some(request.prepare(runtime, plan.clone())?),
            _ => None,
        };
        let metadata = step
            .operation
            .metadata()
            .map(|words| runtime.metadata(words))
            .transpose()?;
        instructions.push(Instruction {
            destination: step.destination,
            operation: step.operation,
            metadata,
            gemm,
        });
    }
    let scratch = expanded
        .scratch
        .iter()
        .map(|spec| Storage::zeros(runtime, spec).map(Some))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(CudaPreparedProgram {
        logical: plan,
        runtime,
        input_layouts: expanded.inputs.iter().map(|s| s.layout.clone()).collect(),
        output_shapes: expanded.outputs.iter().map(|s| s.shape().clone()).collect(),
        inputs: expanded.inputs,
        outputs: expanded.outputs,
        precisions: expanded.precisions,
        instructions,
        scratch,
        stats: expanded.stats,
        state: Default::default(),
    })
}
