#![cfg(not(target_arch = "wasm32"))]
use compute_mlx::{MlxBackend, MlxCompiledProgram, MlxError, MlxProgramOutput};
use half::{bf16, f16};
use tensor_core::{CompareOp, HasShape, LowDtype, Shape, TensorLowBackend};

#[path = "compiled_indexing/low.rs"]
mod low;
#[path = "compiled_indexing/movement.rs"]
mod movement;
#[path = "compiled_indexing/validation.rs"]
mod validation;

const OPS: [CompareOp; 6] = [
    CompareOp::Equal,
    CompareOp::NotEqual,
    CompareOp::Less,
    CompareOp::LessEqual,
    CompareOp::Greater,
    CompareOp::GreaterEqual,
];
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
        Err(error) => {
            eprintln!("SKIP compiled MLX indexing unavailable: {error}");
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
fn compare<T: PartialOrd>(a: T, b: T, op: CompareOp) -> u32 {
    u32::from(match op {
        CompareOp::Equal => a == b,
        CompareOp::NotEqual => a != b,
        CompareOp::Less => a < b,
        CompareOp::LessEqual => a <= b,
        CompareOp::Greater => a > b,
        CompareOp::GreaterEqual => a >= b,
    })
}
fn decode(dtype: LowDtype, bits: u16) -> f32 {
    match dtype {
        LowDtype::F16 => f16::from_bits(bits).to_f32(),
        LowDtype::Bf16 => bf16::from_bits(bits).to_f32(),
    }
}
fn uints(b: &MlxBackend, output: &MlxProgramOutput, dims: &[usize], expected: &[u32]) {
    assert_eq!(output.shape(), &shape(dims));
    assert_eq!(b.read_u32(output.as_tensor().unwrap()).unwrap(), expected);
}
fn bits(b: &MlxBackend, output: &MlxProgramOutput, dims: &[usize], expected: &[u16]) {
    assert_eq!(output.shape(), &shape(dims));
    assert_eq!(b.read_low_bits(output.as_low().unwrap()).unwrap(), expected);
}
