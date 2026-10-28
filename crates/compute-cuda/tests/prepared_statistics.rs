use compute_cuda::{
    CudaError, CudaPrepareOptions, CudaProgramBuilder, CudaProgramOutput, CudaRuntime, CudaValue,
};
use tensor_core::{
    AttentionMask, AttentionOptions, LowDtype, ScanOptions, Shape, TensorBackend,
    TensorIndexBackend, TensorLowBackend,
};
#[path = "prepared_statistics/attention.rs"]
mod attention;
#[path = "prepared_statistics/statistics.rs"]
mod statistics;
#[path = "prepared_statistics/support.rs"]
mod support;
use support::*;
type Result<T = ()> = std::result::Result<T, CudaError>;
fn shape(d: &[usize]) -> Shape {
    Shape::new(d.to_vec()).unwrap()
}
fn record(b: &mut CudaProgramBuilder<'_>, x: &CudaProgramOutput) -> Result<CudaValue> {
    match x {
        CudaProgramOutput::F32(t) => b.input(t.layout().clone()),
        CudaProgramOutput::Low(t) => b.input_low(t.low_dtype(), t.layout().clone()),
        CudaProgramOutput::U32(t) => b.input_u32(t.layout().clone()),
    }
}
fn upload(
    rt: &CudaRuntime,
    dtype: Option<LowDtype>,
    dims: &[usize],
    data: &[f32],
) -> Result<CudaProgramOutput> {
    Ok(if let Some(d) = dtype {
        CudaProgramOutput::Low(rt.upload_low(
            d,
            shape(dims),
            &data.iter().map(|&x| encode(d, x)).collect::<Vec<_>>(),
        )?)
    } else {
        CudaProgramOutput::F32(rt.upload_f32(shape(dims), data)?)
    })
}
fn permute(rt: &CudaRuntime, x: &CudaProgramOutput, axes: &[usize]) -> Result<CudaProgramOutput> {
    Ok(match x {
        CudaProgramOutput::F32(t) => CudaProgramOutput::F32(rt.permute(t, axes)?),
        CudaProgramOutput::Low(t) => CudaProgramOutput::Low(rt.permute_low(t, axes)?),
        _ => unreachable!(),
    })
}
#[test]
fn native_cuda_prepared_statistics_attention_contract() -> Result {
    let rt = match CudaRuntime::new() {
        Ok(rt) => rt,
        Err(e @ CudaError::Unavailable(_)) => {
            assert!(
                !["CUDA_REQUIRED", "COMPUTE_REQUIRE_CUDA"]
                    .iter()
                    .any(|v| std::env::var(v).as_deref() == Ok("1")),
                "required native CUDA statistics/attention unavailable: {e}"
            );
            eprintln!("SKIP native CUDA prepared statistics/attention: {e}");
            return Ok(());
        }
        Err(e) => return Err(e),
    };
    eprintln!(
        "Prepared statistics/attention hardware: {:?}",
        rt.capabilities()
    );
    for dtype in [None, Some(LowDtype::F16), Some(LowDtype::Bf16)] {
        statistics::replay(&rt, dtype)?;
        statistics::singleton_empty(&rt, dtype)?;
        attention::replay(&rt, dtype)?;
        attention::zero_keys(&rt, dtype)?;
    }
    statistics::extremes(&rt)?;
    attention::tiny_products(&rt)?;
    rt.synchronize()
}
