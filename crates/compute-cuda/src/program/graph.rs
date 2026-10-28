//! CUDA Graph execution with private fixed-address storage and explicit bridges.
use super::{
    CudaPrepareOptions, CudaProgramStats,
    execution::{self, CudaPreparedProgram, Instruction},
    gate::ExecutionState,
    operation::MatmulRequest,
    plan::{CudaDtype, TensorSpec},
    storage::Storage,
    typed::{CudaProgramInput, CudaProgramOutput, CudaProgramOutputMut},
    validation,
};
use crate::{CudaError, CudaRuntime, CudaTensor};
use gpu_compute::cuda::{cudarc::driver::CudaEvent, graph::CudaGraph};
use tensor_core::{Layout, Shape};
mod preparation;
mod transfers;
use transfers::Transfer;

/// Capture limits include dense slots, scratch, all adapter metadata and the
/// padded cuBLAS workspace. Driver graph/module/library bookkeeping is excluded.
#[derive(Clone, Copy, Debug)]
pub struct CudaCaptureOptions {
    pub preparation: CudaPrepareOptions,
    pub max_owned_bytes: usize,
    /// None selects the cuBLAS workspace recommendation for this architecture.
    /// This option is unused when the lowered schedule contains no active GEMM.
    pub cublas_workspace_bytes: Option<usize>,
}
impl Default for CudaCaptureOptions {
    fn default() -> Self {
        Self {
            preparation: CudaPrepareOptions::default(),
            max_owned_bytes: usize::MAX,
            cublas_workspace_bytes: None,
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CudaGraphStats {
    /// Calls in the dense captured schedule, not a profiler's native node count.
    pub program: CudaProgramStats,
    pub input_slot_bytes: usize,
    pub output_slot_bytes: usize,
    pub transfer_metadata_bytes: usize,
    pub workspace_bytes: usize,
    pub owned_bytes: usize,
    /// Additional kernels outside the graph for materialization and snapshots.
    pub input_copy_launches: usize,
    pub output_copy_launches: usize,
}

// Field order is intentional: destroy graph handles before referenced resources.
struct Resources {
    graph: Option<CudaGraph>,
    _instructions: Vec<Instruction>,
    _scratch: Vec<Option<Storage>>,
    input_slots: Vec<CudaProgramOutput>,
    output_slots: Vec<CudaProgramOutput>,
    input_copies: Vec<Transfer>,
    output_copies: Vec<Transfer>,
    input_ready: CudaEvent,
    output_ready: CudaEvent,
    runtime: CudaRuntime,
}

/// Serialized graph replay. Slots are private; results are independent snapshots.
/// Input/output copies and two event bridges run outside capture. Enqueue and
/// synchronization errors and enqueue panics poison future replay. Drop waits
/// for both streams.
/// The driver graph intentionally makes this type neither Send nor Sync.
pub struct CudaGraphProgram<'rt> {
    runtime: &'rt CudaRuntime,
    resources: Option<Resources>,
    inputs: Vec<TensorSpec>,
    outputs: Vec<TensorSpec>,
    input_layouts: Vec<Layout>,
    output_shapes: Vec<Shape>,
    precisions: Vec<MatmulRequest>,
    stats: CudaGraphStats,
    state: ExecutionState,
}
impl<'rt> CudaPreparedProgram<'rt> {
    /// Consume a prepared program and capture a schedule rebuilt for private
    /// dense slots. Existing pending work is completed before its storage is freed.
    /// Requires the runtime's device primary context; no global tracking is changed.
    pub fn capture_owned(
        self,
        options: CudaCaptureOptions,
    ) -> Result<CudaGraphProgram<'rt>, CudaError> {
        preparation::capture(self, options)
    }
}
impl CudaGraphProgram<'_> {
    pub fn stats(&self) -> CudaGraphStats {
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

    pub fn run(&mut self, inputs: &[&CudaTensor]) -> Result<Vec<CudaTensor>, CudaError> {
        validation::legacy_f32(&self.inputs, &self.outputs)?;
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
        validation::legacy_f32(&self.inputs, &self.outputs)?;
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
    /// Allocate independent outputs only after validating all input signatures.
    pub fn run_typed(
        &mut self,
        inputs: &[CudaProgramInput<'_>],
    ) -> Result<Vec<CudaProgramOutput>, CudaError> {
        self.state.check()?;
        execution::validate_inputs(self.runtime, &self.inputs, &self.precisions, inputs)?;
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
    /// Validate every binding and precision before any copy, event or graph launch.
    /// No adapter device allocation, metadata upload or event creation per replay.
    pub fn run_typed_into(
        &mut self,
        inputs: &[CudaProgramInput<'_>],
        outputs: &mut [CudaProgramOutputMut<'_>],
    ) -> Result<(), CudaError> {
        let runtime = self.runtime;
        let permit = self.state.begin(|| {
            execution::validate_inputs(runtime, &self.inputs, &self.precisions, inputs)?;
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
        let r = self.resources.as_mut().expect("live graph resources");
        permit.execute(|| {
            for ((copy, input), slot) in r.input_copies.iter().zip(inputs).zip(&mut r.input_slots) {
                copy.launch(
                    runtime,
                    input.storage(),
                    slot.as_output_mut().storage_mut()?,
                )?;
            }
            // Copies use public tensors' ordinary access guards on their own
            // runtime stream. Only private buffers participate in graph capture.
            r.input_ready.record(&runtime.device.stream)?;
            r.runtime.device.stream.wait(&r.input_ready)?;
            // SAFETY: all captured addresses, functions and library owners remain
            // private and live in r. The graph has a single exclusive &mut caller.
            unsafe {
                r.graph.as_mut().expect("captured graph").launch()?;
            }
            r.output_ready.record(&r.runtime.device.stream)?;
            runtime.device.stream.wait(&r.output_ready)?;
            for ((copy, slot), output) in r.output_copies.iter().zip(&r.output_slots).zip(outputs) {
                copy.launch(runtime, slot.as_input().storage(), output.storage_mut()?)?;
            }
            Ok(())
        })
    }
    pub fn synchronize(&mut self) -> Result<(), CudaError> {
        let runtime = self.runtime;
        let private = &self
            .resources
            .as_ref()
            .expect("live graph resources")
            .runtime;
        self.state.begin(|| Ok(()))?.execute(|| {
            let public_result = runtime.synchronize();
            let private_result = private.synchronize();
            public_result.and(private_result)
        })
    }
}
impl Drop for CudaGraphProgram<'_> {
    fn drop(&mut self) {
        if let Some(resources) = self.resources.take() {
            let public = self.runtime.synchronize();
            let private = resources.runtime.synchronize();
            if public.is_err() || private.is_err() {
                // Completion cannot be proved. Retain every captured owner rather
                // than free storage a device may still reference after an error.
                std::mem::forget(resources);
            }
        }
    }
}
