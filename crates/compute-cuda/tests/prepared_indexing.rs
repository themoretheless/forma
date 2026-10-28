use compute_cuda::{
    CudaDtype, CudaError, CudaPrepareOptions, CudaPreparedProgram, CudaProgramBuilder,
    CudaProgramInput, CudaProgramOutput, CudaRuntime, CudaValue,
};
use tensor_core::{
    LowDtype, ScanOptions, ScatterOp, Shape, TensorBackend, TensorIndexBackend, TensorLowBackend,
};
#[path = "prepared_indexing/movement.rs"]
mod movement;
#[path = "prepared_indexing/scans.rs"]
mod scans;
#[path = "prepared_indexing/scatter.rs"]
mod scatter;
type Result<T = ()> = std::result::Result<T, CudaError>;
fn shape(dims: &[usize]) -> Shape {
    Shape::new(dims.to_vec()).unwrap()
}
fn record_input(
    graph: &mut CudaProgramBuilder<'_>,
    value: &CudaProgramOutput,
) -> Result<CudaValue> {
    match value {
        CudaProgramOutput::F32(t) => graph.input(t.layout().clone()),
        CudaProgramOutput::U32(t) => graph.input_u32(t.layout().clone()),
        CudaProgramOutput::Low(t) => graph.input_low(t.low_dtype(), t.layout().clone()),
    }
}
fn upload_bits(
    rt: &CudaRuntime,
    dtype: CudaDtype,
    dims: &[usize],
    bits: &[u32],
) -> Result<CudaProgramOutput> {
    Ok(match dtype {
        CudaDtype::F32 => CudaProgramOutput::F32(rt.upload_f32(
            shape(dims),
            &bits.iter().map(|&x| f32::from_bits(x)).collect::<Vec<_>>(),
        )?),
        CudaDtype::U32 => CudaProgramOutput::U32(rt.upload_u32(shape(dims), bits)?),
        _ => CudaProgramOutput::Low(
            rt.upload_low(
                dtype.low_dtype().unwrap(),
                shape(dims),
                &bits
                    .iter()
                    .map(|&x| u16::try_from(x).unwrap())
                    .collect::<Vec<_>>(),
            )?,
        ),
    })
}
fn read_bits(rt: &CudaRuntime, value: &CudaProgramOutput) -> Result<Vec<u32>> {
    match value {
        CudaProgramOutput::F32(t) => Ok(rt.read_f32(t)?.into_iter().map(f32::to_bits).collect()),
        CudaProgramOutput::U32(t) => rt.read_u32(t),
        CudaProgramOutput::Low(t) => Ok(rt.read_low_bits(t)?.into_iter().map(u32::from).collect()),
    }
}
fn transpose(rt: &CudaRuntime, value: &CudaProgramOutput) -> Result<CudaProgramOutput> {
    Ok(match value {
        CudaProgramOutput::F32(t) => CudaProgramOutput::F32(rt.permute(t, &[1, 0])?),
        CudaProgramOutput::U32(t) => CudaProgramOutput::U32(rt.permute_u32(t, &[1, 0])?),
        CudaProgramOutput::Low(t) => CudaProgramOutput::Low(rt.permute_low(t, &[1, 0])?),
    })
}
fn narrow(
    rt: &CudaRuntime,
    value: &CudaProgramOutput,
    axis: usize,
    start: usize,
    len: usize,
) -> Result<CudaProgramOutput> {
    Ok(match value {
        CudaProgramOutput::F32(t) => CudaProgramOutput::F32(rt.narrow(t, axis, start, len)?),
        CudaProgramOutput::U32(t) => CudaProgramOutput::U32(rt.narrow(t, axis, start, len)?),
        CudaProgramOutput::Low(t) => CudaProgramOutput::Low(rt.narrow_low(t, axis, start, len)?),
    })
}
fn replay(
    program: &mut CudaPreparedProgram<'_>,
    inputs: &[CudaProgramInput<'_>],
    outputs: &mut [CudaProgramOutput],
) -> Result {
    program.run_typed_into(
        inputs,
        &mut outputs
            .iter_mut()
            .map(CudaProgramOutput::as_output_mut)
            .collect::<Vec<_>>(),
    )
}
fn modes() -> [ScanOptions; 4] {
    [
        ScanOptions {
            inclusive: true,
            reverse: false,
        },
        ScanOptions {
            inclusive: false,
            reverse: false,
        },
        ScanOptions {
            inclusive: true,
            reverse: true,
        },
        ScanOptions {
            inclusive: false,
            reverse: true,
        },
    ]
}

/// SKIP on a non-CUDA host is build evidence only. Required mode must fail;
/// it must never count an absent NVIDIA device as numerical qualification.
#[test]
fn native_cuda_prepared_indexing_contract() -> Result {
    let rt = match CudaRuntime::new() {
        Ok(rt) => rt,
        Err(error @ CudaError::Unavailable(_)) => {
            assert!(
                !["CUDA_REQUIRED", "COMPUTE_REQUIRE_CUDA"]
                    .iter()
                    .any(|name| std::env::var(name).as_deref() == Ok("1")),
                "required native prepared CUDA indexing execution unavailable: {error}"
            );
            eprintln!("SKIP native prepared CUDA indexing qualification: {error}");
            return Ok(());
        }
        Err(error) => return Err(error),
    };
    eprintln!("Prepared indexing CUDA hardware: {:?}", rt.capabilities());
    scans::float_and_unsigned(&rt)?;
    for dtype in [
        CudaDtype::F32,
        CudaDtype::U32,
        CudaDtype::F16,
        CudaDtype::Bf16,
    ] {
        movement::replay_and_empty(&rt, dtype)?;
        scatter::replay(&rt, dtype)?;
    }
    movement::large_compaction(&rt)?;
    for dtype in [LowDtype::F16, LowDtype::Bf16] {
        scans::low(&rt, dtype)?;
        scatter::low_arithmetic(&rt, dtype)?;
    }
    rt.synchronize()
}
