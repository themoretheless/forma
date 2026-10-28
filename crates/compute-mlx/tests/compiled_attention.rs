#![cfg(not(target_arch = "wasm32"))]
use compute_mlx::{MlxBackend, MlxCompiledProgram, MlxError, MlxTensor};
use tensor_core::{
    AttentionMask, AttentionOptions, HasShape, LowDtype, Shape, TensorAttentionBackend,
    TensorLowBackend,
};

#[path = "compiled_attention/boundaries.rs"]
mod boundaries;
#[path = "compiled_attention/replay.rs"]
mod replay;

fn shape(dims: &[usize]) -> Shape {
    Shape::new(dims.to_vec()).unwrap()
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
            eprintln!("MLX compiled attention unavailable: {e}");
            assert!(std::env::var_os("COMPUTE_REQUIRE_MLX").is_none(), "{e}");
            None
        }
    }
}
fn traces(p: &MlxCompiledProgram, runs: usize) {
    assert_eq!(
        p.trace_count(),
        if std::env::var_os("MLX_DISABLE_COMPILE").is_some() {
            runs
        } else {
            1
        }
    );
}
fn close(actual: &[f32], expected: &[f32]) {
    assert_eq!(actual.len(), expected.len());
    for (i, (&a, &e)) in actual.iter().zip(expected).enumerate() {
        assert!(
            a.is_finite() && (a - e).abs() <= 5e-5 * e.abs().max(1e-20),
            "at {i}: {a} != {e}"
        );
    }
}
fn encode(d: LowDtype, x: f32) -> u16 {
    match d {
        LowDtype::F16 => half::f16::from_f32(x).to_bits(),
        LowDtype::Bf16 => half::bf16::from_f32(x).to_bits(),
    }
}

// Independent f64 reference for [batch=2,Hq=4,Q=3,D=3] and shared Hkv=2.
fn reference(q: &[f32], k: &[f32], v: &[f32], mask: &[f32], options: AttentionOptions) -> Vec<f32> {
    let mut out = vec![];
    for batch in 0..2 {
        for head in 0..4 {
            for row in 0..3 {
                let logits: Vec<_> = (0..5)
                    .map(|col| {
                        if options
                            .causal
                            .is_some_and(|o| col as i64 > row as i64 + i64::from(o))
                        {
                            return f64::NEG_INFINITY;
                        }
                        let dot: f64 = (0..3)
                            .map(|d| {
                                f64::from(q[(head * 3 + row) * 3 + d])
                                    * f64::from(k[((batch * 2 + head / 2) * 5 + col) * 3 + d])
                            })
                            .sum();
                        dot * f64::from(options.scale.unwrap_or(1. / 3f32.sqrt()))
                            + f64::from(mask[row * 5 + col])
                    })
                    .collect();
                let max = logits.iter().copied().fold(f64::NEG_INFINITY, f64::max);
                let weights: Vec<_> = logits
                    .iter()
                    .map(|&x| if max.is_finite() { (x - max).exp() } else { 0. })
                    .collect();
                let sum: f64 = weights.iter().sum();
                for channel in 0..3 {
                    out.push(if sum == 0. {
                        0.
                    } else {
                        (weights
                            .iter()
                            .enumerate()
                            .map(|(col, &w)| {
                                w * f64::from(v[((batch * 2 + head / 2) * 5 + col) * 3 + channel])
                            })
                            .sum::<f64>()
                            / sum) as f32
                    });
                }
            }
        }
    }
    out
}
fn upload_view(b: &MlxBackend, dims: &[usize], values: &[f32], strided: bool) -> MlxTensor {
    if !strided {
        return b.upload_f32(shape(dims), values).unwrap();
    }
    let rank = dims.len();
    let rows = dims[rank - 2];
    let cols = dims[rank - 1];
    let mut data = vec![0.; values.len()];
    for batch in 0..values.len() / rows / cols {
        for r in 0..rows {
            for c in 0..cols {
                data[(batch * cols + c) * rows + r] = values[(batch * rows + r) * cols + c];
            }
        }
    }
    let mut physical = dims.to_vec();
    physical.swap(rank - 2, rank - 1);
    let t = b.upload_f32(shape(&physical), &data).unwrap();
    let mut axes: Vec<_> = (0..rank).collect();
    axes.swap(rank - 2, rank - 1);
    b.permute(&t, &axes).unwrap()
}

fn upload_low_view(
    b: &MlxBackend,
    dtype: LowDtype,
    dims: &[usize],
    values: &[f32],
    strided: bool,
) -> compute_mlx::MlxLowTensor {
    let low = b
        .upload_low(
            dtype,
            shape(dims),
            &values.iter().map(|&v| encode(dtype, v)).collect::<Vec<_>>(),
        )
        .unwrap();
    if !strided {
        return low;
    }
    let mut axes: Vec<_> = (0..dims.len()).collect();
    axes.swap(dims.len() - 2, dims.len() - 1);
    let transposed = b.permute_low(&low, &axes).unwrap();
    let dense = b.materialize_low(&transposed).unwrap();
    b.permute_low(&dense, &axes).unwrap()
}
