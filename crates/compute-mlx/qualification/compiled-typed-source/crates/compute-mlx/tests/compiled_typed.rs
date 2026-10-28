#![cfg(not(target_arch = "wasm32"))]
use compute_mlx::{MlxBackend, MlxCompiledProgram, MlxDtype, MlxError, MlxProgramOutput};
use half::{bf16, f16};
use tensor_core::{
    BinaryOp, HasLowDtype, HasShape, LowDtype, ReduceOp, Shape, TensorLowBackend, UnaryOp,
};

#[path = "compiled_typed/arithmetic.rs"]
mod arithmetic;
#[path = "compiled_typed/casts.rs"]
mod casts;
#[path = "compiled_typed/matmul.rs"]
mod matmul;
#[path = "compiled_typed/reductions.rs"]
mod reductions;
#[path = "compiled_typed/tiny.rs"]
mod tiny;
#[path = "compiled_typed/unsigned.rs"]
mod unsigned;
#[path = "compiled_typed/validation.rs"]
mod validation;

fn shape(dims: &[usize]) -> Shape {
    Shape::new(dims.to_vec()).unwrap()
}
fn backend() -> Option<MlxBackend> {
    let result = MlxBackend::new_gpu().and_then(|b| {
        if !b.compile_available() {
            Err(MlxError::CompileUnavailable)
        } else if !b.low_precision_support(LowDtype::Bf16).matmul_f32 {
            Err(MlxError::UnsupportedLowPrecision {
                dtype: LowDtype::Bf16,
                operation: "typed compiled custom kernels",
            })
        } else {
            Ok(b)
        }
    });
    match result {
        Ok(b) => Some(b),
        Err(error) => {
            eprintln!("SKIP typed MLX programs unavailable: {error}");
            assert!(std::env::var_os("COMPUTE_REQUIRE_MLX").is_none(), "{error}");
            None
        }
    }
}
fn traces(program: &MlxCompiledProgram, runs: usize) {
    let expected = if std::env::var_os("MLX_DISABLE_COMPILE").is_some() {
        runs
    } else {
        usize::from(runs != 0)
    };
    assert_eq!(program.trace_count(), expected);
}
fn decode(dtype: LowDtype, bits: u16) -> f32 {
    match dtype {
        LowDtype::F16 => f16::from_bits(bits).to_f32(),
        LowDtype::Bf16 => bf16::from_bits(bits).to_f32(),
    }
}
fn encode(dtype: LowDtype, value: f32) -> u16 {
    match dtype {
        LowDtype::F16 => f16::from_f32(value).to_bits(),
        LowDtype::Bf16 => bf16::from_f32(value).to_bits(),
    }
}
fn low_values(
    b: &MlxBackend,
    output: &MlxProgramOutput,
    dims: &[usize],
    dtype: LowDtype,
    expected: &[u16],
) {
    assert_eq!(output.shape(), &shape(dims));
    let low = output.as_low().unwrap();
    assert_eq!(low.low_dtype(), dtype);
    assert_eq!(b.read_low_bits(low).unwrap(), expected);
}
fn floats(b: &MlxBackend, output: &MlxProgramOutput, dims: &[usize], expected: &[f32]) {
    assert_eq!(output.shape(), &shape(dims));
    assert_eq!(output.dtype(), MlxDtype::F32);
    let actual = b.read_f32(output.as_tensor().unwrap()).unwrap();
    assert_eq!(actual.len(), expected.len());
    for (i, (&a, &e)) in actual.iter().zip(expected).enumerate() {
        if e.is_nan() {
            assert!(a.is_nan(), "NaN classification lost at {i}");
        } else {
            assert_eq!(a.to_bits(), e.to_bits(), "f32 bits at {i}: {a:?} vs {e:?}");
        }
    }
}
