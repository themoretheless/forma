use super::super::{plan::allocation_bytes, preparation as prepared};
use super::transfers::TransferPlan;
use super::*;
use crate::matmul::{capture_default_workspace_bytes, capture_workspace_allocation_bytes};
use gpu_compute::cuda::graph::{CudaGraphApi, CudaGraphCapture};

fn add(a: usize, b: usize) -> Result<usize, CudaError> {
    a.checked_add(b).ok_or(CudaError::InvalidInput(
        "CUDA graph resource byte/count overflow",
    ))
}
fn account(
    expanded: &prepared::Expanded,
    input_copies: &[TransferPlan],
    output_copies: &[TransferPlan],
    workspace: usize,
    options: CudaCaptureOptions,
) -> Result<CudaGraphStats, CudaError> {
    let sum_slots = |specs: &[TensorSpec]| {
        specs.iter().try_fold(0, |bytes, spec| {
            add(bytes, allocation_bytes(spec.shape(), spec.dtype)?)
        })
    };
    let input_slot_bytes = sum_slots(&expanded.inputs)?;
    let output_slot_bytes = sum_slots(&expanded.outputs)?;
    let transfer_metadata_bytes = input_copies
        .iter()
        .chain(output_copies)
        .try_fold(0, |bytes, copy| add(bytes, copy.metadata_bytes()?))?;
    let metadata_bytes = add(expanded.stats.metadata_bytes, transfer_metadata_bytes)?;
    if metadata_bytes > options.preparation.max_metadata_bytes {
        return Err(CudaError::PreparationBudget {
            kind: "graph metadata",
            required: metadata_bytes,
            limit: options.preparation.max_metadata_bytes,
        });
    }
    let owned_bytes = [
        input_slot_bytes,
        output_slot_bytes,
        metadata_bytes,
        expanded.stats.scratch_bytes,
        workspace,
    ]
    .into_iter()
    .try_fold(0, add)?;
    if owned_bytes > options.max_owned_bytes {
        return Err(CudaError::PreparationBudget {
            kind: "graph owned storage",
            required: owned_bytes,
            limit: options.max_owned_bytes,
        });
    }
    Ok(CudaGraphStats {
        program: expanded.stats,
        input_slot_bytes,
        output_slot_bytes,
        transfer_metadata_bytes,
        workspace_bytes: workspace,
        owned_bytes,
        input_copy_launches: input_copies.iter().filter(|c| c.pass.count != 0).count(),
        output_copy_launches: output_copies.iter().filter(|c| c.pass.count != 0).count(),
    })
}
pub(super) fn capture<'rt>(
    prepared: CudaPreparedProgram<'rt>,
    options: CudaCaptureOptions,
) -> Result<CudaGraphProgram<'rt>, CudaError> {
    prepared.state.check()?;
    let runtime = prepared.runtime;
    let dense = prepared.logical.dense_inputs()?;
    let expanded = prepared::expand(
        dense.clone(),
        runtime.capabilities.multiprocessors,
        options.preparation,
        |request| request.validate(runtime),
    )?;
    let input_copies = prepared
        .inputs
        .iter()
        .map(TransferPlan::new)
        .collect::<Result<Vec<_>, _>>()?;
    let output_copies = expanded
        .outputs
        .iter()
        .map(TransferPlan::new)
        .collect::<Result<Vec<_>, _>>()?;
    let workspace = if expanded.stats.gemm_calls == 0 {
        None
    } else {
        Some(options.cublas_workspace_bytes.unwrap_or_else(|| {
            capture_default_workspace_bytes(runtime.capabilities.compute_capability)
        }))
    };
    let workspace_bytes = workspace
        .map(capture_workspace_allocation_bytes)
        .transpose()?
        .unwrap_or(0);
    let stats = account(
        &expanded,
        &input_copies,
        &output_copies,
        workspace_bytes,
        options,
    )?;
    let api = CudaGraphApi::load()?;
    let inputs = prepared.inputs.clone();
    let outputs = expanded.outputs.clone();
    let precisions = expanded.precisions.clone();
    // Both budget checks and all logical lowering precede new GPU resources.
    // Finish earlier direct submissions before retiring their owned scratch.
    runtime.synchronize()?;
    drop(prepared);
    let private = runtime.graph_runtime()?;
    let input_slots = expanded
        .inputs
        .iter()
        .map(|spec| CudaProgramOutput::zeros(&private, spec.shape().clone(), spec.dtype))
        .collect::<Result<Vec<_>, _>>()?;
    let output_slots = outputs
        .iter()
        .map(|spec| CudaProgramOutput::zeros(&private, spec.shape().clone(), spec.dtype))
        .collect::<Result<Vec<_>, _>>()?;
    let prepared = prepared::prepare_expanded(&private, dense, expanded)?;
    let mut instructions = prepared.instructions;
    let scratch = prepared.scratch;
    if let Some(workspace) = workspace {
        let handle = private.prepare_capture_blas(workspace)?;
        for instruction in &mut instructions {
            if let Some(gemm) = &mut instruction.gemm {
                gemm.attach_capture_blas(handle.clone())?;
            }
        }
    }
    let input_copies = input_copies
        .into_iter()
        .map(|p| p.prepare(&private))
        .collect::<Result<Vec<_>, _>>()?;
    let output_copies = output_copies
        .into_iter()
        .map(|p| p.prepare(&private))
        .collect::<Result<Vec<_>, _>>()?;
    let flags = gpu_compute::cuda::cudarc::driver::sys::CUevent_flags::CU_EVENT_DISABLE_TIMING;
    let input_ready = runtime.device.context.new_event(Some(flags))?;
    let output_ready = private.device.context.new_event(Some(flags))?;
    // Own everything before the first execution. Every error or panic from
    // warmup onward therefore uses the same synchronized retirement path.
    let mut result = CudaGraphProgram {
        runtime,
        resources: Some(Resources {
            graph: None,
            _instructions: instructions,
            _scratch: scratch,
            input_slots,
            output_slots,
            input_copies,
            output_copies,
            input_ready,
            output_ready,
            runtime: private,
        }),
        input_layouts: inputs.iter().map(|s| s.layout.clone()).collect(),
        output_shapes: outputs.iter().map(|s| s.shape().clone()).collect(),
        inputs,
        outputs,
        precisions,
        stats,
        state: ExecutionState::default(),
    };
    {
        let r = result.resources.as_mut().unwrap();
        let arguments = r
            .input_slots
            .iter()
            .map(CudaProgramOutput::as_input)
            .collect::<Vec<_>>();
        let mut destinations = r
            .output_slots
            .iter_mut()
            .map(CudaProgramOutput::as_output_mut)
            .collect::<Vec<_>>();
        // Finish first-use kernel/library setup and metadata transfers before capture.
        execution::execute(
            &r.runtime,
            &r._instructions,
            &mut r._scratch,
            &arguments,
            &mut destinations,
        )?;
        r.runtime.synchronize()?;
        // SAFETY: only private no-event buffers enter the capture. No host
        // transfers, synchronization, allocation or runtime mutation in execute.
        // The capture guard is dropped before result on every failure/unwind.
        let capture = unsafe { CudaGraphCapture::begin(r.runtime.device.stream.clone(), &api)? };
        if let Err(error) = execution::execute(
            &r.runtime,
            &r._instructions,
            &mut r._scratch,
            &arguments,
            &mut destinations,
        ) {
            capture.abort()?;
            return Err(error);
        }
        r.graph = Some(capture.finish()?);
    }
    // SAFETY: result now owns every captured resource, including on failure.
    unsafe {
        result
            .resources
            .as_mut()
            .unwrap()
            .graph
            .as_mut()
            .unwrap()
            .upload()?;
    }
    // Upload is a one-time preparation operation; nothing remains pending at return.
    result.synchronize()?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::super::super::{builder::CudaProgramPlanBuilder, preparation::expand};
    use super::*;
    fn shape(d: &[usize]) -> Shape {
        Shape::new(d.to_vec()).unwrap()
    }
    #[test]
    fn dense_slots_are_budgeted_by_logical_size_and_every_copy_metadata_byte() {
        let layout = Layout::new(shape(&[2, 3]), vec![1_000_000, 1], 123).unwrap();
        let mut builder = CudaProgramPlanBuilder::new();
        let x = builder.input(layout.clone()).unwrap();
        let original = builder.finish(&[x, x]).unwrap();
        let dense = original.dense_inputs().unwrap();
        let expanded = expand(dense, 1, CudaPrepareOptions::default(), |_| Ok(())).unwrap();
        let inputs = original
            .inputs
            .iter()
            .map(TransferPlan::new)
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        let outputs = expanded
            .outputs
            .iter()
            .map(TransferPlan::new)
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        let stats = account(
            &expanded,
            &inputs,
            &outputs,
            0,
            CudaCaptureOptions::default(),
        )
        .unwrap();
        assert_eq!((stats.input_slot_bytes, stats.output_slot_bytes), (24, 48));
        assert_eq!(stats.transfer_metadata_bytes, 96);
        assert_eq!(
            (stats.input_copy_launches, stats.output_copy_launches),
            (1, 2)
        );
        assert_eq!(
            stats.owned_bytes,
            stats.input_slot_bytes
                + stats.output_slot_bytes
                + stats.transfer_metadata_bytes
                + stats.program.metadata_bytes
                + stats.program.scratch_bytes
        );
        let mut options = CudaCaptureOptions {
            max_owned_bytes: stats.owned_bytes - 1,
            ..Default::default()
        };
        assert!(matches!(
            account(&expanded, &inputs, &outputs, 0, options),
            Err(CudaError::PreparationBudget {
                kind: "graph owned storage",
                ..
            })
        ));
        options.max_owned_bytes = stats.owned_bytes;
        assert!(account(&expanded, &inputs, &outputs, 0, options).is_ok());
        options.preparation.max_metadata_bytes =
            stats.program.metadata_bytes + stats.transfer_metadata_bytes - 1;
        assert!(matches!(
            account(&expanded, &inputs, &outputs, 0, options),
            Err(CudaError::PreparationBudget {
                kind: "graph metadata",
                ..
            })
        ));
    }
    #[test]
    fn empty_sentinels_and_scalar_copy_metadata_are_counted_without_unused_launches() {
        let mut b = CudaProgramPlanBuilder::new();
        let empty = b
            .input_low(
                tensor_core::LowDtype::Bf16,
                Layout::contiguous(shape(&[0])).unwrap(),
            )
            .unwrap();
        let scalar = b
            .input_u32(Layout::contiguous(shape(&[])).unwrap())
            .unwrap();
        let plan = b.finish(&[empty, scalar]).unwrap();
        let expanded = expand(plan.clone(), 1, CudaPrepareOptions::default(), |_| Ok(())).unwrap();
        let input = plan
            .inputs
            .iter()
            .map(TransferPlan::new)
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        let output = expanded
            .outputs
            .iter()
            .map(TransferPlan::new)
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        let stats = account(&expanded, &input, &output, 0, CudaCaptureOptions::default()).unwrap();
        assert_eq!((stats.input_slot_bytes, stats.output_slot_bytes), (6, 6));
        assert_eq!(stats.transfer_metadata_bytes, 16);
        assert_eq!(
            (stats.input_copy_launches, stats.output_copy_launches),
            (1, 1)
        );
        assert!(
            account(
                &expanded,
                &input,
                &output,
                usize::MAX,
                CudaCaptureOptions::default()
            )
            .is_err()
        );
    }
}
