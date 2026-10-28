use compute_cuda::{
    CudaCaptureOptions, CudaDtype, CudaError, CudaGraphProgram, CudaPrepareOptions,
    CudaProgramBuilder, CudaProgramOutput, CudaRuntime, CudaValue,
};
use tensor_core::{
    AttentionMask, AttentionOptions, LowDtype, MatmulPrecision, ScanOptions, ScatterOp, Shape,
    TensorBackend, TensorIndexBackend, TensorLowBackend, UnaryOp,
};
// Reuse the already independently tested arithmetic codec/statistics oracle.
#[path = "graph/matmul.rs"]
mod matmul;
#[path = "graph/movement.rs"]
mod movement;
#[path = "prepared_statistics/support.rs"]
mod reference;
#[path = "graph/statistics.rs"]
mod statistics;
#[path = "graph/validation.rs"]
mod validation;
use reference::*;
type Result<T = ()> = std::result::Result<T, CudaError>;
fn shape(dims: &[usize]) -> Shape {
    Shape::new(dims.to_vec()).unwrap()
}
fn record(b: &mut CudaProgramBuilder<'_>, x: &CudaProgramOutput) -> Result<CudaValue> {
    match x {
        CudaProgramOutput::F32(t) => b.input(t.layout().clone()),
        CudaProgramOutput::U32(t) => b.input_u32(t.layout().clone()),
        CudaProgramOutput::Low(t) => b.input_low(t.low_dtype(), t.layout().clone()),
    }
}
fn upload(
    rt: &CudaRuntime,
    dtype: CudaDtype,
    dims: &[usize],
    words: &[u32],
) -> Result<CudaProgramOutput> {
    Ok(match dtype {
        CudaDtype::F32 => CudaProgramOutput::F32(
            rt.upload_f32(
                shape(dims),
                &words
                    .iter()
                    .copied()
                    .map(f32::from_bits)
                    .collect::<Vec<_>>(),
            )?,
        ),
        CudaDtype::U32 => CudaProgramOutput::U32(rt.upload_u32(shape(dims), words)?),
        _ => CudaProgramOutput::Low(
            rt.upload_low(
                dtype.low_dtype().unwrap(),
                shape(dims),
                &words
                    .iter()
                    .map(|&x| u16::try_from(x).unwrap())
                    .collect::<Vec<_>>(),
            )?,
        ),
    })
}
fn upload_float(
    rt: &CudaRuntime,
    dtype: Option<LowDtype>,
    dims: &[usize],
    data: &[f32],
) -> Result<CudaProgramOutput> {
    match dtype {
        None => Ok(CudaProgramOutput::F32(rt.upload_f32(shape(dims), data)?)),
        Some(t) => Ok(CudaProgramOutput::Low(rt.upload_low(
            t,
            shape(dims),
            &data.iter().map(|&x| encode(t, x)).collect::<Vec<_>>(),
        )?)),
    }
}
fn read(rt: &CudaRuntime, x: &CudaProgramOutput) -> Result<Vec<u32>> {
    match x {
        CudaProgramOutput::F32(t) => Ok(rt.read_f32(t)?.into_iter().map(f32::to_bits).collect()),
        CudaProgramOutput::U32(t) => rt.read_u32(t),
        CudaProgramOutput::Low(t) => Ok(rt.read_low_bits(t)?.into_iter().map(u32::from).collect()),
    }
}
fn transpose(rt: &CudaRuntime, x: &CudaProgramOutput) -> Result<CudaProgramOutput> {
    Ok(match x {
        CudaProgramOutput::F32(t) => CudaProgramOutput::F32(rt.permute(t, &[1, 0])?),
        CudaProgramOutput::U32(t) => CudaProgramOutput::U32(rt.permute_u32(t, &[1, 0])?),
        CudaProgramOutput::Low(t) => CudaProgramOutput::Low(rt.permute_low(t, &[1, 0])?),
    })
}
fn replay(
    graph: &mut CudaGraphProgram<'_>,
    inputs: &[&CudaProgramOutput],
    outputs: &mut [CudaProgramOutput],
) -> Result {
    graph.run_typed_into(
        &inputs.iter().map(|x| x.as_input()).collect::<Vec<_>>(),
        &mut outputs
            .iter_mut()
            .map(CudaProgramOutput::as_output_mut)
            .collect::<Vec<_>>(),
    )
}

/// Compilation on a host without NVIDIA is not numerical qualification.
#[test]
fn native_cuda_owned_graph_contract() -> Result {
    let rt = match CudaRuntime::new() {
        Ok(rt) => rt,
        Err(e @ CudaError::Unavailable(_)) => {
            assert!(
                !["CUDA_REQUIRED", "COMPUTE_REQUIRE_CUDA"]
                    .iter()
                    .any(|name| std::env::var(name).as_deref() == Ok("1")),
                "required native CUDA graph unavailable: {e}"
            );
            eprintln!("SKIP native CUDA graph: {e}");
            return Ok(());
        }
        Err(e) => return Err(e),
    };
    eprintln!("CUDA Graph hardware: {:?}", rt.capabilities());
    matmul::strided_views_and_gemm(&rt, None)?;
    for dtype in [LowDtype::F16, LowDtype::Bf16] {
        if rt.low_precision_support(dtype).matmul_f32 {
            matmul::strided_views_and_gemm(&rt, Some(dtype))?;
        }
    }
    matmul::precision_modes(&rt)?;
    for dtype in [
        CudaDtype::F32,
        CudaDtype::U32,
        CudaDtype::F16,
        CudaDtype::Bf16,
    ] {
        movement::replay_and_counters(&rt, dtype)?;
    }
    for dtype in [None, Some(LowDtype::F16), Some(LowDtype::Bf16)] {
        statistics::resident_statistics_attention(&rt, dtype)?;
    }
    validation::rejection_budget_empty_and_lifetime(&rt)?;
    rt.synchronize()
}
