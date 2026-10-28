#![cfg(not(target_arch = "wasm32"))]
use compute_mlx::{MlxBackend, MlxCompiledProgram, MlxError, MlxLowTensor, MlxTensor};
use tensor_core::{HasShape, LowDtype, ScatterOp, Shape, TensorLowBackend};
#[path = "compiled_scatter/boundaries.rs"]
mod boundaries;
#[path = "compiled_scatter/replay.rs"]
mod replay;
const OPS: [ScatterOp; 5] = [
    ScatterOp::Replace,
    ScatterOp::Add,
    ScatterOp::Multiply,
    ScatterOp::Min,
    ScatterOp::Max,
];
fn shape(d: &[usize]) -> Shape {
    Shape::new(d.to_vec()).unwrap()
}
fn backend() -> Option<MlxBackend> {
    match MlxBackend::new_gpu().and_then(|b| {
        if b.compile_available() {
            Ok(b)
        } else {
            Err(MlxError::CompileUnavailable)
        }
    }) {
        Ok(b) => Some(b),
        Err(e) => {
            eprintln!("MLX compiled scatter unavailable: {e}");
            assert!(std::env::var_os("COMPUTE_REQUIRE_MLX").is_none(), "{e}");
            None
        }
    }
}
fn traces(p: &MlxCompiledProgram, n: usize) {
    assert_eq!(
        p.trace_count(),
        if std::env::var_os("MLX_DISABLE_COMPILE").is_some() {
            n
        } else {
            1
        }
    );
}
fn encode(d: LowDtype, x: f32) -> u16 {
    match d {
        LowDtype::F16 => half::f16::from_f32(x).to_bits(),
        LowDtype::Bf16 => half::bf16::from_f32(x).to_bits(),
    }
}
fn decode(d: LowDtype, x: u16) -> f32 {
    match d {
        LowDtype::F16 => half::f16::from_bits(x).to_f32(),
        LowDtype::Bf16 => half::bf16::from_bits(x).to_f32(),
    }
}
fn index_values(run: usize) -> Vec<u32> {
    match run {
        0 => vec![0, 2, 2, 4, 1, 0],
        1 => vec![5, 2, u32::MAX, 5, 1, 9],
        2 => vec![u32::MAX; 6],
        _ => vec![4, 4, 3, 3, 2, 2],
    }
}
fn indices(b: &MlxBackend, values: &[u32], strided: bool) -> MlxTensor {
    if !strided {
        return b.upload_u32(shape(&[2, 3]), values).unwrap();
    }
    let physical: Vec<_> = (0..3)
        .flat_map(|c| (0..2).map(move |r| values[r * 3 + c]))
        .collect();
    b.permute(&b.upload_u32(shape(&[3, 2]), &physical).unwrap(), &[1, 0])
        .unwrap()
}
fn low_view(
    b: &MlxBackend,
    d: LowDtype,
    dims: &[usize],
    data: &[u16],
    strided: bool,
) -> MlxLowTensor {
    let input = b.upload_low(d, shape(dims), data).unwrap();
    if !strided {
        return input;
    }
    let mut axes: Vec<_> = (0..dims.len()).collect();
    axes.swap(0, dims.len() - 1);
    let t = b.permute_low(&input, &axes).unwrap();
    b.permute_low(&b.materialize_low(&t).unwrap(), &axes)
        .unwrap()
}
fn float_fold(op: ScatterOp, a: f32, b: f32) -> f32 {
    match op {
        ScatterOp::Replace => b,
        ScatterOp::Add => a + b,
        ScatterOp::Multiply => a * b,
        ScatterOp::Min => a.min(b),
        ScatterOp::Max => a.max(b),
    }
}
fn unsigned_fold(op: ScatterOp, a: u32, b: u32) -> u32 {
    match op {
        ScatterOp::Replace => b,
        ScatterOp::Add => a.wrapping_add(b),
        ScatterOp::Multiply => a.wrapping_mul(b),
        ScatterOp::Min => a.min(b),
        ScatterOp::Max => a.max(b),
    }
}
fn reference<T: Copy>(
    base: &[T],
    updates: &[T],
    idx: &[u32],
    op: ScatterOp,
    fold: impl Fn(ScatterOp, T, T) -> T,
) -> Vec<T> {
    let mut out = base.to_vec();
    for row in 0..2 {
        for (j, &dest) in idx.iter().enumerate() {
            if dest < 5 {
                let i = row * 5 + dest as usize;
                out[i] = fold(op, out[i], updates[row * 6 + j]);
            }
        }
    }
    out
}
