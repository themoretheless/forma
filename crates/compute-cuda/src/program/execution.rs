use super::{
    CudaProgramStats,
    gate::ExecutionState,
    operation::{Destination, MatmulRequest, Operation},
    plan::{CudaDtype, TensorSpec},
    storage::Storage,
    typed::{CudaProgramInput, CudaProgramOutput, CudaProgramOutputMut},
    validation,
};
use crate::{CudaError, CudaRuntime, CudaTensor, matmul::PreparedGemm};
use gpu_compute::cuda::CudaSlice;
use tensor_core::{Layout, Shape};

pub(super) struct Instruction {
    pub destination: Destination,
    pub operation: Operation,
    pub metadata: Option<CudaSlice<u64>>,
    pub gemm: Option<PreparedGemm>,
}
/// A typed, fixed-layout launch sequence. No caller allocations are retained.
/// Enqueue or explicit synchronization errors permanently poison the program.
pub struct CudaPreparedProgram<'rt> {
    pub(super) logical: super::plan::CudaProgramPlan,
    pub(super) runtime: &'rt CudaRuntime,
    pub(super) inputs: Vec<TensorSpec>,
    pub(super) outputs: Vec<TensorSpec>,
    pub(super) input_layouts: Vec<Layout>,
    pub(super) output_shapes: Vec<Shape>,
    pub(super) precisions: Vec<MatmulRequest>,
    pub(super) instructions: Vec<Instruction>,
    pub(super) scratch: Vec<Option<Storage>>,
    pub(super) stats: CudaProgramStats,
    pub(super) state: ExecutionState,
}
impl CudaPreparedProgram<'_> {
    pub fn stats(&self) -> CudaProgramStats {
        self.stats
    }
    pub fn is_poisoned(&self) -> bool {
        self.state.poisoned
    }
    pub fn input_layouts(&self) -> &[Layout] {
        &self.input_layouts
    }
    pub fn output_shapes(&self) -> &[Shape] {
        &self.output_shapes
    }
    pub fn input_dtypes(&self) -> Vec<CudaDtype> {
        self.inputs.iter().map(|s| s.dtype).collect()
    }
    pub fn output_dtypes(&self) -> Vec<CudaDtype> {
        self.outputs.iter().map(|s| s.dtype).collect()
    }
    fn require_f32(&self) -> Result<(), CudaError> {
        validation::legacy_f32(&self.inputs, &self.outputs)
    }

    /// Backward-compatible f32 signatures only. Typed internals with exclusively
    /// f32 inputs/outputs remain valid; use run_typed for all other signatures.
    pub fn run(&mut self, inputs: &[&CudaTensor]) -> Result<Vec<CudaTensor>, CudaError> {
        self.require_f32()?;
        self.run_typed(
            &inputs
                .iter()
                .map(|t| CudaProgramInput::F32(t))
                .collect::<Vec<_>>(),
        )?
        .into_iter()
        .map(CudaProgramOutput::into_f32)
        .collect()
    }
    pub fn run_into(
        &mut self,
        inputs: &[&CudaTensor],
        outputs: &mut [&mut CudaTensor],
    ) -> Result<(), CudaError> {
        self.require_f32()?;
        self.run_typed_into(
            &inputs
                .iter()
                .map(|t| CudaProgramInput::F32(t))
                .collect::<Vec<_>>(),
            &mut outputs
                .iter_mut()
                .map(|t| CudaProgramOutputMut::F32(t))
                .collect::<Vec<_>>(),
        )
    }
    /// Validate first, then allocate fresh independent typed outputs. Returned
    /// tensors remain unchanged by future replay and outlive the program.
    pub fn run_typed(
        &mut self,
        inputs: &[CudaProgramInput<'_>],
    ) -> Result<Vec<CudaProgramOutput>, CudaError> {
        self.state.check()?;
        validate_inputs(self.runtime, &self.inputs, &self.precisions, inputs)?;
        let mut outputs = self
            .outputs
            .iter()
            .map(|s| CudaProgramOutput::zeros(self.runtime, s.shape().clone(), s.dtype))
            .collect::<Result<Vec<_>, _>>()?;
        self.run_typed_into(
            inputs,
            &mut outputs
                .iter_mut()
                .map(CudaProgramOutput::as_output_mut)
                .collect::<Vec<_>>(),
        )?;
        Ok(outputs)
    }
    /// No adapter device tensor allocation, metadata upload or recompilation.
    /// cuBLAS implementation/workspace allocation remains library-controlled.
    pub fn run_typed_into(
        &mut self,
        inputs: &[CudaProgramInput<'_>],
        outputs: &mut [CudaProgramOutputMut<'_>],
    ) -> Result<(), CudaError> {
        let runtime = self.runtime;
        let permit = self.state.begin(|| {
            validate_inputs(runtime, &self.inputs, &self.precisions, inputs)?;
            for output in outputs.iter() {
                output.check(runtime)?;
            }
            validation::outputs(
                &self.outputs,
                &inputs
                    .iter()
                    .map(CudaProgramInput::binding)
                    .collect::<Vec<_>>(),
                &outputs
                    .iter()
                    .map(CudaProgramOutputMut::binding)
                    .collect::<Vec<_>>(),
            )
        })?;
        permit.execute(|| {
            execute(
                runtime,
                &self.instructions,
                &mut self.scratch,
                inputs,
                outputs,
            )
        })
    }
    /// Observe deferred stream errors and poison this program on failure.
    pub fn synchronize(&mut self) -> Result<(), CudaError> {
        let rt = self.runtime;
        self.state.begin(|| Ok(()))?.execute(|| rt.synchronize())
    }
}
pub(super) fn validate_inputs(
    rt: &CudaRuntime,
    expected: &[TensorSpec],
    precisions: &[MatmulRequest],
    inputs: &[CudaProgramInput<'_>],
) -> Result<(), CudaError> {
    for input in inputs {
        input.check(rt)?;
    }
    validation::inputs(
        expected,
        &inputs
            .iter()
            .map(CudaProgramInput::binding)
            .collect::<Vec<_>>(),
    )?;
    validation::precisions(precisions, |request| request.validate(rt))
}
pub(super) fn execute(
    rt: &CudaRuntime,
    instructions: &[Instruction],
    scratch: &mut [Option<Storage>],
    inputs: &[CudaProgramInput<'_>],
    outputs: &mut [CudaProgramOutputMut<'_>],
) -> Result<(), CudaError> {
    for instruction in instructions {
        match instruction.destination {
            Destination::Scratch(index) => {
                super::scratch::with_outputs(
                    scratch,
                    index,
                    instruction.operation.auxiliary_destination(),
                    |sources, output, auxiliary| {
                        instruction.launch(
                            rt,
                            inputs,
                            sources,
                            output.as_mut(),
                            auxiliary.map(Storage::as_mut),
                        )
                    },
                )?;
            }
            Destination::Output(index) => {
                if instruction.operation.auxiliary_destination().is_some() {
                    return Err(CudaError::InvalidInput(
                        "terminal instruction has auxiliary scratch",
                    ));
                }
                instruction.launch(rt, inputs, scratch, outputs[index].storage_mut()?, None)?
            }
        }
    }
    Ok(())
}
